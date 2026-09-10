//! PATH edit planning and explicit application, derived from Hunk's MIT installer.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum ShellPathPlan {
    Skipped,
    AlreadyPresent(PathBuf),
    Edit {
        path: PathBuf,
        original: Option<Vec<u8>>,
        replacement: Vec<u8>,
    },
}

/// Apply a plan to a quiescent destination, retaining a new recovery JSON file.
/// Parent-directory races and multi-file installation rollback require caller coordination.
pub fn apply(plan: &ShellPathPlan, recovery: &Path) -> Result<bool> {
    let ShellPathPlan::Edit {
        path,
        original,
        replacement,
    } = plan
    else {
        return Ok(false);
    };
    ensure!(
        replacement.starts_with(original.as_deref().unwrap_or_default()),
        "PATH edit must preserve original bytes"
    );
    ensure!(
        original.as_deref() != Some(replacement.as_slice()),
        "PATH edit is unchanged"
    );
    let current = read_existing(path)?;
    ensure!(
        current.as_ref().map(|(_, bytes)| bytes) == original.as_ref(),
        "shell profile changed since planning"
    );
    let recovery_parent = recovery
        .parent()
        .context("recovery file needs a parent")?
        .canonicalize()?;
    let recovery =
        recovery_parent.join(recovery.file_name().context("recovery filename required")?);
    if let Some(parent) = path.parent().and_then(|parent| parent.canonicalize().ok()) {
        ensure!(
            parent.join(path.file_name().context("profile filename required")?) != recovery,
            "recovery file must differ from profile"
        );
    }
    let mut record = tempfile::NamedTempFile::new_in(&recovery_parent)?;
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        current
            .as_ref()
            .map(|(metadata, _)| metadata.permissions().mode())
    };
    #[cfg(not(unix))]
    let mode: Option<u32> = None;
    serde_json::to_writer(
        record.as_file_mut(),
        &serde_json::json!({"schema":1,"path":std::path::absolute(path)?,"original":original,"unixMode":mode}),
    )?;
    record.as_file().sync_all()?;
    record
        .persist_noclobber(&recovery)
        .context("recovery file exists or cannot be created")?;
    let parent = path.parent().context("profile needs a parent")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(replacement)?;
    if let Some((metadata, _)) = &current {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.as_file().sync_all()?;
    let rechecked = read_existing(path)?;
    ensure!(
        rechecked.as_ref().map(|(_, bytes)| bytes) == original.as_ref()
            && rechecked
                .as_ref()
                .map(|(metadata, _)| metadata.permissions())
                == current.as_ref().map(|(metadata, _)| metadata.permissions()),
        "shell profile changed before replacement; recovery retained"
    );
    if original.is_none() {
        temporary
            .persist_noclobber(path)
            .context("profile appeared before creation; recovery retained")?;
    } else {
        temporary
            .persist(path)
            .context("profile replacement failed; recovery retained")?;
    }
    Ok(true)
}

