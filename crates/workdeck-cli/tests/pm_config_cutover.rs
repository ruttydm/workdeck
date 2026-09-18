use assert_cmd::prelude::*;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("HOME", root.join("test-home"))
        .env("XDG_CONFIG_HOME", root.join("test-home/config"))
        .args(args)
        .output()
        .unwrap()
}

fn success(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    root
}

#[test]
fn app_config_init_and_path_use_native_root_without_initializing_planning() {
    let root = repository();
    success(root.path(), &["config", "init", "--json"]);
    let path = success(root.path(), &["config", "path", "--json"]);
    assert_eq!(
        path["data"]["path"],
        root.path()
            .canonicalize()
            .unwrap()
            .join(".workdeck/config.toml")
            .to_str()
            .unwrap()
    );
    assert!(root.path().join(".workdeck/config.toml").is_file());
    assert!(!root.path().join(".agents").exists());
    let status = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(status.contains(".workdeck/config.toml"));
    assert!(
        !status.contains(".local"),
        "local lock must not enter version control: {status}"
    );
    for marker in [
        "config.yml",
        "issues",
        "projects.toml",
        "cycles.toml",
        "labels.toml",
    ] {
        assert!(
            !root.path().join(".workdeck").join(marker).exists(),
            "unexpected PM initialization: {marker}"
        );
    }
}

