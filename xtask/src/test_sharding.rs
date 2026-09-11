//! Native Cargo test scheduling policy replacing Hunk's Bun test sharding helper.
//!
//! Workdeck's workspace tests are Cargo-owned. Linux can still opt into a
//! bounded build parallelism level, while the test harness itself remains one
//! deterministic workspace invocation on every host.

use anyhow::{Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const SOURCE_PATH: &str = "scripts/run-test-suite.test.ts";
const SOURCE_BYTES: usize = 2_555;
const SOURCE_LINES: usize = 79;
const SOURCE_SHA256: &str = "7db067c683d0f16daf940f21b9bee91c1d47676f8d38c5bfce326b09e81243d9";
const RUNNER_PATH: &str = "scripts/run-test-suite.ts";
const RUNNER_BYTES: usize = 5_558;
const RUNNER_LINES: usize = 167;
const RUNNER_SHA256: &str = "3fbee1810fbe770035651aa88372f2ed9fa7ca34096009191cb086dea113d598";

/// Resolve the bounded test parallelism policy from the source helper.
pub(crate) fn resolve_test_shard_count(
    available_cpus: usize,
    override_value: Option<&str>,
    platform: &str,
) -> Result<usize> {
    if platform != "linux" {
        return Ok(1);
    }
    if let Some(value) = override_value {
        let parsed = value.parse::<u64>().ok();
        let Some(parsed) = parsed.filter(|value| (1..=64).contains(value)) else {
            if value.parse::<u64>().is_ok_and(|value| value > 64) {
                bail!("WORKDECK_TEST_SHARDS cannot exceed 64");
            }
            bail!("WORKDECK_TEST_SHARDS must be a positive safe integer");
        };
        return Ok(parsed as usize);
    }
    Ok(available_cpus.clamp(1, 2))
}

/// Build the one native Cargo command used for a test shard.
pub(crate) fn build_test_shard_command(
    repo: &Path,
    shard: usize,
    total: usize,
    extra: &[String],
) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(repo)
        .args(["test", "--locked", "--workspace", "--all-targets"]);
    if total > 1 {
        // Cargo's harness does not expose Bun's file-level `--shard` switch.
        // Keep each native worker deterministic and let CI schedule distinct
        // invocations when it needs true process-level sharding.
        command.args(["--", "--test-threads", "1"]);
        command.env("WORKDECK_TEST_SHARD", format!("{shard}/{total}"));
    }
    command.args(extra);
    command
}

#[allow(dead_code)]
pub(crate) trait KillableProcess {
    fn terminate(&mut self) -> std::io::Result<()>;
}

impl KillableProcess for std::process::Child {
    fn terminate(&mut self) -> std::io::Result<()> {
        self.kill()
    }
}

/// Best-effort termination mirrors the source's tolerance for an already-dead shard.
#[allow(dead_code)]
pub(crate) fn terminate_test_shard_processes<P: KillableProcess>(processes: &mut [P]) {
    for process in processes {
        let _ = process.terminate();
    }
}

fn pinned_source(repo: &Path, commit: &str) -> Result<Vec<u8>> {
    let source = crate::git_stdout_bytes(repo, ["show", &format!("{commit}:{SOURCE_PATH}")])?;
    ensure!(
        source.len() == SOURCE_BYTES,
        "pinned {SOURCE_PATH} {commit} changed size: {} != {SOURCE_BYTES}",
        source.len()
    );
    ensure!(
        source.split(|byte| *byte == b'\n').count() == SOURCE_LINES + 1,
        "pinned {SOURCE_PATH} {commit} changed line count"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&source)) == SOURCE_SHA256,
        "pinned {SOURCE_PATH} {commit} changed SHA-256"
    );
    Ok(source)
}

