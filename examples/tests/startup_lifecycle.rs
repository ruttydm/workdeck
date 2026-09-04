use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use workdeck_extension_api::Registration;
use workdeck_extension_host::{
    LoadStartupExtensionsOptions, TrustDecision, TrustStore, load_startup_extensions,
};

fn install_extension(root: &Path, id: &str) -> PathBuf {
    let directory = root.join(id);
    fs::create_dir_all(&directory).unwrap();
    let executable_name = format!("extension{}", std::env::consts::EXE_SUFFIX);
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-startup-lifecycle-extension"),
        directory.join(&executable_name),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = directory.join(&executable_name);
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(executable, permissions).unwrap();
    }
    let manifest = directory.join("workdeck-extension.toml");
    fs::write(
        &manifest,
        format!(
            "id = {id:?}\nname = {id:?}\nversion = \"1.0.0\"\napi_version = 1\nexecutable = {executable_name:?}\ncapabilities = [\"configuration\", \"themes\", \"events\"]\n"
        ),
    )
    .unwrap();
    manifest
}

fn options<'a>(
    cwd: &'a Path,
    global_directory: Option<&'a Path>,
    repo_root: Option<&'a Path>,
    trust: &'a TrustStore,
    configs: &'a BTreeMap<String, serde_json::Value>,
) -> LoadStartupExtensionsOptions<'a> {
    LoadStartupExtensionsOptions {
        enabled: true,
        cwd,
        global_directory,
        repo_root,
        trust,
        explicit_paths: &[],
        user_config_paths: &[],
        repo_config_paths: &[],
        host_version: "startup-test",
        extension_configs: configs,
        notifications: None,
        previous_load: None,
        defer_event_bus_binding: false,
    }
}

#[test]
fn discovers_global_extensions_and_delivers_their_configuration() {
    let root = TempDir::new().unwrap();
    let global = root.path().join("global");
    install_extension(&global, "themed");
    let trust = TrustStore::default();
    let log = root.path().join("context.log");
    let configs = BTreeMap::from([(
        "themed".into(),
        json!({
            "themeId": "midnight",
            "logPath": log,
            "logCwd": true,
            "logStderr": true
        }),
    )]);
    let mut result =
        load_startup_extensions(options(root.path(), Some(&global), None, &trust, &configs))
            .unwrap();
    assert!(result.issues.is_empty());
    assert_eq!(result.extensions.len(), 1);
    assert_eq!(
        result.extensions[0].metadata(),
        workdeck_extension_host::ExtensionMetadata {
            id: "themed".into(),
            source_path: fs::canonicalize(global.join("themed/workdeck-extension.toml")).unwrap(),
            origin: workdeck_extension_host::ManifestOrigin::Global,
        }
    );
    assert!(result.extensions[0].handshake.registrations.iter().any(
        |registration| matches!(registration, Registration::Theme(theme) if theme.id == "midnight")
    ));
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        format!("factory:themed:null\ncwd:{}\n", root.path().display())
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while result.logs.snapshot().is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(
        result.logs.snapshot(),
        [workdeck_extension_host::ExtensionLogEntry {
            extension_id: "themed".into(),
            message: "factory log 🧭".into(),
        }]
    );
    assert_eq!(result.extensions[0].logs(), result.logs.snapshot());
    result.retire();
}

#[test]
fn extends_a_provisional_prefix_without_starting_its_process_again() {
    let root = TempDir::new().unwrap();
    let global = root.path().join("global");
    let repo = root.path().join("repo");
    let repo_extensions = repo.join(".agents/workdeck/extensions");
    fs::create_dir_all(&repo_extensions).unwrap();
    install_extension(&global, "global");
    install_extension(&repo_extensions, "local");
    let log = root.path().join("factories.log");
    let configs = BTreeMap::from([
        ("global".into(), json!({"logPath": log, "value": "global"})),
        ("local".into(), json!({"logPath": log, "value": "local"})),
    ]);
    let mut trust = TrustStore::default();
    trust.grant(&repo, TrustDecision::Trusted);
    let provisional =
        load_startup_extensions(options(&repo, Some(&global), None, &trust, &configs)).unwrap();
    let mut final_options = options(&repo, Some(&global), Some(&repo), &trust, &configs);
    final_options.previous_load = Some(provisional);
    let mut result = load_startup_extensions(final_options).unwrap();
    assert_eq!(
        result
            .extensions
            .iter()
            .map(|extension| extension.manifest.id.as_str())
            .collect::<Vec<_>>(),
        ["global", "local"]
    );
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "factory:global:\"global\"\nfactory:local:\"local\"\n"
    );
    result.retire();
}

#[test]
fn changed_configuration_retires_before_rebuilding_the_extension() {
    let root = TempDir::new().unwrap();
    let global = root.path().join("global");
    install_extension(&global, "configured");
    let log = root.path().join("lifecycle.log");
    let trust = TrustStore::default();
    let first_config = BTreeMap::from([("configured".into(), json!({"logPath": log, "value": 1}))]);
    let first = load_startup_extensions(options(
        root.path(),
        Some(&global),
        None,
        &trust,
        &first_config,
    ))
    .unwrap();
    let second_config =
        BTreeMap::from([("configured".into(), json!({"logPath": log, "value": 2}))]);
    let mut second_options = options(root.path(), Some(&global), None, &trust, &second_config);
    second_options.previous_load = Some(first);
    let mut result = load_startup_extensions(second_options).unwrap();
    assert_eq!(
        fs::read_to_string(&log).unwrap(),
        "factory:configured:1\nshutdown\nfactory:configured:2\n"
    );
    result.retire();
}
