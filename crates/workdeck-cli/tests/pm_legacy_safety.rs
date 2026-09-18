#[path = "support/legacy.rs"]
mod legacy_fixture;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use workdeck_cli::store::WorkdeckStore;

#[test]
fn legacy_store_never_reads_native_authority() {
    let root = tempfile::tempdir().unwrap();
    let native = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let original = fs::read(native.root().join("config.yml")).unwrap();
    let store = WorkdeckStore::new(native.root());
    assert!(store.load_issues().is_err());
    assert!(store.load_reference_data().is_err());
    assert!(store.load_agent_sessions().is_err());
    assert!(store.load_events().is_err());
    assert!(store.legacy_export_document().is_err());
    assert_eq!(
        fs::read(native.root().join("config.yml")).unwrap(),
        original
    );
    assert!(!native.root().join("projects.toml").exists());
    assert!(!native.root().join("events.jsonl").exists());
}

#[test]
fn legacy_reader_worker() {
    let Ok(root) = std::env::var("WORKDECK_LEGACY_READ_FIXTURE") else {
        return;
    };
    let store = WorkdeckStore::new(root);
    let failed = match std::env::var("WORKDECK_LEGACY_READ_KIND").unwrap().as_str() {
        "issues" => store.load_issues().is_err(),
        "agents" => store.load_agent_sessions().is_err(),
        "references" => store.load_reference_data().is_err(),
        "events" => store.load_events().is_err(),
        "preview_import" => store.preview_legacy_import(&serde_json::json!({"issues":[],"projects":[],"cycles":[],"labels":[],"agent_sessions":[],"events":[]}), true).is_err(),
        _ => panic!("unknown test case"),
    };
    assert!(failed, "unsafe source was accepted");
}

#[cfg(unix)]
#[test]
fn legacy_readers_reject_special_and_linked_sources_with_bounded_execution() {
    for (kind, path) in [
        ("issues", "issues/WD-1.toml"),
        ("agents", "agents/one.toml"),
        ("references", "projects.toml"),
        ("events", "events.jsonl"),
        ("preview_import", "events.jsonl"),
    ] {
        for symlink in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let store = WorkdeckStore::new(root.path());
            legacy_fixture::init(store.root());
            let path = root.path().join(path);
            if path.exists() {
                fs::remove_file(&path).unwrap();
            }
            if symlink {
                let outside = root.path().join("outside.toml");
                fs::write(
                    &outside,
                    if kind == "issues" {
                        "key='WD-1'\ntitle='outside'\n"
                    } else if kind == "agents" {
                        "id='one'\ntitle='outside'\n"
                    } else {
                        ""
                    },
                )
                .unwrap();
                std::os::unix::fs::symlink(outside, &path).unwrap();
            } else {
                let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            }
            assert_worker_finishes(root.path(), kind);
            if symlink && kind == "preview_import" {
                assert_eq!(fs::read(root.path().join("outside.toml")).unwrap(), b"");
            }
        }
    }
}

#[cfg(unix)]
fn assert_worker_finishes(root: &Path, kind: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "legacy_reader_worker", "--test-threads=1"])
        .env("WORKDECK_LEGACY_READ_FIXTURE", root)
        .env("WORKDECK_LEGACY_READ_KIND", kind)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{kind} did not reject unsafe input");
            return;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("{kind} reader blocked on a special file");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
