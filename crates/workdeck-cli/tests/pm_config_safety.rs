#![cfg(unix)]
use assert_cmd::prelude::*;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn run_bounded(root: &Path, args: &[&str]) -> std::process::ExitStatus {
    let mut child = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config-home"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("configuration read blocked: {args:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn config_inspection_rejects_fifo_and_oversized_files_without_blocking() {
    for oversized in [false, true] {
        let root = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
        fs::create_dir(root.path().join(".workdeck")).unwrap();
        let path = root.path().join(".workdeck/config.toml");
        if oversized {
            fs::write(
                &path,
                format!("#{}\nmode='split'\n", "a".repeat(2 * 1024 * 1024)),
            )
            .unwrap();
        } else {
            let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        }
        for args in [
            vec!["config", "get", "mode", "--json"],
            vec!["config", "show", "--json"],
            vec!["config", "validate", "--json"],
        ] {
            assert!(
                !run_bounded(root.path(), &args).success(),
                "unsafe configuration accepted: {args:?}"
            );
        }
    }
}

#[test]
fn explicit_user_config_symlink_keeps_its_existing_read_only_compatibility() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir_all(root.path().join("config-home/workdeck")).unwrap();
    fs::write(
        root.path().join("dotfile.toml"),
        "mode='split'\ntab_width=3\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        root.path().join("dotfile.toml"),
        root.path().join("config-home/workdeck/config.toml"),
    )
    .unwrap();
    assert!(run_bounded(root.path(), &["config", "validate", "--json"]).success());
    assert_eq!(
        fs::read_to_string(root.path().join("dotfile.toml")).unwrap(),
        "mode='split'\ntab_width=3\n"
    );
}