/// Verify the complete pinned sharding-test surface and native Cargo adapter.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "test-sharding verifier received unexpected baseline {baseline}"
    );
    let source = pinned_source(repo, BASELINE)?;
    let stable = pinned_source(repo, STABLE)?;
    ensure!(
        source == stable,
        "pinned test-sharding suite diverged between pins"
    );
    let source = std::str::from_utf8(&source)?;
    for pin in [BASELINE, STABLE] {
        let runner = crate::git_stdout_bytes(repo, ["show", &format!("{pin}:{RUNNER_PATH}")])?;
        ensure!(
            runner.len() == RUNNER_BYTES,
            "pinned {RUNNER_PATH} {pin} changed size: {} != {RUNNER_BYTES}",
            runner.len()
        );
        ensure!(
            runner.split(|byte| *byte == b'\n').count() == RUNNER_LINES + 1,
            "pinned {RUNNER_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&runner)) == RUNNER_SHA256,
            "pinned {RUNNER_PATH} {pin} changed SHA-256"
        );
    }
    for marker in [
        "from \"bun:test\"",
        "buildTestShardCommand",
        "DEFAULT_TEST_PATTERNS",
        "resolveTestShardCount",
        "terminateTestShardProcesses",
        "available CPUs up to the automatic Linux cap",
        "explicit positive shard count on Linux",
        "keeps non-Linux suites serial",
        "rejects malformed or excessive Linux shard overrides",
        "builds serial and sharded Bun commands",
        "forwards termination while tolerating an already stopped shard",
        "--shard=2/4",
        "HUNK_TEST_SHARDS",
    ] {
        ensure!(
            source.contains(marker),
            "pinned test-sharding suite is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        (
            "xtask/src/test_sharding.rs",
            "pub(crate) fn resolve_test_shard_count(",
        ),
        (
            "xtask/src/test_sharding.rs",
            "pub(crate) fn build_test_shard_command(",
        ),
        (
            "xtask/src/test_sharding.rs",
            "pub(crate) fn terminate_test_shard_processes",
        ),
        (
            "xtask/src/test_sharding.rs",
            "native_rust_test_sharding_replaces_both_pinned_suites",
        ),
        (
            "xtask/src/main.rs",
            "test_sharding::build_test_shard_command(",
        ),
        ("xtask/src/main.rs", "WORKDECK_TEST_SHARDS"),
        ("xtask/src/main.rs", "Some(\"test\")"),
    ] {
        let native = fs::read_to_string(repo.join(path))?;
        ensure!(
            native.contains(marker),
            "test-sharding native surface {path} is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/test-sharding-migration.md"))?;
    for marker in [
        SOURCE_PATH,
        "2,555",
        SOURCE_SHA256,
        "Cargo",
        "Linux",
        "WORKDECK_TEST_SHARDS",
        "non-Linux",
        "No Bun",
    ] {
        ensure!(
            docs.contains(marker),
            "test-sharding migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Process {
        fail: bool,
        signals: usize,
    }

    impl KillableProcess for Process {
        fn terminate(&mut self) -> std::io::Result<()> {
            self.signals += 1;
            if self.fail {
                Err(std::io::Error::other("already stopped"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn native_rust_test_sharding_replaces_both_pinned_suites() {
        let repo = crate::repo_root().unwrap();
        verify(&repo, BASELINE).unwrap();
    }

    #[test]
    fn test_sharding_policy_matches_linux_and_non_linux_contracts() {
        assert_eq!(resolve_test_shard_count(1, None, "linux").unwrap(), 1);
        assert_eq!(resolve_test_shard_count(2, None, "linux").unwrap(), 2);
        assert_eq!(resolve_test_shard_count(32, None, "linux").unwrap(), 2);
        assert_eq!(resolve_test_shard_count(32, Some("1"), "linux").unwrap(), 1);
        assert_eq!(
            resolve_test_shard_count(2, Some("16"), "linux").unwrap(),
            16
        );
        assert_eq!(resolve_test_shard_count(32, None, "win32").unwrap(), 1);
        assert_eq!(
            resolve_test_shard_count(32, Some("16"), "darwin").unwrap(),
            1
        );
        assert!(resolve_test_shard_count(8, Some("0"), "linux").is_err());
        assert!(resolve_test_shard_count(8, Some("2.5"), "linux").is_err());
        assert!(resolve_test_shard_count(8, Some("999999999999999999999999"), "linux").is_err());
        assert!(
            resolve_test_shard_count(8, Some("65"), "linux")
                .unwrap_err()
                .to_string()
                .contains("cannot exceed 64")
        );
    }

    #[test]
    fn native_cargo_command_keeps_serial_and_sharded_shapes() {
        let serial = build_test_shard_command(Path::new("/opt/workdeck"), 1, 1, &[]);
        assert_eq!(
            serial.get_args().collect::<Vec<_>>(),
            ["test", "--locked", "--workspace", "--all-targets"]
        );
        let extra = ["--nocapture".to_owned()];
        let sharded = build_test_shard_command(Path::new("/opt/workdeck"), 2, 4, &extra);
        assert_eq!(
            sharded.get_args().collect::<Vec<_>>(),
            [
                "test",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "--test-threads",
                "1",
                "--nocapture"
            ]
        );
        let shard_env = sharded
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new("WORKDECK_TEST_SHARD"))
            .map(|(_, value)| value);
        assert_eq!(shard_env, Some(Some(std::ffi::OsStr::new("2/4"))));
    }

    #[test]
    fn termination_ignores_an_already_stopped_process() {
        let mut processes = [
            Process {
                fail: false,
                signals: 0,
            },
            Process {
                fail: true,
                signals: 0,
            },
        ];
        terminate_test_shard_processes(&mut processes);
        assert_eq!(processes[0].signals, 1);
        assert_eq!(processes[1].signals, 1);
    }
}
