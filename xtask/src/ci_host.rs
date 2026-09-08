//! Native CI host assertion. This is configuration validation, not cross-platform execution proof.

use anyhow::{Context, Result, bail, ensure};
use std::process::Command;

const HOSTS: &[(&str, &str, &str, &str)] = &[
    (
        "ubuntu-24.04",
        "x86_64-unknown-linux-gnu",
        "x86_64",
        "linux",
    ),
    (
        "ubuntu-24.04-arm",
        "aarch64-unknown-linux-gnu",
        "aarch64",
        "linux",
    ),
    ("macos-15", "aarch64-apple-darwin", "aarch64", "macos"),
    ("macos-15-intel", "x86_64-apple-darwin", "x86_64", "macos"),
    (
        "windows-2025",
        "x86_64-pc-windows-msvc",
        "x86_64",
        "windows",
    ),
];

fn validate(expected: &str, rustc_verbose: &str, arch: &str, os: &str) -> Result<()> {
    let (_, _, expected_arch, expected_os) = HOSTS
        .iter()
        .find(|(_, target, _, _)| *target == expected)
        .context("unsupported native CI target")?;
    let hosts = rustc_verbose
        .lines()
        .filter_map(|line| line.strip_prefix("host: "))
        .collect::<Vec<_>>();
    ensure!(
        hosts == [expected],
        "compiler host does not match expected native target {expected}: {hosts:?}"
    );
    ensure!(
        arch == *expected_arch && os == *expected_os,
        "xtask was compiled for {arch}/{os}, expected {expected_arch}/{expected_os}"
    );
    Ok(())
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let expected = args
        .next()
        .context("ci-host requires one expected native target")?;
    if args.next().is_some() {
        bail!("ci-host accepts exactly one expected native target");
    }
    let output = Command::new("rustc")
        .args(["--version", "--verbose"])
        .output()?;
    ensure!(
        output.status.success(),
        "rustc host inspection failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let verbose = String::from_utf8(output.stdout)?;
    validate(
        &expected,
        &verbose,
        std::env::consts::ARCH,
        std::env::consts::OS,
    )?;
    println!("Native compiler and xtask target verified: {expected}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_host_rejects_cross_compiled_missing_duplicate_and_wrong_architecture() {
        for (_, target, arch, os) in HOSTS {
            let verbose = format!("rustc version\nhost: {target}\nrelease: test\n");
            assert!(validate(target, &verbose, arch, os).is_ok());
            assert!(validate(target, "release: test\n", arch, os).is_err());
            assert!(validate(target, &format!("{verbose}host: {target}\n"), arch, os).is_err());
            assert!(validate(target, &verbose, "wrong", os).is_err());
            assert!(validate(target, &verbose, arch, "wrong").is_err());
            assert!(validate(target, "host: wasm32-unknown-unknown\n", arch, os).is_err());
        }
        assert!(validate("unrecognized", "", "", "").is_err());
    }

    #[test]
    fn ci_matrix_covers_all_five_explicit_native_hosts() {
        assert_native_matrix(include_str!("../../.github/workflows/ci.yml"), "tui");
        assert_native_matrix(include_str!("../../.github/workflows/release.yml"), "build");
    }

    fn assert_native_matrix(source: &str, job: &str) {
        let workflow: serde_norway::Value = serde_norway::from_str(source).unwrap();
        let matrix = workflow["jobs"][job]["strategy"]["matrix"]["include"]
            .as_sequence()
            .unwrap();
        assert_eq!(matrix.len(), HOSTS.len());
        for (runner, target, _, _) in HOSTS {
            assert_eq!(
                matrix
                    .iter()
                    .filter(|entry| entry["os"].as_str() == Some(runner)
                        && entry["target"].as_str() == Some(target))
                    .count(),
                1
            );
        }
        let steps = workflow["jobs"][job]["steps"].as_sequence().unwrap();
        assert!(
            steps.iter().any(
                |step| step["run"].as_str() == Some("cargo xtask ci-host ${{ matrix.target }}")
            )
        );
    }
}
