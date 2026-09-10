use std::{fs, process::Command};

#[test]
fn publication_cli_applies_checks_staleness_and_preserves_targeted_images() {
    use sha2::{Digest, Sha256};
    let repo = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let backups = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(repo.path().join("cards.json"), b"[]").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .current_dir(repo.path())
            .args(args)
            .output()
            .unwrap()
    };
    let targets = run(&["social-cards-plan", "cards.json", "extensions"]);
    assert!(targets.status.success());
    let targets: serde_json::Value = serde_json::from_slice(&targets.stdout).unwrap();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1200, 630);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![0; 1200 * 630 * 3])
            .unwrap();
    }
    fs::write(staging.path().join("0000.png"), &bytes).unwrap();
    let manifest = serde_json::json!({"schema":1,"stagingDirectory":staging.path().canonicalize().unwrap(),
        "rendered":true,"published":false,"replaceChangelogDirectory":false,
        "images":[{"stagedFile":"0000.png","target":targets["targets"][0],"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}]});
    fs::write(
        staging.path().join("capture.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    fs::create_dir_all(repo.path().join("site/static/changelog/og")).unwrap();
    let stale = repo.path().join("site/static/changelog/og/stale.png");
    fs::write(&stale, b"untouched").unwrap();
    let stage = staging.path().to_str().unwrap();
    let plan = run(&[
        "social-cards-publication-plan",
        stage,
        "cards.json",
        "extensions",
    ]);
    assert!(plan.status.success(), "{plan:?}");
    fs::write(repo.path().join("plan.json"), &plan.stdout).unwrap();
    let backup = backups.path().join("first");
    let publish = run(&[
        "social-cards-publish",
        "plan.json",
        backup.to_str().unwrap(),
        stage,
        "cards.json",
        "extensions",
    ]);
    assert!(publish.status.success(), "{publish:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&publish.stdout).unwrap(),
        serde_json::json!({"applied":true,"files":1})
    );
    assert_eq!(
        fs::read(repo.path().join("site/static/extensions/og.png")).unwrap(),
        bytes
    );
    assert_eq!(fs::read(&stale).unwrap(), b"untouched");
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(backup.join("recovery.json")).unwrap()).unwrap();
    assert_eq!(
        saved,
        serde_json::from_slice::<serde_json::Value>(&plan.stdout).unwrap()
    );
    let second_backup = backups.path().join("second");
    let args = [
        "social-cards-publish",
        "plan.json",
        second_backup.to_str().unwrap(),
        stage,
        "cards.json",
        "extensions",
    ];
    let stale_plan = run(&args);
    assert!(!stale_plan.status.success());
    assert!(stale_plan.stdout.is_empty());
    assert!(String::from_utf8_lossy(&stale_plan.stderr).contains("stale or modified"));
    assert!(!second_backup.exists());
    let current = run(&[
        "social-cards-publication-plan",
        stage,
        "cards.json",
        "extensions",
    ]);
    assert!(current.status.success());
    fs::write(repo.path().join("plan.json"), current.stdout).unwrap();
    let unchanged = run(&args);
    assert!(unchanged.status.success(), "{unchanged:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&unchanged.stdout).unwrap(),
        serde_json::json!({"applied":false,"files":0})
    );
    assert!(!second_backup.exists());
    assert_eq!(fs::read(staging.path().join("0000.png")).unwrap(), bytes);
    assert_eq!(fs::read(repo.path().join("cards.json")).unwrap(), b"[]");
    assert!(!repo.path().join(".agents").exists());
}

#[test]
fn saved_capture_cli_validates_hashes_without_changing_inputs() {
    use sha2::{Digest, Sha256};
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(repo.path().join("cards.json"), b"[]").unwrap();
    let plan = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo.path())
        .args(["social-cards-plan", "cards.json", "extensions"])
        .output()
        .unwrap();
    assert!(plan.status.success());
    let plan: serde_json::Value = serde_json::from_slice(&plan.stdout).unwrap();
    let staging = tempfile::tempdir().unwrap();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1200, 630);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![0; 1200 * 630 * 3])
            .unwrap();
    }
    fs::write(staging.path().join("0000.png"), &bytes).unwrap();
    let manifest = serde_json::json!({"schema":1,"stagingDirectory":staging.path().canonicalize().unwrap(),
        "rendered":true,"published":false,"replaceChangelogDirectory":false,
        "images":[{"stagedFile":"0000.png","target":plan["targets"][0],"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))}]});
    let encoded = serde_json::to_vec_pretty(&manifest).unwrap();
    fs::write(staging.path().join("capture.json"), &encoded).unwrap();
    let check = || {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .current_dir(repo.path())
            .args([
                "social-cards-check",
                staging.path().to_str().unwrap(),
                "cards.json",
                "extensions",
            ])
            .output()
            .unwrap()
    };
    let valid = check();
    assert!(valid.status.success(), "{valid:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&valid.stdout).unwrap(),
        serde_json::json!({"valid":true,"published":false,"images":1})
    );
    fs::write(staging.path().join("0000.png"), b"changed").unwrap();
    let invalid = check();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("stale, modified"));
    assert_eq!(
        fs::read(staging.path().join("0000.png")).unwrap(),
        b"changed"
    );
    assert_eq!(
        fs::read(staging.path().join("capture.json")).unwrap(),
        encoded
    );
    assert_eq!(fs::read(repo.path().join("cards.json")).unwrap(), b"[]");
    assert!(!repo.path().join("site").exists());
    assert!(!repo.path().join(".agents").exists());
}

#[test]
fn card_planning_is_read_only_and_distinguishes_full_and_targeted_runs() {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let input =
        br#"[{"slug":"index","title":"Changelog","meta":"Releases","alt":"Release history"}]"#;
    fs::write(repo.path().join("cards.json"), input).unwrap();
    for (slugs, full, count) in [(vec![], true, 2), (vec!["extensions"], false, 1)] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .current_dir(repo.path())
            .args(["social-cards-plan", "cards.json"])
            .args(slugs)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(plan["replaceChangelogDirectory"], full);
        assert_eq!(plan["rendered"], false);
        assert_eq!(plan["width"], 1200);
        assert_eq!(plan["height"], 630);
        assert_eq!(plan["targets"].as_array().unwrap().len(), count);
    }
    let unknown = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo.path())
        .args(["social-cards-plan", "cards.json", "unknown"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(unknown.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("Known slugs: index, extensions"));
    assert_eq!(fs::read(repo.path().join("cards.json")).unwrap(), input);
    assert!(!repo.path().join("site").exists());
    assert!(!repo.path().join(".agents").exists());
    fs::write(repo.path().join("font.woff2"), b"fixture font bytes").unwrap();
    let html = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo.path())
        .args([
            "social-cards-html",
            "cards.json",
            "font.woff2",
            "extensions",
        ])
        .output()
        .unwrap();
    assert!(html.status.success(), "{html:?}");
    let pages: serde_json::Value = serde_json::from_slice(&html.stdout).unwrap();
    assert_eq!(pages.as_object().unwrap().len(), 1);
    let document = pages["site/static/extensions/og.png"].as_str().unwrap();
    assert!(document.contains("<!doctype html>"));
    assert!(document.contains("width: 1200px"));
    assert!(document.contains("workdeck.dev/extensions"));
    assert!(!document.contains("<script"));
    assert!(!repo.path().join("site").exists());
    assert_eq!(
        fs::read(repo.path().join("font.woff2")).unwrap(),
        b"fixture font bytes"
    );
    let failed_capture = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repo.path())
        .args([
            "social-cards-capture",
            "cards.json",
            "font.woff2",
            "missing-driver",
            "missing-browser",
            "extensions",
        ])
        .output()
        .unwrap();
    assert!(!failed_capture.status.success());
    assert!(failed_capture.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed_capture.stderr).contains("launch WebDriver"));
    assert!(!repo.path().join("site").exists());
}
