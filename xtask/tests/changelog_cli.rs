use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(repo: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn native_fragment_cli_round_trip_and_read_only_status() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .arg("init")
            .arg("--quiet")
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let empty = run(repo.path(), &["changelog", "status"]);
    assert!(empty.status.success());
    assert!(empty.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&empty.stdout).unwrap(),
        serde_json::json!({"bump":null,"fragments":[]})
    );
    assert!(!repo.path().join("changes").exists());
    let added = run(
        repo.path(),
        &["changelog", "add", "fix-unicode", "patch", "Fix λ."],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert!(added.stderr.is_empty());
    assert_eq!(added.stdout, b"changes/fix-unicode.md\n");
    let path = repo.path().join("changes/fix-unicode.md");
    let original = std::fs::read(&path).unwrap();
    let status = run(repo.path(), &["changelog", "status"]);
    assert!(status.status.success());
    assert!(status.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&status.stdout).unwrap(),
        serde_json::json!({"bump":"patch","fragments":[{"id":"fix-unicode","bump":"patch","body":"Fix λ."}]})
    );
    for args in [
        vec!["changelog", "add", "fix-unicode", "major", "Overwrite"],
        vec!["changelog", "status", "unexpected"],
        vec!["changelog", "plan", "unexpected"],
    ] {
        let rejected = run(repo.path(), &args);
        assert!(!rejected.status.success());
        assert!(rejected.stdout.is_empty());
        assert!(!rejected.stderr.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    assert_eq!(
        std::fs::read_dir(repo.path().join("changes"))
            .unwrap()
            .count(),
        1
    );
    let help = run(repo.path(), &["help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for command in ["changelog add", "changelog status", "changelog plan"] {
        assert!(help.contains(command));
    }
}