#[test]
fn config_init_after_pm_init_does_not_add_legacy_authority() {
    let root = repository();
    success(root.path(), &["init", "--json"]);
    let original = fs::read(root.path().join(".workdeck/config.yml")).unwrap();
    success(root.path(), &["config", "init", "--json"]);
    success(root.path(), &["config", "init", "--json"]);
    assert_eq!(
        fs::read(root.path().join(".workdeck/config.yml")).unwrap(),
        original
    );
    for marker in ["agents", "projects.toml", "cycles.toml", "labels.toml"] {
        assert!(
            !root.path().join(".workdeck").join(marker).exists(),
            "unexpected legacy path: {marker}"
        );
    }
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn config_set_validates_before_publication_and_preserves_toml_comments() {
    let root = repository();
    fs::create_dir(root.path().join(".workdeck")).unwrap();
    let path = root.path().join(".workdeck/config.toml");
    let original = "# Keep this explanation\nmode = 'split' # user's choice\n\n[git]\nrecent_commits = 12 # history window\n";
    fs::write(&path, original).unwrap();
    assert!(
        !run(root.path(), &["config", "set", "tab_width", "0", "--json"])
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    success(root.path(), &["config", "set", "mode", "stack", "--json"]);
    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("# Keep this explanation"));
    assert!(updated.contains("# user's choice"));
    assert!(updated.contains("recent_commits = 12 # history window"));
    assert_eq!(
        toml::from_str::<toml::Value>(&updated).unwrap()["mode"].as_str(),
        Some("stack")
    );
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn explicit_config_set_moves_preserved_legacy_preferences_into_native_root() {
    let root = repository();
    fs::create_dir_all(root.path().join(".agents/workdeck")).unwrap();
    let path = root.path().join(".agents/workdeck/config.toml");
    let original = "# authored legacy settings\nmode = 'split' # keep choice comment\n\n[future]\nflag = true # unknown setting retained\n";
    fs::write(&path, original).unwrap();
    let selected = success(root.path(), &["config", "path", "--json"]);
    assert_eq!(
        selected["data"]["path"],
        path.canonicalize().unwrap().to_str().unwrap()
    );
    success(root.path(), &["config", "set", "mode", "stack", "--json"]);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let native = root.path().join(".workdeck/config.toml");
    let updated = fs::read_to_string(&native).unwrap();
    assert_eq!(
        toml::from_str::<toml::Value>(&updated).unwrap()["mode"].as_str(),
        Some("stack")
    );
    assert!(updated.contains("# authored legacy settings"));
    assert!(updated.contains("# keep choice comment"));
    assert!(updated.contains("flag = true # unknown setting retained"));
    let selected = success(root.path(), &["config", "path", "--json"]);
    assert_eq!(
        selected["data"]["path"],
        native.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(!path.parent().unwrap().join(".local").exists());
}

#[test]
fn explicit_config_init_copies_complete_legacy_bytes_once_without_planning_cutover() {
    let root = repository();
    let legacy = root.path().join(".agents/workdeck");
    fs::create_dir_all(legacy.join("issues")).unwrap();
    let original = "# exact bytes\r\nmode = 'split'\r\n[paths]\r\ndata_dir = '.agents/workdeck'\r\n[future]\r\nitems = [1, 2] # preserved\r\n";
    fs::write(legacy.join("config.toml"), original).unwrap();
    success(root.path(), &["config", "init", "--json"]);
    let native = root.path().join(".workdeck/config.toml");
    assert_eq!(fs::read(&native).unwrap(), original.as_bytes());
    assert_eq!(
        fs::read(legacy.join("config.toml")).unwrap(),
        original.as_bytes()
    );
    success(root.path(), &["config", "init", "--json"]);
    assert_eq!(fs::read(&native).unwrap(), original.as_bytes());
    let config = success(root.path(), &["config", "show", "--json"]);
    assert_eq!(config["data"]["paths"]["data_dir"], ".agents/workdeck");
    assert!(!root.path().join(".workdeck/config.yml").exists());
    assert!(!legacy.join(".local").exists());
}

#[test]
fn invalid_legacy_preference_edit_never_publishes_canonical_config() {
    let root = repository();
    let legacy = root.path().join(".agents/workdeck");
    fs::create_dir_all(&legacy).unwrap();
    let original = "# remains authoritative\nmode = 'split'\n";
    fs::write(legacy.join("config.toml"), original).unwrap();
    let output = run(root.path(), &["config", "set", "tab_width", "0", "--json"]);
    assert!(!output.status.success());
    assert!(!root.path().join(".workdeck/config.toml").exists());
    assert!(!root.path().join(".workdeck").exists());
    assert_eq!(
        fs::read(legacy.join("config.toml")).unwrap(),
        original.as_bytes()
    );
    assert!(!legacy.join(".local").exists());
}

#[test]
fn orphaned_organization_and_hierarchy_sources_never_select_legacy_preferences() {
    for marker in [
        "users.yml",
        "schema.yml",
        "initiatives/old/item.md",
        "milestones/old/item.md",
        "targets/old/item.md",
        "wiki/old.md",
        "views/old.yml",
        "features/old.md",
        "gates/old.yml",
        "relations/issues/old.yml",
        "evidence/old.yml",
        "questions/old.md",
        "commands/old.yml",
        "checks/old.yml",
        "check-profiles/old.yml",
        "runs/old/intent.yml",
    ] {
        let root = repository();
        let native = root.path().join(".workdeck");
        let record = native.join(marker);
        fs::create_dir_all(record.parent().unwrap()).unwrap();
        fs::write(&record, "orphan authority; do not overwrite\n").unwrap();
        let legacy = root.path().join(".agents/workdeck");
        fs::create_dir_all(&legacy).unwrap();
        let old_config = "# retained legacy preferences\n[paths]\ndata_dir='shadow'\n";
        fs::write(legacy.join("config.toml"), old_config).unwrap();
        let selected = success(root.path(), &["config", "path", "--json"]);
        assert_eq!(
            selected["data"]["path"],
            native
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .join(".workdeck/config.toml")
                .to_str()
                .unwrap(),
            "{marker}"
        );
        for args in [
            &["issue", "list", "--json"][..],
            &["export", "--json"][..],
            &["command", "list", "--json"][..],
            &["check", "plan", "--check", "unit", "--json"][..],
        ] {
            let rejected = run(root.path(), args);
            assert!(!rejected.status.success(), "{marker} {args:?}");
            let value: Value = serde_json::from_slice(&rejected.stdout).unwrap();
            assert_eq!(
                value["error"]["code"], "not_initialized",
                "{marker}: {value}"
            );
        }
        assert_eq!(
            fs::read_to_string(&record).unwrap(),
            "orphan authority; do not overwrite\n"
        );
        assert_eq!(
            fs::read_to_string(legacy.join("config.toml")).unwrap(),
            old_config
        );
        assert!(!native.join("config.yml").exists());
        assert!(!native.join("config.toml").exists());
        assert!(!root.path().join("shadow").exists());
        fs::create_dir(legacy.join("issues")).unwrap();
        assert!(
            !run(root.path(), &["config", "path", "--json"])
                .status
                .success(),
            "{marker}: source ambiguity was ignored"
        );
    }
}

#[cfg(unix)]
#[test]
fn config_set_rejects_a_symlink_without_touching_its_target() {
    let root = repository();
    fs::create_dir(root.path().join(".workdeck")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("config.toml");
    fs::write(&target, "mode = 'split'\n").unwrap();
    std::os::unix::fs::symlink(&target, root.path().join(".workdeck/config.toml")).unwrap();
    assert!(
        !run(root.path(), &["config", "set", "mode", "stack", "--json"])
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(target).unwrap(), "mode = 'split'\n");
}

#[test]
fn config_set_can_repair_invalid_preferences_and_edit_inline_tables() {
    let root = repository();
    fs::create_dir(root.path().join(".workdeck")).unwrap();
    let path = root.path().join(".workdeck/config.toml");
    fs::write(
        &path,
        "tab_width = 0\nui = { preview = true } # retained inline preferences\n",
    )
    .unwrap();
    success(root.path(), &["config", "set", "tab_width", "4", "--json"]);
    success(
        root.path(),
        &["config", "set", "ui.preview", "false", "--json"],
    );
    let raw = fs::read_to_string(&path).unwrap();
    assert!(raw.contains("# retained inline preferences"));
    let parsed: toml::Value = toml::from_str(&raw).unwrap();
    assert_eq!(parsed["tab_width"].as_integer(), Some(4));
    assert_eq!(parsed["ui"]["preview"].as_bool(), Some(false));
}