fn read_existing(path: &Path) -> Result<Option<(std::fs::Metadata, Vec<u8>)>> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(Some(super::transaction::read_binary(path)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Plan without writing profiles, creating directories or changing this process's PATH.
pub fn plan(
    bin: &str,
    home: &Path,
    env: &BTreeMap<String, String>,
    no_modify: bool,
) -> Result<ShellPathPlan> {
    if no_modify {
        return Ok(ShellPathPlan::Skipped);
    }
    let configured = |name: &str| env.get(name).filter(|s| !s.is_empty());
    let quoted = format!("'{}'", bin.replace('\'', "'\\''"));
    let github = configured("GITHUB_PATH");
    let (path, line) = if let Some(path) = github {
        (PathBuf::from(path), bin.to_owned())
    } else {
        let shell = configured("SHELL").map_or("sh", |s| {
            s.trim_end_matches('/').rsplit('/').next().unwrap_or("sh")
        });
        let path = match shell {
            "zsh" => configured("ZDOTDIR")
                .map_or_else(|| home.to_path_buf(), PathBuf::from)
                .join(".zshrc"),
            "bash" => [".bashrc", ".bash_profile", ".profile"]
                .into_iter()
                .map(|p| home.join(p))
                .find(|p| p.is_file())
                .unwrap_or_else(|| home.join(".bashrc")),
            "fish" => home.join(".config/fish/config.fish"),
            _ => home.join(".profile"),
        };
        let line = if shell == "fish" {
            format!("fish_add_path {quoted}")
        } else {
            format!("export PATH={quoted}:\"$PATH\"")
        };
        (path, line)
    };
    let original = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if github.is_none()
        && original.as_ref().is_some_and(|bytes| {
            bytes
                .windows(line.len())
                .any(|part| part == line.as_bytes())
        })
    {
        return Ok(ShellPathPlan::AlreadyPresent(path));
    }
    let mut replacement = original.clone().unwrap_or_default();
    if github.is_none() {
        replacement
            .extend_from_slice(b"\n# Added by the Workdeck installer (https://workdeck.dev)\n");
    }
    replacement.extend_from_slice(line.as_bytes());
    replacement.push(b'\n');
    Ok(ShellPathPlan::Edit {
        path,
        original,
        replacement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn assert_profile_executes(shell: &str) {
        let home = tempfile::tempdir().unwrap();
        // Shell metacharacters must remain literal data, including substitution.
        let bin = home
            .path()
            .join("it's a $PATH `literal` $(exit 91) directory");
        std::fs::create_dir(&bin).unwrap();
        let bin = bin.to_str().unwrap();
        let env = BTreeMap::from([("SHELL".into(), shell.into())]);
        let planned = plan(bin, home.path(), &env, false).unwrap();
        let ShellPathPlan::Edit { path, .. } = &planned else {
            panic!("new profile must require an edit");
        };
        apply(&planned, &home.path().join("recovery.json")).unwrap();
        let output = std::process::Command::new(shell)
            .args([
                "-f",
                "-c",
                ". \"$1\"; printf '%s' \"$PATH\"",
                "profile-test",
            ])
            .arg(path)
            .env_clear()
            .env("HOME", home.path())
            .env("ZDOTDIR", home.path())
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell}: {output:?}");
        assert!(output.stderr.is_empty(), "{shell}: {output:?}");
        assert_eq!(output.stdout, format!("{bin}:/usr/bin:/bin").as_bytes());
        assert!(matches!(
            plan(bin, home.path(), &env, false).unwrap(),
            ShellPathPlan::AlreadyPresent(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn applied_profile_executes_in_posix_shell_without_expanding_directory_data() {
        assert_profile_executes("/bin/sh");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applied_profiles_execute_in_macos_bash_and_zsh() {
        assert_profile_executes("/bin/bash");
        assert_profile_executes("/bin/zsh");
    }

    #[test]
    fn recovery_cannot_create_the_missing_profile() {
        let home = tempfile::tempdir().unwrap();
        let planned = plan("/app/bin", home.path(), &BTreeMap::new(), false).unwrap();
        let profile = home.path().join(".profile");
        assert!(
            apply(&planned, &profile)
                .unwrap_err()
                .to_string()
                .contains("must differ")
        );
        assert!(!profile.exists());
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
    }

    #[test]
    fn profile_apply_preserves_original_recovery_and_rejects_stale_plans() {
        let home = tempfile::tempdir().unwrap();
        let profile = home.path().join(".profile");
        std::fs::write(&profile, [0xff, b'\n']).unwrap();
        let planned = plan("/app/bin", home.path(), &BTreeMap::new(), false).unwrap();
        let recovery = home.path().join("profile-backup.json");
        assert!(apply(&planned, &recovery).unwrap());
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&recovery).unwrap()).unwrap();
        assert_eq!(record["original"], serde_json::json!([255, 10]));
        let written = std::fs::read(&profile).unwrap();
        assert!(written.starts_with(&[0xff, b'\n']));
        assert!(apply(&planned, &home.path().join("second-backup")).is_err());
        assert!(!home.path().join("second-backup").exists());
        assert_eq!(std::fs::read(&profile).unwrap(), written);
        let again = plan("/app/bin", home.path(), &BTreeMap::new(), false).unwrap();
        assert!(!apply(&again, &recovery).unwrap());
    }

    #[test]
    fn missing_profile_creation_records_absence_and_refuses_existing_recovery() {
        let home = tempfile::tempdir().unwrap();
        let env = BTreeMap::from([("SHELL".into(), "/bin/fish".into())]);
        let planned = plan("/app/bin", home.path(), &env, false).unwrap();
        let recovery = home.path().join("backup.json");
        std::fs::write(&recovery, b"existing").unwrap();
        assert!(apply(&planned, &recovery).is_err());
        assert!(!home.path().join(".config").exists());
        let recovery = home.path().join("new-backup.json");
        assert!(apply(&planned, &recovery).unwrap());
        let record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&recovery).unwrap()).unwrap();
        assert!(record["original"].is_null());
        assert!(home.path().join(".config/fish/config.fish").is_file());
    }

    #[test]
    fn shell_selection_quoting_and_existing_bytes_are_preserved_without_writes() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(".bash_profile"), [0xff, b'\n']).unwrap();
        for (shell, suffix, expected) in [
            (
                "/bin/bash",
                ".bash_profile",
                "export PATH='/tmp/it'\\''s bin':\"$PATH\"",
            ),
            (
                "/bin/zsh",
                ".zshrc",
                "export PATH='/tmp/it'\\''s bin':\"$PATH\"",
            ),
            (
                "/usr/bin/fish",
                ".config/fish/config.fish",
                "fish_add_path '/tmp/it'\\''s bin'",
            ),
            (
                "/bin/sh",
                ".profile",
                "export PATH='/tmp/it'\\''s bin':\"$PATH\"",
            ),
        ] {
            let env = BTreeMap::from([("SHELL".into(), shell.into())]);
            let ShellPathPlan::Edit {
                path,
                original,
                replacement,
            } = plan("/tmp/it's bin", home.path(), &env, false).unwrap()
            else {
                panic!("expected edit")
            };
            assert_eq!(path, home.path().join(suffix));
            assert!(replacement.ends_with(format!("{expected}\n").as_bytes()));
            if shell == "/bin/bash" {
                assert_eq!(original, Some(vec![0xff, b'\n']));
                assert!(replacement.starts_with(&[0xff, b'\n']));
            } else {
                assert!(original.is_none());
                assert!(!path.exists());
            }
        }
        assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 1);
    }

    #[test]
    fn zdotdir_and_bash_precedence_follow_source_without_creating_directories() {
        let home = tempfile::tempdir().unwrap();
        let redirected = home.path().join("other-zsh");
        let env = BTreeMap::from([
            ("SHELL".into(), "/bin/zsh".into()),
            ("ZDOTDIR".into(), redirected.to_string_lossy().into_owned()),
        ]);
        let ShellPathPlan::Edit { path, .. } = plan("/app/bin", home.path(), &env, false).unwrap()
        else {
            panic!("expected edit")
        };
        assert_eq!(path, redirected.join(".zshrc"));
        assert!(!redirected.exists());
        let env = BTreeMap::from([("SHELL".into(), "/bin/bash///".into())]);
        for (created, selected) in [
            (None, ".bashrc"),
            (Some(".profile"), ".profile"),
            (Some(".bash_profile"), ".bash_profile"),
            (Some(".bashrc"), ".bashrc"),
        ] {
            if let Some(created) = created {
                std::fs::write(home.path().join(created), b"original").unwrap();
            }
            let ShellPathPlan::Edit { path, .. } =
                plan("/app/bin", home.path(), &env, false).unwrap()
            else {
                panic!("expected edit")
            };
            assert_eq!(path, home.path().join(selected));
        }
    }

    #[test]
    fn github_path_keeps_duplicate_appends_and_empty_environment_falls_back() {
        let home = tempfile::tempdir().unwrap();
        let github = home.path().join("github-path");
        std::fs::write(&github, b"/app/bin\n").unwrap();
        let mut env =
            BTreeMap::from([("GITHUB_PATH".into(), github.to_string_lossy().into_owned())]);
        let ShellPathPlan::Edit {
            replacement,
            original,
            ..
        } = plan("/app/bin", home.path(), &env, false).unwrap()
        else {
            panic!("expected append")
        };
        assert_eq!(original, Some(b"/app/bin\n".to_vec()));
        assert_eq!(replacement, b"/app/bin\n/app/bin\n");
        assert_eq!(std::fs::read(&github).unwrap(), b"/app/bin\n");
        env.insert("GITHUB_PATH".into(), String::new());
        env.insert("SHELL".into(), "/bin/zsh".into());
        env.insert("ZDOTDIR".into(), String::new());
        let ShellPathPlan::Edit { path, .. } = plan("/app/bin", home.path(), &env, false).unwrap()
        else {
            panic!("expected profile")
        };
        assert_eq!(path, home.path().join(".zshrc"));
        assert!(!path.exists());
    }

    #[test]
    fn no_modify_precedes_github_and_profile_idempotence_matches_substring_check() {
        let home = tempfile::tempdir().unwrap();
        let github = home.path().join("github-path");
        let env = BTreeMap::from([("GITHUB_PATH".into(), github.to_string_lossy().into_owned())]);
        assert_eq!(
            plan("/bin", home.path(), &env, true).unwrap(),
            ShellPathPlan::Skipped
        );
        let ShellPathPlan::Edit { replacement, .. } =
            plan("/bin", home.path(), &env, false).unwrap()
        else {
            panic!("expected edit")
        };
        assert_eq!(replacement, b"/bin\n");
        assert!(!github.exists());
        std::fs::write(
            home.path().join(".profile"),
            b"# export PATH='/bin':\"$PATH\" suffix\n",
        )
        .unwrap();
        assert_eq!(
            plan("/bin", home.path(), &BTreeMap::new(), false).unwrap(),
            ShellPathPlan::AlreadyPresent(home.path().join(".profile"))
        );
    }
}
