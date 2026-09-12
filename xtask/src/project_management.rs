//! Explicit project-management gates used by repository CI and release checks.
//!
//! The profile is checked before any command runs. Its validator source pin and
//! trusted-baseline reference make drift visible when the validator changes;
//! local execution remains qualification evidence, not an external trust claim.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const DEFAULT_PROFILE: &str = "standalone";
const PROFILE_PATH: &str = "ci/workdeck-pm-release.json";
const VALIDATOR_SOURCE: &str = "xtask/src/project_management.rs";

const STANDALONE_CHECKS: &[&[&str]] = &[
    &[
        "cargo",
        "test",
        "--locked",
        "-p",
        "workdeck-pm",
        "--all-targets",
        "--no-fail-fast",
    ],
    &[
        "cargo",
        "test",
        "--locked",
        "-p",
        "workdeck-cli",
        "--test",
        "pm_catalog",
        "--test",
        "pm_green_gate",
        "--test",
        "pm_imported_check",
        "--test",
        "pm_dogfood",
        "--test",
        "pm_fault_matrix",
        "--no-fail-fast",
    ],
    &[
        "cargo",
        "run",
        "--locked",
        "--release",
        "-p",
        "workdeck-pm",
        "--example",
        "projection_bench",
        "--",
        "features",
        "40000",
        "--enforce-targets",
    ],
    &[
        "cargo",
        "run",
        "--locked",
        "--release",
        "-p",
        "workdeck-pm",
        "--example",
        "projection_bench",
        "--",
        "issues",
        "10000",
        "--enforce-targets",
    ],
];

const STANDALONE_RELEASE_BUILD: &[&str] = &[
    "cargo",
    "build",
    "--locked",
    "--release",
    "--package",
    "workdeck-cli",
    "--bin",
    "workdeck",
];

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReleaseProfile {
    schema: u32,
    id: String,
    purpose: String,
    validator: ValidatorPin,
    checks: Vec<Vec<String>>,
    release_build: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ValidatorPin {
    source: String,
    sha256: String,
    trusted_baseline: String,
}

pub(super) fn run(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    match args.next().as_deref() {
        Some("check") => check(repo, parse_profile(args)?),
        Some("performance") => performance(repo, parse_profile(args)?),
        Some("release-check") => {
            let profile = parse_profile(args)?;
            let profile_document = load_profile(repo, &profile)?;
            run_profile_checks(repo, &profile_document)?;
            run_command(repo, &profile_document.release_build)?;
            println!("Workdeck PM release-readiness checks passed.");
            Ok(())
        }
        Some("profile") => {
            let profile = parse_profile(args)?;
            let profile_document = load_profile(repo, &profile)?;
            println!("{}", serde_json::to_string_pretty(&profile_document)?);
            Ok(())
        }
        Some(other) => {
            bail!(
                "pm requires the check, performance, release-check, or profile command, got {other:?}"
            )
        }
        None => bail!("pm requires the check, performance, release-check, or profile command"),
    }
}

fn parse_profile(mut args: impl Iterator<Item = String>) -> Result<String> {
    let mut profile = DEFAULT_PROFILE.to_owned();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--profile" => {
                profile = args.next().context("pm --profile requires a profile ID")?;
                if profile.trim().is_empty() || profile.len() > 64 {
                    bail!("pm profile ID must be nonempty and at most 64 bytes");
                }
            }
            "-h" | "--help" => {
                println!(
                    "Usage: cargo xtask pm <check|performance|release-check|profile> [--profile ID]"
                );
                std::process::exit(0);
            }
            _ => bail!("unknown pm option {argument:?}"),
        }
    }
    Ok(profile)
}

