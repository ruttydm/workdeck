//! Disposable execution of pinned shell helpers for oracle capture only.
use anyhow::{Context, Result, ensure};
use std::path::Path;
use std::process::Command;

fn helper<'a>(source: &'a str, name: &str) -> Result<&'a str> {
    let start = source
        .find(&format!("{name}() {{"))
        .context("missing installer oracle helper")?;
    let end = source[start..]
        .find("\n}")
        .context("unterminated installer oracle helper")?;
    Ok(&source[start..start + end + 2])
}

pub(super) fn run(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(args.next().is_none(), "install-oracle accepts no arguments");
    println!("{}", serde_json::to_string_pretty(&capture(repo)?)?);
    Ok(())
}

fn capture(repo: &Path) -> Result<Vec<serde_json::Value>> {
    let mut cases = Vec::new();
    for (pin, commit) in [
        (
            "hunk-port/main-2c00f435",
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
        ),
        (
            "hunk-port/stable-v0.20.1",
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
        ),
    ] {
        require_pin(repo, pin, commit)?;
        let source = Command::new("git")
            .current_dir(repo)
            .args(["show", &format!("{commit}:install.sh")])
            .output()?;
        ensure!(
            source.status.success(),
            "cannot read pinned installer source"
        );
        let source = String::from_utf8(source.stdout)?;
        let helpers = [
            helper(&source, "fail")?,
            helper(&source, "detect_os")?,
            helper(&source, "detect_arch")?,
        ]
        .join("\n");
        for (os, arch, translated) in [
            ("Darwin", "x86_64", false),
            ("Darwin", "x86_64", true),
            ("Darwin", "amd64", true),
            ("Darwin", "aarch64", false),
            ("Linux", "x86_64", false),
            ("Linux", "amd64", false),
            ("Linux", "arm64", false),
            ("Linux", "aarch64", false),
            ("Linux", "riscv64", false),
            ("FreeBSD", "x86_64", false),
        ] {
            let script = format!(
                "{helpers}\nuname() {{ if [ \"$1\" = \"-s\" ]; then printf '%s\\n' '{os}'; else printf '%s\\n' '{arch}'; fi; }}\nsysctl() {{ printf '%s\\n' '{}'; }}\ndetect_os && detect_arch",
                u8::from(translated)
            );
            let output = Command::new("sh")
                .current_dir(repo)
                .args(["-c", &script])
                .output()?;
            let stdout = String::from_utf8(output.stdout)?;
            let stderr = String::from_utf8(output.stderr)?;
            cases.push(
                serde_json::json!({"pin":pin, "os":os, "arch":arch, "translated":translated,
                "exit_code":output.status.code().context("oracle terminated by signal")?,
                "output":format!("{stdout}{stderr}"), "stdout":stdout, "stderr":stderr}),
            );
        }
    }
    Ok(cases)
}

fn require_pin(repo: &Path, pin: &str, expected: &str) -> Result<()> {
    let resolved = Command::new("git")
        .current_dir(repo)
        .args(["rev-parse", "--verify", &format!("{pin}^{{commit}}")])
        .output()?;
    ensure!(
        resolved.status.success() && String::from_utf8(resolved.stdout)?.trim() == expected,
        "installer oracle pin is missing or changed: {pin}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "executes both pinned shell installers' PATH helpers in temporary directories"]
    fn native_path_bytes_match_both_pinned_installers() {
        use std::collections::BTreeMap;
        use workdeck_cli::install::shell_path::{ShellPathPlan, plan};

        let repo = crate::repo_root().unwrap();
        for (pin, commit) in [
            (
                "hunk-port/main-2c00f435",
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            ),
            (
                "hunk-port/stable-v0.20.1",
                "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
            ),
        ] {
            require_pin(&repo, pin, commit).unwrap();
            let source = Command::new("git")
                .current_dir(&repo)
                .args(["show", &format!("{commit}:install.sh")])
                .output()
                .unwrap();
            assert!(source.status.success());
            let source = String::from_utf8(source.stdout).unwrap();
            let helpers = ["info", "squote", "add_path_line"]
                .map(|name| helper(&source, name).unwrap())
                .join("\n");
            for shell in ["sh", "zsh", "fish"] {
                for bin in [
                    "/app/bin",
                    "/app/it's bin",
                    "/app/$PATH `literal`",
                    "/app/$(exit 91)",
                    "/app/line\nbreak",
                ] {
                    for original in [None, Some(b"# original\n".to_vec()), Some(vec![255, 10])] {
                        let home = tempfile::tempdir().unwrap();
                        let env = BTreeMap::from([("SHELL".into(), shell.into())]);
                        let ShellPathPlan::Edit { path, .. } =
                            plan(bin, home.path(), &env, false).unwrap()
                        else {
                            panic!("missing profile")
                        };
                        if let Some(bytes) = &original {
                            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                            std::fs::write(&path, bytes).unwrap();
                        }
                        let planned = plan(bin, home.path(), &env, false).unwrap();
                        let ShellPathPlan::Edit { replacement, .. } = planned else {
                            panic!("new line")
                        };
                        let script = format!(
                            "set -eu\n{helpers}\nbin_dir=$1\nquoted=\"'$(squote \"$bin_dir\")'\"\nif [ \"$3\" = fish ]; then line=\"fish_add_path $quoted\"; else line=\"export PATH=$quoted:\\\"\\$PATH\\\"\"; fi\nadd_path_line \"$2\" \"$line\""
                        );
                        let output = Command::new("/bin/sh")
                            .env_clear()
                            .env("PATH", "/usr/bin:/bin")
                            .args(["-c", &script, "oracle", bin])
                            .arg(&path)
                            .arg(shell)
                            .output()
                            .unwrap();
                        assert!(output.status.success(), "{pin} {shell}: {output:?}");
                        assert!(output.stderr.is_empty(), "{pin} {shell}: {output:?}");
                        let bytes = std::fs::read(&path).unwrap();
                        // Normalize only the literal branding comment, preserving arbitrary bytes.
                        let marker = b"# Added by the Hunk installer (https://hunk.dev)";
                        let offset = bytes
                            .windows(marker.len())
                            .position(|part| part == marker)
                            .unwrap();
                        let mut normalized = bytes[..offset].to_vec();
                        normalized.extend_from_slice(
                            b"# Added by the Workdeck installer (https://workdeck.dev)",
                        );
                        normalized.extend_from_slice(&bytes[offset + marker.len()..]);
                        assert_eq!(normalized, replacement, "{pin} {shell} {bin:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn changed_pin_is_rejected_before_reading_source() {
        let directory = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "--quiet"],
            vec![
                "-c",
                "user.name=Oracle",
                "-c",
                "user.email=oracle@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "fixture",
            ],
            vec!["tag", "hunk-port/main-2c00f435"],
        ] {
            assert!(
                Command::new("git")
                    .current_dir(directory.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert!(
            require_pin(
                directory.path(),
                "hunk-port/main-2c00f435",
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
            )
            .unwrap_err()
            .to_string()
            .contains("missing or changed")
        );
    }

    #[test]
    #[ignore = "executes pinned shell oracle; run explicitly during capture verification"]
    fn frozen_installer_platform_capture_is_reproducible() {
        let repo = crate::repo_root().unwrap();
        let expected: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../port/hunk/install-platform-oracle.json"))
                .unwrap();
        assert_eq!(capture(&repo).unwrap(), expected);
    }
}
