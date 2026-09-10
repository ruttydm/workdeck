//! Read-only planning of PATH edits from Hunk's MIT-licensed installer behavior.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.
use anyhow::Result;
use std::collections::BTreeMap;
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
        let shell = configured("SHELL").map_or("sh", |s| s.rsplit('/').next().unwrap_or("sh"));
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
