use std::{fs, path::Path, process::Command};

#[test]
fn site_commands_reject_stale_web_skill_without_repairing_it() {
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
    let inventory = "site/data/third-party-assets.json";
    let assets: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join(inventory)).unwrap()).unwrap();
    let mut paths = vec![
        inventory.to_owned(),
        "skills/workdeck-review/SKILL.md".to_owned(),
    ];
    for asset in assets.as_array().unwrap() {
        for file in asset["files"].as_array().unwrap() {
            paths.push(file["path"].as_str().unwrap().to_owned());
        }
    }
    for path in paths {
        let target = repo.path().join(&path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(source.join(path), target).unwrap();
    }
    let web_skill = repo
        .path()
        .join("site/static/docs/workdeck-review-skill.md");
    fs::create_dir_all(web_skill.parent().unwrap()).unwrap();
    fs::write(&web_skill, "stale").unwrap();
    for subcommand in ["build", "check", "serve"] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .current_dir(repo.path())
            .args(["site", subcommand])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("site/static/docs/workdeck-review-skill.md is out of date"),
            "{error}"
        );
        assert_eq!(fs::read(&web_skill).unwrap(), b"stale");
        assert!(!repo.path().join("site/public").exists());
        assert!(!repo.path().join(".agents").exists());
    }
}

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
