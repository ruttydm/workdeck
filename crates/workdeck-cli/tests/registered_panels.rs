use std::fs;
use workdeck_cli::repository_panels::RepositoryPanels;
use workdeck_pm::{Repository, RequestId, SourceSelector, registry::*};
use workdeck_tui::workbench::RepositoryPanelProvider;

fn register(owner: &Repository, checkout: RegisteredCheckout) {
    let store = RegistryStore::open(owner).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register { checkout },
            },
            &RequestId::new(),
        )
        .unwrap();
}

#[test]
fn registered_worktree_provider_keeps_both_roots_separate_and_rejects_removed_mappings() {
    let origin = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner = Repository::init(origin.path(), "WD").unwrap();
    Repository::init(target.path(), "WD").unwrap();
    fs::write(origin.path().join("same.rs"), "original owner bytes").unwrap();
    fs::write(target.path().join("same.rs"), "selected checkout bytes").unwrap();
    let mapping =
        inspect_checkout("secondary", target.path(), SourceSelector::WorkingTree).unwrap();
    let panels = RepositoryPanels::new(origin.path(), None, 10).unwrap();
    assert!(
        panels.open_registered_worktree(&mapping).is_err(),
        "inspection alone does not register a navigation source"
    );
    register(&owner, mapping.clone());
    let selected = panels.open_registered_worktree(&mapping).unwrap();
    assert_eq!(selected.source().root, mapping.checkout);
    assert_eq!(
        selected
            .read_file_for_navigation(&mapping.checkout.join("same.rs"))
            .unwrap(),
        b"selected checkout bytes"
    );
    assert!(
        selected
            .read_file_for_navigation(&panels.source().root.join("same.rs"))
            .is_err()
    );
    assert!(
        panels
            .read_file_for_navigation(&mapping.checkout.join("same.rs"))
            .is_err()
    );
    assert_eq!(
        panels
            .read_file_for_navigation(&panels.source().root.join("same.rs"))
            .unwrap(),
        b"original owner bytes"
    );
    let store = RegistryStore::open(&owner).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Remove {
                    alias: "secondary".into(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    assert!(panels.open_registered_worktree(&mapping).is_err());
    // A previously opened provider keeps its own explicit source binding; registry
    // removal cannot redirect it to an unrelated file tree.
    assert_eq!(
        selected
            .read_file_for_navigation(&mapping.checkout.join("same.rs"))
            .unwrap(),
        b"selected checkout bytes"
    );
}

#[test]
fn immutable_source_mapping_cannot_become_a_working_tree_file_provider() {
    let origin = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner = Repository::init(origin.path(), "WD").unwrap();
    Repository::init(target.path(), "WD").unwrap();
    let mapping = inspect_checkout("accepted", target.path(), SourceSelector::Accepted).unwrap();
    register(&owner, mapping.clone());
    let panels = RepositoryPanels::new(origin.path(), None, 10).unwrap();
    assert!(panels.open_registered_worktree(&mapping).is_err());
}
