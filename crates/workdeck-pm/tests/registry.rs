#![cfg(unix)]
use std::{
    fs,
    sync::{Arc, Barrier},
};
use workdeck_pm::{registry::*, *};

fn fixture() -> (tempfile::TempDir, Repository, tempfile::TempDir, Repository) {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    (owner, owner_repo, target, target_repo)
}
fn register(store: &RegistryStore, target: &std::path::Path, alias: &str) -> RegistryRequest {
    RegistryRequest {
        expected: store.snapshot().unwrap().source,
        mutation: RegistryMutation::Register {
            checkout: inspect_checkout(alias, target, SourceSelector::WorkingTree).unwrap(),
        },
    }
}

#[test]
fn empty_reads_are_inert_and_registration_never_writes_to_the_target() {
    let (owner, owner_repo, target, target_repo) = fixture();
    let store = RegistryStore::open(&owner_repo).unwrap();
    assert!(store.snapshot().unwrap().entries.is_empty());
    assert!(!owner.path().join(".workdeck/.local/repositories").exists());
    let target_config = fs::read(target_repo.root().join("config.yml")).unwrap();
    store
        .mutate(
            &register(&store, target.path(), "secondary"),
            &RequestId::new(),
        )
        .unwrap();
    let (entry, resolved) = store.resolve("secondary").unwrap();
    assert_eq!(entry.repository, *target_repo.identity());
    assert_eq!(resolved.identity(), target_repo.identity());
    assert_eq!(
        fs::read(target_repo.root().join("config.yml")).unwrap(),
        target_config
    );
    assert!(!target.path().join(".git").exists());
    assert!(!target_repo.root().join(".local/repositories").exists());
    assert_eq!(
        fs::read(owner_repo.root().join(".local/repositories/.gitignore")).unwrap(),
        b"*\n"
    );
}

#[test]
fn lost_acknowledgement_replays_original_mapping_after_its_removal() {
    let (_owner, owner_repo, target, _) = fixture();
    let store = RegistryStore::open(&owner_repo).unwrap();
    let input = register(&store, target.path(), "secondary");
    let id = RequestId::new();
    let failure = store
        .mutate_with_faults(&input, &id, |point| {
            if point == RegistryFaultPoint::AfterPublish {
                Err(PmError::new(ErrorCode::Io, "lost response"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(failure.code, ErrorCode::Io);
    let after = store.snapshot().unwrap();
    assert_eq!(after.entries.len(), 1);
    store
        .mutate(
            &RegistryRequest {
                expected: after.source,
                mutation: RegistryMutation::Remove {
                    alias: "secondary".into(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    fs::remove_dir_all(target.path().join(".workdeck")).unwrap();
    let replay = store.mutate(&input, &id).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.checkout.alias, "secondary");
    assert!(store.snapshot().unwrap().entries.is_empty());
    let mut changed = input.clone();
    changed.expected = store.snapshot().unwrap().source;
    assert_eq!(
        store.mutate(&changed, &id).unwrap_err().code,
        ErrorCode::Conflict
    );
}

#[test]
fn two_writers_cannot_replace_the_same_inspected_registry() {
    let (_owner, owner_repo, target, _) = fixture();
    let input = register(
        &RegistryStore::open(&owner_repo).unwrap(),
        target.path(),
        "secondary",
    );
    let barrier = Arc::new(Barrier::new(2));
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let owner = owner_repo.clone();
            let input = input.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = RegistryStore::open(&owner).unwrap();
                barrier.wait();
                store.mutate(&input, &RequestId::new())
            })
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|result| result.as_ref().err())
            .unwrap()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn matching_repository_id_does_not_authorize_a_replaced_checkout_directory() {
    let (_owner, owner_repo, target, target_repo) = fixture();
    let store = RegistryStore::open(&owner_repo).unwrap();
    store
        .mutate(
            &register(&store, target.path(), "secondary"),
            &RequestId::new(),
        )
        .unwrap();
    let original = target_repo.root().with_file_name("saved-planning");
    fs::rename(target_repo.root(), &original).unwrap();
    fs::create_dir(target_repo.root()).unwrap();
    fs::copy(
        original.join("config.yml"),
        target_repo.root().join("config.yml"),
    )
    .unwrap();
    assert_eq!(
        Repository::open_source(target_repo.root())
            .unwrap()
            .identity(),
        target_repo.identity()
    );
    assert_eq!(
        store.resolve("secondary").unwrap_err().code,
        ErrorCode::StaleSource
    );
}

#[test]
fn registry_paths_reject_symlinks_and_preserve_existing_ignore_policy() {
    use std::os::unix::fs::symlink;
    let (_owner, owner_repo, target, _) = fixture();
    let store = RegistryStore::open(&owner_repo).unwrap();
    let input = register(&store, target.path(), "secondary");
    let directory = owner_repo.root().join(".local/repositories");
    fs::create_dir_all(directory.parent().unwrap()).unwrap();
    symlink(target.path(), &directory).unwrap();
    assert_eq!(store.snapshot().unwrap_err().code, ErrorCode::UnsafePath);
    assert_eq!(
        store.mutate(&input, &RequestId::new()).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    fs::remove_file(&directory).unwrap();
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join(".gitignore"), "keep\n").unwrap();
    assert_eq!(
        store.mutate(&input, &RequestId::new()).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        fs::read_to_string(directory.join(".gitignore")).unwrap(),
        "keep\n"
    );
    assert!(!directory.join("registry.json").exists());
}

#[test]
fn unavailable_mappings_are_visible_and_removable_without_opening_the_target() {
    let (_owner, owner_repo, target, _) = fixture();
    let store = RegistryStore::open(&owner_repo).unwrap();
    store
        .mutate(
            &register(&store, target.path(), "secondary"),
            &RequestId::new(),
        )
        .unwrap();
    fs::remove_dir_all(target.path().join(".workdeck")).unwrap();
    assert!(store.resolve("secondary").is_err());
    let state = store.snapshot().unwrap();
    assert_eq!(state.entries.len(), 1);
    store
        .mutate(
            &RegistryRequest {
                expected: state.source,
                mutation: RegistryMutation::Remove {
                    alias: "secondary".into(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    assert!(store.snapshot().unwrap().entries.is_empty());
}

#[test]
fn replacing_a_reviewed_target_before_publication_rejects_the_registration() {
    let (_owner, owner_repo, target, target_repo) = fixture();
    let store = RegistryStore::open(&owner_repo).unwrap();
    let input = register(&store, target.path(), "secondary");
    let outcome = store.mutate_with_faults(&input, &RequestId::new(), |point| {
        if point == RegistryFaultPoint::BeforePublish {
            let saved = target_repo.root().with_file_name("old-planning");
            fs::rename(target_repo.root(), &saved).unwrap();
            fs::create_dir(target_repo.root()).unwrap();
            fs::copy(
                saved.join("config.yml"),
                target_repo.root().join("config.yml"),
            )
            .unwrap();
        }
        Ok(())
    });
    assert_eq!(outcome.unwrap_err().code, ErrorCode::StaleSource);
    assert!(store.snapshot().unwrap().entries.is_empty());
}
