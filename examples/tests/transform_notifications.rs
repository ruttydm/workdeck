//! Hunk MIT host.test.ts notification sink semantics, extended to native failures.
use std::sync::{Arc, Mutex};
use workdeck_extension_api::ExtensionNotifyType;
use workdeck_extension_host::LoadedExtension;

#[test]
fn notification_precedes_failure_warning_and_previous_changeset_is_preserved() {
    for kind in ["notify-error", "notify-crash", "notify-invalid"] {
        let directory = tempfile::TempDir::new().unwrap();
        let bin = directory.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let executable = format!("probe{}", std::env::consts::EXE_SUFFIX);
        std::fs::copy(
            env!("CARGO_BIN_EXE_workdeck-example-pty-extension-probe-extension"),
            bin.join(&executable),
        )
        .unwrap();
        std::fs::write(directory.path().join("fixture-kind"), kind).unwrap();
        let manifest = directory.path().join("workdeck-extension.toml");
        std::fs::write(&manifest, format!(
            "id = \"notifier\"\nname = \"Notifier\"\nversion = \"0.1.0\"\napi_version = 1\nexecutable = \"bin/{executable}\"\ncapabilities = [\"changeset-transforms\", \"notifications\"]\n"
        )).unwrap();
        let mut extension = LoadedExtension::spawn(&manifest, "test").unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&received);
        let _subscription = extension
            .notifications()
            .subscribe(move |notice| sink.lock().unwrap().push(notice));
        let original = workdeck_diff::parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
            "fixture",
            "Unchanged review",
            workdeck_core::ChangesetSource::Patch {
                label: "fixture".into(),
            },
        )
        .unwrap();
        let result = extension.apply_changeset_transforms(original.clone());
        assert_eq!(result, original, "{kind}");
        let notices = received.lock().unwrap();
        assert_eq!(notices.len(), 2, "{kind}: {notices:?}");
        assert_eq!(notices[0].message, "before failure", "{kind}");
        assert_eq!(notices[0].notification_type, ExtensionNotifyType::Info);
        assert_eq!(notices[1].notification_type, ExtensionNotifyType::Warning);
        assert!(notices[0].id < notices[1].id);
        assert!(notices[1].message.contains("Extension notifier"));
        assert!(notices[1].message.contains(if kind == "notify-invalid" {
            "invalid changeset"
        } else {
            "failed transforming"
        }));
    }
}
