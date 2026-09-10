use std::{fs, path::Path, process::Command};

#[test]
fn site_commands_reject_changed_assets_before_starting_zola() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::create_dir_all(repo.path().join("site/data")).unwrap();
    fs::create_dir_all(repo.path().join("site/static/fonts")).unwrap();
    fs::copy(
        source.join("site/data/third-party-assets.json"),
        repo.path().join("site/data/third-party-assets.json"),
    )
    .unwrap();
    let font = repo
        .path()
        .join("site/static/fonts/jetbrains-mono-latin-wght-normal.woff2");
    fs::write(&font, b"changed asset").unwrap();
    for subcommand in ["build", "check", "serve"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .current_dir(repo.path())
            .args(["site", subcommand])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("website asset hash mismatch"),
            "{output:?}"
        );
        assert_eq!(fs::read(&font).unwrap(), b"changed asset");
        assert!(!repo.path().join("site/public").exists());
        assert!(!repo.path().join(".agents").exists());
    }
}
