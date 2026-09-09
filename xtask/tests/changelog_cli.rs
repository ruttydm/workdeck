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

#[test]
fn version_plan_uses_real_cargo_metadata_without_mutating_inputs() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::write(repo.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(repo.path().join("Cargo.toml"), "[package]\nname = \"workdeck-cli\"\nversion = \"1.2.3\"\nedition = \"2024\"\n\n[workspace]\n").unwrap();
    assert!(
        Command::new("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success()
    );
    for (id, bump, body) in [
        ("minor-feature", "minor", "Add feature."),
        ("patch-fix", "patch", "Fix λ."),
    ] {
        let output = run(repo.path(), &["changelog", "add", id, bump, body]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let paths = [
        "Cargo.toml",
        "Cargo.lock",
        "changes/minor-feature.md",
        "changes/patch-fix.md",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|path| std::fs::read(repo.path().join(path)).unwrap())
        .collect();
    let output = run(repo.path(), &["changelog", "plan"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["current"], "1.2.3");
    assert_eq!(plan["next"], "1.3.0");
    assert_eq!(plan["bump"], "minor");
    assert_eq!(plan["applied"], false);
    assert_eq!(plan["fragments"].as_array().unwrap().len(), 2);
    assert_eq!(
        plan["notes"],
        "## 1.3.0\n\n### Minor Changes\n\n- Add feature.\n\n### Patch Changes\n\n- Fix λ.\n\n"
    );
    for (path, expected) in paths.iter().zip(before) {
        use sha2::{Digest, Sha256};
        assert_eq!(
            plan["inputs"][path],
            format!("{:x}", Sha256::digest(&expected))
        );
        assert_eq!(
            std::fs::read(repo.path().join(path)).unwrap(),
            expected,
            "plan changed {path}"
        );
    }
}
