//! Incremental MIT translation of Hunk install.sh. Preflight only; no installation writes.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    version: String,
    no_modify_path: bool,
    allow_conflicts: bool,
}

fn options(
    args: impl Iterator<Item = String>,
    env: &BTreeMap<String, String>,
) -> Result<Option<Options>> {
    let mut options = Options {
        version: env.get("WORKDECK_VERSION").cloned().unwrap_or_default(),
        no_modify_path: env
            .get("WORKDECK_NO_MODIFY_PATH")
            .is_some_and(|value| value == "1"),
        allow_conflicts: env
            .get("WORKDECK_ALLOW_CONFLICTING_INSTALLS")
            .is_some_and(|value| value == "1"),
    };
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--no-modify-path" => options.no_modify_path = true,
            "-f" | "--force" => options.allow_conflicts = true,
            arg if arg.starts_with('-') => {
                bail!("Unknown option: {arg} (run with --help to see the supported options)")
            }
            _ => options.version = arg,
        }
    }
    options.version = options
        .version
        .strip_prefix('v')
        .unwrap_or(&options.version)
        .to_owned();
    Ok(Some(options))
}

fn platform(os: &str, arch: &str, translated: bool) -> Result<(&'static str, &'static str)> {
    let os = match os {
        "Darwin" | "macos" => "darwin",
        "Linux" | "linux" => "linux",
        "Windows" | "windows" => "windows",
        _ => bail!("Unsupported operating system: {os}. No native Workdeck archive is available."),
    };
    let arch = match arch {
        "x86_64" | "amd64" if os == "darwin" && translated => "arm64",
        "x86_64" | "amd64" => "x64",
        "arm64" | "aarch64" if os != "windows" => "arm64",
        _ => bail!("Unsupported architecture: {arch}. No native Workdeck archive is available."),
    };
    Ok((os, arch))
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let env = [
        "WORKDECK_VERSION",
        "WORKDECK_NO_MODIFY_PATH",
        "WORKDECK_ALLOW_CONFLICTING_INSTALLS",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)))
    .collect();
    let Some(options) = options(args, &env)? else {
        println!(
            "Read-only Workdeck installer preflight\nUsage: cargo xtask install-plan [version] [--no-modify-path] [-f|--force]\nNo downloads, shell-profile edits, or installation writes are performed."
        );
        return Ok(());
    };
    let translated = cfg!(target_os = "macos")
        && std::process::Command::new("sysctl")
            .args(["-n", "sysctl.proc_translated"])
            .output()
            .ok()
            .is_some_and(|output| {
                output.status.success()
                    && output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout) == b"1"
            });
    let (os, arch) = platform(std::env::consts::OS, std::env::consts::ARCH, translated)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "options": options, "os": os, "arch": arch, "executionAvailable": false,
            "remaining": ["release resolution", "competing installs", "verified archive extraction", "atomic installation", "shell profile updates"]
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_options_preserve_source_order_environment_and_single_prefix_removal() {
        let env = BTreeMap::from([
            ("WORKDECK_VERSION".into(), "v1.0".into()),
            ("WORKDECK_NO_MODIFY_PATH".into(), "true".into()),
        ]);
        let parsed = options(
            ["v2.0", "--force", "vv3.0", "--no-modify-path"]
                .map(str::to_owned)
                .into_iter(),
            &env,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            parsed,
            Options {
                version: "v3.0".into(),
                no_modify_path: true,
                allow_conflicts: true
            }
        );
        let defaults = options(std::iter::empty(), &env).unwrap().unwrap();
        assert_eq!(defaults.version, "1.0");
        assert!(!defaults.no_modify_path && !defaults.allow_conflicts);
        assert!(
            options(["--help", "--bad"].map(str::to_owned).into_iter(), &env)
                .unwrap()
                .is_none()
        );
        assert!(options(["--bad", "--help"].map(str::to_owned).into_iter(), &env).is_err());
        assert!(options(["--".into()].into_iter(), &env).is_err());
    }

    #[test]
    fn archive_platform_corrects_rosetta_without_changing_linux_or_windows() {
        for arch in ["amd64", "x86_64"] {
            assert_eq!(platform("Darwin", arch, true).unwrap(), ("darwin", "arm64"));
            assert_eq!(platform("Darwin", arch, false).unwrap(), ("darwin", "x64"));
            assert_eq!(platform("Linux", arch, true).unwrap(), ("linux", "x64"));
            assert_eq!(platform("Windows", arch, true).unwrap(), ("windows", "x64"));
        }
        for arch in ["arm64", "aarch64"] {
            assert_eq!(platform("Linux", arch, false).unwrap(), ("linux", "arm64"));
            assert!(platform("Windows", arch, false).is_err());
        }
        assert!(platform("FreeBSD", "x86_64", false).is_err());
        assert!(platform("Linux", "i686", false).is_err());
    }
}