fn load_profile(repo: &Path, id: &str) -> Result<ReleaseProfile> {
    let path = repo.join(PROFILE_PATH);
    let bytes =
        fs::read(&path).with_context(|| format!("read PM release profile {}", path.display()))?;
    let profile: ReleaseProfile = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse PM release profile {}", path.display()))?;
    if profile.schema != 1 || profile.id != id {
        bail!("PM release profile {id:?} is missing or has an unsupported schema");
    }
    if profile.purpose.trim().is_empty() {
        bail!("PM release profile purpose is empty");
    }
    if profile.validator.source != VALIDATOR_SOURCE {
        bail!(
            "PM release profile validator source must be {VALIDATOR_SOURCE}, got {:?}",
            profile.validator.source
        );
    }
    let baseline_digest = validate_trusted_baseline(&profile.validator.trusted_baseline)?;
    let actual = sha256_file(&repo.join(&profile.validator.source))?;
    if actual != profile.validator.sha256 {
        bail!(
            "PM validator source pin mismatch: profile {}, current {}",
            profile.validator.sha256,
            actual
        );
    }
    if baseline_digest != profile.validator.sha256 {
        bail!(
            "PM validator pin and trusted baseline digest disagree: pin {}, baseline {}",
            profile.validator.sha256,
            baseline_digest
        );
    }
    if profile.checks.is_empty() || profile.release_build.is_empty() {
        bail!("PM release profile must contain checks and a release build");
    }
    validate_standalone_commands(&profile.checks, &profile.release_build)?;
    for command in &profile.checks {
        validate_profile_command(command)?;
    }
    validate_command(&profile.release_build)?;
    Ok(profile)
}

fn validate_profile_command(command: &[String]) -> Result<()> {
    match command.get(1).map(String::as_str) {
        Some("run") => validate_performance_command(command),
        Some("test" | "build") => validate_command(command),
        _ => bail!(
            "PM release profile only permits bounded cargo test/build commands or the exact projection benchmark"
        ),
    }
}

fn validate_standalone_commands(checks: &[Vec<String>], release_build: &[String]) -> Result<()> {
    if checks.len() != STANDALONE_CHECKS.len() {
        bail!(
            "standalone PM release profile must contain exactly {} required checks",
            STANDALONE_CHECKS.len()
        );
    }
    for (index, (actual, expected)) in checks.iter().zip(STANDALONE_CHECKS).enumerate() {
        if !command_matches(actual, expected) {
            bail!(
                "standalone PM release profile check {index} does not match the required command"
            );
        }
    }
    if !command_matches(release_build, STANDALONE_RELEASE_BUILD) {
        bail!("standalone PM release profile release build does not match the required command");
    }
    Ok(())
}

fn command_matches(actual: &[String], expected: &[&str]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

fn validate_command(command: &[String]) -> Result<()> {
    let Some(program) = command.first() else {
        bail!("PM release profile contains an empty command");
    };
    if program != "cargo" || command.iter().any(|arg| arg.contains('\0')) {
        bail!("PM release profile commands must be bounded cargo invocations");
    }
    let subcommand = command.get(1).map(String::as_str).unwrap_or_default();
    if !matches!(subcommand, "test" | "build") {
        bail!("PM release profile only permits cargo test/build commands");
    }
    Ok(())
}

fn validate_performance_command(command: &[String]) -> Result<()> {
    if command.first().map(String::as_str) != Some("cargo")
        || command.get(1).map(String::as_str) != Some("run")
        || command.len() != 12
        || command.get(2).map(String::as_str) != Some("--locked")
        || command.get(3).map(String::as_str) != Some("--release")
        || command.get(4).map(String::as_str) != Some("-p")
        || command.get(5).map(String::as_str) != Some("workdeck-pm")
        || command.get(6).map(String::as_str) != Some("--example")
        || command.get(7).map(String::as_str) != Some("projection_bench")
        || command.get(8).map(String::as_str) != Some("--")
        || command.get(11).map(String::as_str) != Some("--enforce-targets")
        || command.iter().any(|argument| argument.contains('\0'))
    {
        bail!("PM performance profile commands must be exact locked release projection benchmarks");
    }
    match (
        command.get(9).map(String::as_str),
        command.get(10).map(String::as_str),
    ) {
        (Some("features"), Some("40000")) | (Some("issues"), Some("10000")) => Ok(()),
        _ => bail!("PM performance profile must benchmark 40000 features or 10000 issues"),
    }
}

/// Validate the portable trusted-baseline reference stored in the profile.
///
/// The external validator still has to resolve the reference and compare the
/// digest. Keeping the tuple in a strict, machine-readable form prevents an
/// opaque label from being mistaken for that verification input while leaving
/// the profile schema and command dispatch unchanged.
fn validate_trusted_baseline(value: &str) -> Result<&str> {
    let Some((reference, digest)) = value.split_once("#sha256:") else {
        bail!(
            "PM release profile trusted baseline must use <full commit or refs/tags/...>#sha256:<64 lowercase hex>"
        );
    };
    if reference.is_empty()
        || reference.len() > 256
        || reference.trim() != reference
        || !valid_trusted_baseline_ref(reference)
    {
        bail!(
            "PM release profile trusted baseline reference is not an immutable Git commit or tag"
        );
    }
    if !lower_hex(digest, 64) {
        bail!(
            "PM release profile trusted baseline digest must be 64 lowercase hexadecimal characters"
        );
    }
    Ok(digest)
}

fn valid_trusted_baseline_ref(reference: &str) -> bool {
    if lower_hex(reference, 40) {
        return true;
    }
    let Some(tag) = reference.strip_prefix("refs/tags/") else {
        return false;
    };
    !tag.is_empty()
        && !tag.ends_with('.')
        && !tag.ends_with('/')
        && !tag.contains("..")
        && !tag.contains("//")
        && !tag.contains("@{")
        && !tag
            .split('/')
            .any(|component| component.starts_with('.') || component.ends_with(".lock"))
        && !tag.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(
                    byte,
                    b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\' | b'#'
                )
        })
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn run_command(repo: &Path, command: &[String]) -> Result<()> {
    validate_command(command)?;
    let args = command
        .iter()
        .skip(1)
        .map(String::as_str)
        .collect::<Vec<_>>();
    super::run_checked(repo, "cargo", &args)
}

