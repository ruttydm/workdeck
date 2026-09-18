//! Read, merge, quarantine, and atomically replace the persisted app-state file.

use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub type AppStateRecord = Map<String, Value>;

const CORRUPT_STATE_SUFFIX: &str = ".corrupt";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
struct AppStateFileRead {
    record: AppStateRecord,
    corrupt: bool,
}

fn read_app_state_file(path: &Path) -> AppStateFileRead {
    let Ok(contents) = fs::read_to_string(path) else {
        return AppStateFileRead {
            record: AppStateRecord::new(),
            corrupt: false,
        };
    };
    if contents.trim().is_empty() {
        return AppStateFileRead {
            record: AppStateRecord::new(),
            corrupt: false,
        };
    }
    match serde_json::from_str::<Value>(&contents) {
        Ok(Value::Object(record)) => AppStateFileRead {
            record,
            corrupt: false,
        },
        Ok(_) | Err(_) => AppStateFileRead {
            record: AppStateRecord::new(),
            corrupt: true,
        },
    }
}

/// Read the persisted state record, treating missing, unreadable, or malformed files as empty.
#[must_use]
pub fn read_app_state_record(path: impl AsRef<Path>) -> AppStateRecord {
    read_app_state_file(path.as_ref()).record
}

fn temp_path(path: &Path) -> PathBuf {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map_or_else(|| "state.json".into(), |name| name.to_os_string());
    name.push(format!(
        ".{}.{}.{sequence}.tmp",
        std::process::id(),
        milliseconds
    ));
    path.with_file_name(name)
}

fn owner_only_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn write_owner_only(path: &Path, serialized: &[u8]) -> io::Result<()> {
    let mut file = owner_only_file(path)?;
    file.write_all(serialized)?;
    file.sync_all()
}

/// Write one state record through an owner-only sibling file and atomic rename.
pub fn write_app_state_record(path: impl AsRef<Path>, record: &AppStateRecord) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_vec_pretty(record)?;
    let temporary = temp_path(path);
    write_owner_only(&temporary, &serialized)?;

    if fs::rename(&temporary, path).is_ok() {
        return Ok(());
    }

    let _ = fs::remove_file(path);
    if fs::rename(&temporary, path).is_ok() {
        return Ok(());
    }

    let result = write_owner_only(path, &serialized);
    let _ = fs::remove_file(&temporary);
    result
}

fn quarantine_corrupt_state_file(path: &Path) {
    let mut quarantined = path.as_os_str().to_os_string();
    quarantined.push(CORRUPT_STATE_SUFFIX);
    let _ = fs::rename(path, PathBuf::from(quarantined));
}

/// Merge a top-level patch into persisted state without disturbing unrelated keys.
pub fn update_app_state_record(
    path: impl AsRef<Path>,
    patch: AppStateRecord,
) -> io::Result<AppStateRecord> {
    let path = path.as_ref();
    let AppStateFileRead {
        mut record,
        corrupt,
    } = read_app_state_file(path);
    if corrupt {
        quarantine_corrupt_state_file(path);
    }
    record.extend(patch);
    write_app_state_record(path, &record)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn record(value: Value) -> AppStateRecord {
        value.as_object().expect("test record").clone()
    }

    #[test]
    fn round_trips_through_a_directory_that_does_not_exist_yet() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("nested/state.json");
        let value = record(json!({"extensionTrust": {"/repo": "trusted"}}));

        write_app_state_record(&path, &value).unwrap();

        assert_eq!(read_app_state_record(&path), value);
    }

    #[test]
    fn missing_or_empty_files_are_empty_records() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("state.json");
        assert!(read_app_state_record(&path).is_empty());
        fs::write(&path, "   \n").unwrap();
        assert!(read_app_state_record(&path).is_empty());
    }

    #[test]
    fn patch_merge_preserves_unrelated_keys_and_replaces_named_values() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("state.json");
        write_app_state_record(
            &path,
            &record(json!({
                "updateNotice": {"lastCheck": 1},
                "extensionTrust": {"/a": "denied"}
            })),
        )
        .unwrap();

        let next =
            update_app_state_record(&path, record(json!({"extensionTrust": {"/b": "trusted"}})))
                .unwrap();
        assert_eq!(
            next,
            record(json!({
                "updateNotice": {"lastCheck": 1},
                "extensionTrust": {"/b": "trusted"}
            }))
        );
        assert_eq!(read_app_state_record(&path), next);
    }

    #[test]
    fn corrupt_bytes_are_quarantined_before_new_state_is_written() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("state.json");
        let damaged = r#"{"extensionTrust": {"/repo": "trus"#;
        fs::write(&path, damaged).unwrap();

        update_app_state_record(
            &path,
            record(json!({"extensionTrust": {"/repo": "denied"}})),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(directory.path().join("state.json.corrupt")).unwrap(),
            damaged
        );
        assert_eq!(
            read_app_state_record(&path),
            record(json!({"extensionTrust": {"/repo": "denied"}}))
        );
    }

    #[test]
    fn non_object_json_is_corrupt_and_quarantined_on_update() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("state.json");
        fs::write(&path, "[1, 2, 3]").unwrap();
        assert!(read_app_state_record(&path).is_empty());

        update_app_state_record(&path, record(json!({"updateNotice": {"lastCheck": 2}}))).unwrap();

        assert!(directory.path().join("state.json.corrupt").exists());
        assert_eq!(
            read_app_state_record(&path),
            record(json!({"updateNotice": {"lastCheck": 2}}))
        );
    }

    #[test]
    fn sibling_temporary_files_are_removed_after_replacement() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("state.json");
        write_app_state_record(&path, &record(json!({"lastCheck": 1}))).unwrap();
        write_app_state_record(&path, &record(json!({"lastCheck": 2}))).unwrap();

        let mut entries = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(entries, ["state.json"]);
    }

    #[cfg(unix)]
    #[test]
    fn state_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = TempDir::new().unwrap();
        let path = directory.path().join("state.json");
        write_app_state_record(&path, &record(json!({"extensionTrust": {}}))).unwrap();

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
