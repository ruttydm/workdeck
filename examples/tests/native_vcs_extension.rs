use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use workdeck_core::{CommonOptions, FileChangeKind, ReviewSide, VcsDiffCommandInput};
use workdeck_extension_api::ExtensionNotifyType;
use workdeck_extension_host::{ExtensionEventBusPhase, LoadedExtension, native_vcs_adapters};
use workdeck_vcs::{
    VcsFileSourceRequest, VcsFileSourceResult, VcsLoadContext, VcsReviewInput,
    create_base_vcs_catalog, create_vcs_watch_plan, create_vcs_watch_signature, detect_vcs,
    get_vcs_adapter, load_vcs_review, materialize_vcs_patch_result, operation_from_input,
};

fn install_fixture(root: &TempDir) -> std::path::PathBuf {
    let extension = root.path().join("native-vcs");
    let bin = extension.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-native-vcs-extension"),
        bin.join("workdeck-example-native-vcs-extension"),
    )
    .unwrap();
    let manifest = extension.join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/native-vcs/workdeck-extension.toml"),
    )
    .unwrap();
    manifest
}

#[test]
fn one_native_process_serves_detection_load_sources_watch_and_retained_tui_clone() {
    let root = TempDir::new().unwrap();
    let manifest = install_fixture(&root);
    let loaded = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let registry = loaded.registry();
    let retained_tui_clone = loaded.clone();
    let notifications = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&notifications);
    let _subscription = loaded.notifications().subscribe(move |notification| {
        captured
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(notification);
    });

    let catalog = create_base_vcs_catalog(
        native_vcs_adapters(std::slice::from_ref(&loaded)),
        "example-vcs",
    );
    let detection = detect_vcs(root.path(), &catalog).unwrap();
    assert_eq!(detection.id, "example-vcs");
    assert_eq!(detection.repo_root, root.path().canonicalize().unwrap());
    let adapter = get_vcs_adapter("example-vcs", &catalog).unwrap();
    let input = VcsReviewInput::Diff(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    });
    let operation = operation_from_input(input);
    let context = VcsLoadContext {
        cwd: root.path().into(),
    };
    let result = load_vcs_review(adapter, &operation, &context, &catalog).unwrap();
    assert_eq!(result.source_cache_key.as_deref(), Some("example-cache-1"));
    let reader = result.source_reader.as_ref().unwrap();
    let old_request = VcsFileSourceRequest {
        path: "tracked.txt".into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        is_untracked: false,
        side: ReviewSide::Old,
    };
    assert!(matches!(
        reader(&old_request),
        Ok(VcsFileSourceResult::Source(_))
    ));
    assert!(matches!(
        reader(&old_request),
        Ok(VcsFileSourceResult::Source(_))
    ));
    let retry_request = VcsFileSourceRequest {
        path: "retry.txt".into(),
        ..old_request.clone()
    };
    assert!(reader(&retry_request).is_err());
    assert!(matches!(
        reader(&retry_request),
        Ok(VcsFileSourceResult::Source(_))
    ));
    let too_large_request = VcsFileSourceRequest {
        path: "too-large.txt".into(),
        ..old_request.clone()
    };
    for _ in 0..2 {
        assert_eq!(
            reader(&too_large_request).unwrap(),
            VcsFileSourceResult::TooLarge { max_bytes: 42 }
        );
    }
    let changeset = materialize_vcs_patch_result(
        result,
        "example-vcs:working",
        workdeck_core::ChangesetSource::WorkingTree { staged: false },
    )
    .unwrap();
    assert_eq!(changeset.files.len(), 1);
    assert_eq!(
        changeset.files[0].sources.old.as_ref().unwrap().content,
        "old\n"
    );
    assert_eq!(
        changeset.files[0].sources.new.as_ref().unwrap().content,
        "new\n"
    );
    assert_eq!(
        create_vcs_watch_signature(adapter, &operation, &context, &catalog).unwrap(),
        "example-vcs:WorkingTreeDiff"
    );
    assert_eq!(
        create_vcs_watch_plan(adapter, &operation, &context, &catalog)
            .unwrap()
            .targets
            .len(),
        1
    );
    assert!(
        notifications
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|notification| {
                notification.notification_type == ExtensionNotifyType::Warning
                    && notification.message.contains("mistyped-example-vcs")
            })
    );

    drop(catalog);
    drop(loaded);
    assert_eq!(registry.phase(), ExtensionEventBusPhase::Ready);
    let mut retained_tui_clone = retained_tui_clone;
    retained_tui_clone.retire();
    assert_eq!(registry.phase(), ExtensionEventBusPhase::Closed);
}