fn run_profile_command(repo: &Path, command: &[String]) -> Result<()> {
    validate_profile_command(command)?;
    let args = command
        .iter()
        .skip(1)
        .map(String::as_str)
        .collect::<Vec<_>>();
    super::run_checked(repo, "cargo", &args)
}

fn run_profile_checks(repo: &Path, profile: &ReleaseProfile) -> Result<()> {
    for command in &profile.checks {
        run_profile_command(repo, command)?;
    }
    println!("Workdeck PM checks passed.");
    Ok(())
}

fn check(repo: &Path, profile: String) -> Result<()> {
    let profile_document = load_profile(repo, &profile)?;
    run_profile_checks(repo, &profile_document)
}

fn performance(repo: &Path, profile: String) -> Result<()> {
    let profile_document = load_profile(repo, &profile)?;
    let mut ran = false;
    for command in &profile_document.checks {
        if command.get(1).map(String::as_str) == Some("run") {
            run_profile_command(repo, command)?;
            ran = true;
        }
    }
    if !ran {
        bail!("PM release profile has no performance checks");
    }
    println!("Workdeck PM performance checks passed.");
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes =
        fs::read(path).with_context(|| format!("read pinned validator {}", path.display()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_commands_are_bounded_to_cargo_test_and_build() {
        assert!(validate_command(&["cargo".into(), "test".into()]).is_ok());
        assert!(validate_command(&["cargo".into(), "build".into()]).is_ok());
        assert!(validate_command(&["sh".into(), "-c".into(), "rm -rf /".into()]).is_err());
        assert!(validate_command(&["cargo".into(), "run".into()]).is_err());
    }

    #[test]
    fn profile_performance_commands_are_exact_and_bounded() {
        let features = [
            "cargo",
            "run",
            "--locked",
            "--release",
            "-p",
            "workdeck-pm",
            "--example",
            "projection_bench",
            "--",
            "features",
            "40000",
            "--enforce-targets",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        let issues = {
            let mut command = features.clone();
            command[9] = "issues".into();
            command[10] = "10000".into();
            command
        };
        assert!(validate_performance_command(&features).is_ok());
        assert!(validate_performance_command(&issues).is_ok());

        let mut shell = features.clone();
        shell[7] = "sh".into();
        assert!(validate_performance_command(&shell).is_err());
        let mut wrong_count = features;
        wrong_count[10] = "39999".into();
        assert!(validate_performance_command(&wrong_count).is_err());
    }

    #[test]
    fn validator_pin_is_deterministic_for_the_current_source() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let path = root.join(VALIDATOR_SOURCE);
        let first = sha256_file(&path).unwrap();
        let second = sha256_file(&path).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn checked_in_profile_contains_both_bounded_performance_workloads() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let profile = load_profile(&root, DEFAULT_PROFILE).unwrap();
        let performance = profile
            .checks
            .iter()
            .filter(|command| command.get(1).map(String::as_str) == Some("run"))
            .collect::<Vec<_>>();
        assert_eq!(performance.len(), 2);
        assert!(performance.iter().any(|command| {
            command.get(9).map(String::as_str) == Some("features")
                && command.get(10).map(String::as_str) == Some("40000")
        }));
        assert!(performance.iter().any(|command| {
            command.get(9).map(String::as_str) == Some("issues")
                && command.get(10).map(String::as_str) == Some("10000")
        }));
    }

    #[test]
    fn trusted_baseline_requires_a_commit_or_tag_and_sha256_digest() {
        let digest = "a".repeat(64);
        assert!(
            validate_trusted_baseline(&format!(
                "refs/tags/workdeck-pm-reviewed-baseline-2026-09-11#sha256:{digest}"
            ))
            .is_ok()
        );
        assert!(
            validate_trusted_baseline(&format!(
                "0123456789abcdef0123456789abcdef01234567#sha256:{digest}"
            ))
            .is_ok()
        );

        for value in [
            "workdeck-pm-reviewed-baseline:2026-09-11",
            "refs/heads/main#sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "refs/tags/review#sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "refs/tags/review#sha256:short",
            "refs/tags/a/./b#sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "refs/tags/.hidden/b#sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "refs/tags/a/b.lock/c#sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(
                validate_trusted_baseline(value).is_err(),
                "arbitrary or unverifiable baseline accepted: {value}"
            );
        }
    }

    #[test]
    fn profile_rejects_a_baseline_digest_that_drifts_from_the_validator_pin() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let profile_text = fs::read_to_string(root.join(PROFILE_PATH)).unwrap();
        let actual = sha256_file(&root.join(VALIDATOR_SOURCE)).unwrap();
        let drifted = profile_text.replace(
            &format!("#sha256:{actual}"),
            &format!("#sha256:{}", "b".repeat(64)),
        );
        assert_ne!(
            drifted, profile_text,
            "checked-in profile must pin the baseline with the validator digest"
        );
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(temp.path().join("ci")).unwrap();
        fs::create_dir_all(temp.path().join("xtask/src")).unwrap();
        fs::write(temp.path().join(PROFILE_PATH), drifted).unwrap();
        fs::write(
            temp.path().join(VALIDATOR_SOURCE),
            fs::read(root.join(VALIDATOR_SOURCE)).unwrap(),
        )
        .unwrap();
        assert!(
            load_profile(temp.path(), DEFAULT_PROFILE).is_err(),
            "a trusted baseline digest that disagrees with the validator pin must fail"
        );
        assert!(load_profile(&root, DEFAULT_PROFILE).is_ok());
    }

    #[test]
    fn standalone_profile_rejects_removed_or_weakened_required_commands() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let profile = load_profile(&root, DEFAULT_PROFILE).unwrap();

        let mut weakened = profile.checks.clone();
        weakened[0][4] = "workdeck-cli".into();
        assert!(
            validate_standalone_commands(&weakened, &profile.release_build).is_err(),
            "replacing the PM package with an unrelated package must fail"
        );

        let mut removed = profile.checks.clone();
        removed.pop();
        assert!(
            validate_standalone_commands(&removed, &profile.release_build).is_err(),
            "removing a required check must fail"
        );

        let mut weakened_release = profile.release_build.clone();
        weakened_release.pop();
        assert!(
            validate_standalone_commands(&profile.checks, &weakened_release).is_err(),
            "weakening the release build must fail"
        );
    }
}
