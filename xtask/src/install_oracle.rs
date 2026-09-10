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
