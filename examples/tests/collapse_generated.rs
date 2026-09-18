use std::sync::{Arc, Mutex};
use workdeck_extension_api::ExtensionNotifyType;
use workdeck_extension_host::LoadedExtension;

#[test]
fn compiled_extension_filters_and_notifies_through_native_host() {
    let directory = tempfile::TempDir::new().unwrap();
    let binary = format!(
        "workdeck-example-collapse-generated-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    std::fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-collapse-generated-extension"),
        directory.path().join(&binary),
    )
    .unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    std::fs::write(
        &manifest,
        include_str!("../extensions/collapse-generated/workdeck-extension.toml").replace(
            "executable = \"workdeck-example-collapse-generated-extension\"",
            &format!("executable = {binary:?}"),
        ),
    )
    .unwrap();
    let mut extension = LoadedExtension::spawn(&manifest, "test").unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&received);
    let _subscription = extension.notifications().subscribe(move |notification| {
        sink.lock().unwrap().push(notification.clone());
    });
    let patch = ["Cargo.lock", "src/main.rs", "dist/output"].map(|path| {
        format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+new\n")
    }).join("");
    let original = workdeck_diff::parse_patch(
        &patch,
        "fixture",
        "Generated files",
        workdeck_core::ChangesetSource::Patch {
            label: "fixture".into(),
        },
    )
    .unwrap();
    let result = extension.apply_changeset_transforms(original.clone());
    assert_eq!(result.files, vec![original.files[1].clone()]);
    assert_eq!(result.title, original.title);
    let notices = received.lock().unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].message, "Collapsed 2 generated files");
    assert_eq!(notices[0].notification_type, ExtensionNotifyType::Info);
}
