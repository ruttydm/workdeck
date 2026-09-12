use workdeck_pm::{registry::*, *};

#[test]
fn prepared_navigation_rechecks_mapping_and_original_owner_identity() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    let store = RegistryStore::open(&repo).unwrap();
    let checkout =
        inspect_checkout("secondary", target.path(), SourceSelector::WorkingTree).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: checkout.clone(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let navigation = store.prepare_navigation(&checkout).unwrap();
    assert_eq!(
        navigation.revalidate().unwrap().identity(),
        target_repo.identity()
    );
    let directory = repo.root().join(".local/repositories");
    let saved = repo.root().join(".local/previous-registry");
    // A copied registry does not turn a replaced owner directory into the
    // original authority carried by the prepared navigation handle.
    let original = repo.root().with_file_name("original-planning");
    std::fs::rename(repo.root(), &original).unwrap();
    std::fs::create_dir(repo.root()).unwrap();
    std::fs::copy(original.join("config.yml"), repo.root().join("config.yml")).unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::copy(
        original.join(".local/repositories/registry.json"),
        directory.join("registry.json"),
    )
    .unwrap();
    assert!(navigation.revalidate().is_err());
    std::fs::remove_dir_all(repo.root()).unwrap();
    std::fs::rename(original, repo.root()).unwrap();
    assert!(navigation.revalidate().is_ok());
    std::fs::rename(&directory, &saved).unwrap();
    assert!(navigation.revalidate().is_err());
}
