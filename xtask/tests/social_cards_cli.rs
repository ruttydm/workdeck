use std::{fs, process::Command};

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
}
