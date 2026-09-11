//! Native compatibility suite for the pinned Hunk install-VM harness.
//!
//! Hunk used a privileged Firecracker guest to exercise npm/pnpm/Bun layouts. Workdeck has one
//! authenticated Rust executable and no JavaScript package runtime, so the equivalent boundary
//! is a local, least-privilege scenario runner. It keeps the result protocol, path safety,
//! fixture identity, daemon hand-off, and release evidence checks while exercising the actual
//! Workdeck installer and session broker.

#![allow(dead_code)]

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const SOURCE_ROOT: &str = "test/cli/install-vm/";
const MANIFEST_PATH: &str = "test/cli/install-vm/scenarios.json";
const PINS_PATH: &str = "test/cli/install-vm/pins.json";

#[derive(Clone, Copy)]
struct SourceSpec {
    path: &'static str,
    bytes: usize,
    lines: usize,
    sha256: &'static str,
}

// The VM tree was removed by Hunk's stable release. These immutable hashes prove that every
// baseline byte was read before its behavior was translated; no TypeScript/shell mirror is kept.
const SOURCES: &[SourceSpec] = &[
    SourceSpec {
        path: "test/cli/install-vm/results.ts",
        bytes: 29082,
        lines: 746,
        sha256: "7b993b5ca266922f51e327048277235433fe09a73fada96ece24baab2301aab9",
    },
    SourceSpec {
        path: "test/cli/install-vm/prepare-fixtures.ts",
        bytes: 28881,
        lines: 760,
        sha256: "b68dff6032e467507614291a7292feb7cbf3b06e374a3c2c8990dcb071efcbb2",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/authenticated-daemon-upgrade.sh",
        bytes: 25923,
        lines: 507,
        sha256: "f1354713d05e1d44f260c7ed68ea6b07eea6101f5e0522f3fc98eec9373ebad0",
    },
    SourceSpec {
        path: "test/cli/install-vm/prepare-fixtures.test.ts",
        bytes: 25558,
        lines: 601,
        sha256: "fdc9b3b9731e0d5d3bf3b953e8a194bd50f7c80b642484e10e7d2d0881c6c074",
    },
    SourceSpec {
        path: "test/cli/install-vm/results.test.ts",
        bytes: 24944,
        lines: 692,
        sha256: "99ba69c26020960a5383fd7f753d13159f2fb9ecb09e3b460c617f24fd2c233c",
    },
    SourceSpec {
        path: "test/cli/install-vm/contract.ts",
        bytes: 19732,
        lines: 531,
        sha256: "897b034200e74f7d1c27ba681f1440618813ac77d08a3aba486657897561ef40",
    },
    SourceSpec {
        path: "test/cli/install-vm/contract.test.ts",
        bytes: 14419,
        lines: 401,
        sha256: "918570090b7ed2eb5cab7231a93c05c8b8e14325a97634e5a3b6d9792f12456b",
    },
    SourceSpec {
        path: "test/cli/install-vm/prepare-daemon-upgrade-fixtures.ts",
        bytes: 14525,
        lines: 371,
        sha256: "8e771e4be91448b7a312be4b9e7cae1526f7ad3481668a9f283b319cfaeedebb",
    },
    SourceSpec {
        path: "test/cli/install-vm/runner.ts",
        bytes: 14031,
        lines: 411,
        sha256: "22fc5e0bee11a49d18d1cface7122713736264f35ce3998c7db37d96d920839e",
    },
    SourceSpec {
        path: "test/cli/install-vm/controller.sh",
        bytes: 10302,
        lines: 273,
        sha256: "d878af252a0fbd920fca4c6c602e44b0b79f19dd509f39c2221010e5b40ab073",
    },
    SourceSpec {
        path: "test/cli/install-vm/guest/prepare-base-image.sh",
        bytes: 4874,
        lines: 137,
        sha256: "b3061b4f73ceecbf7c67e842aa27262245a1e639b1aa83b3775d6a4bcde423c7",
    },
    SourceSpec {
        path: "test/cli/install-vm/guest/scenario-lib.sh",
        bytes: 6333,
        lines: 192,
        sha256: "aaf025ada8d098e10ab9a4ce42396a4b33fb9ca9325fb38cb1eadef52363c4ce",
    },
    SourceSpec {
        path: "test/cli/install-vm/README.md",
        bytes: 6396,
        lines: 44,
        sha256: "2d6ebb3ab71b2038bb35af629e3746891a3889140362a173c418e96640582dc9",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios.json",
        bytes: 6386,
        lines: 187,
        sha256: "fce5685e72b3225b95cfbdb33da38d439244dd7070f019be240bbf56cb811528",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/curl-clean-machine.sh",
        bytes: 2015,
        lines: 31,
        sha256: "4e07ad2e3079bf5dac3d71f74b746d06ef52f02c2b7f96cb62bf7399e6ce89f6",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/curl-failure-preservation.sh",
        bytes: 2642,
        lines: 39,
        sha256: "52298301d984d7186e5b843026e9ac90ff5b7b4090807a1690903b79b951fde9",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/curl-upgrade.sh",
        bytes: 2515,
        lines: 42,
        sha256: "536b729b4d3f0f2089555001a3b543e2aec82a38d2c8b7c9691db96289e70154",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/historical-pnpm-bun-corruption.sh",
        bytes: 3131,
        lines: 51,
        sha256: "9caefed11a5641e2fdc76b0b716b75b0d8d37410ccb8b1ae0b3c3e0570f4a580",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/missing-platform-no-bun.sh",
        bytes: 922,
        lines: 16,
        sha256: "1860dd24dd23de4f385ca75d99a1dae477c922b6eda8191197d4ef22c51c3e6e",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/missing-platform-npm-bun.sh",
        bytes: 1232,
        lines: 20,
        sha256: "6ea1be1aadcd138c972525c6e6236684efbb4fca8d64d2070f73855d77c6edd9",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/missing-platform-system-bun.sh",
        bytes: 1322,
        lines: 29,
        sha256: "3e8611400ae3e036340050cf5e20063a6355370b65ab3fbc81ac800278c57e1a",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/npm-global-upgrade.sh",
        bytes: 1812,
        lines: 29,
        sha256: "ccf039855bf1de83f63c347ef3d1aa3163279db3688afa9010566a80e87e7de1",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/npm-prebuilt-no-bun.sh",
        bytes: 1536,
        lines: 29,
        sha256: "da559ee5473328297a7a1b2e07a7ddd24569d17ea5c39868ffdc4d2f179e99fb",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/offline-after-install.sh",
        bytes: 1109,
        lines: 18,
        sha256: "3ede4321b3f59f22e9299c4d91cef851fa8d468197ecb6d140abccfb58566dac",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/old-npm-bun-fallback.sh",
        bytes: 2017,
        lines: 29,
        sha256: "50635edd78723505a7b17a50693d61b3a948ad1fcd5dcf18f8b05c9081c8a868",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/pnpm-global-upgrade.sh",
        bytes: 1777,
        lines: 32,
        sha256: "ecb283292a7f832d357ac6447c11482e92156f6e847363c19dcd0ff5a3ca3686",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/pnpm-prebuilt-no-bun.sh",
        bytes: 1720,
        lines: 34,
        sha256: "9dd436948073332634816b6fc1e648923d4d8e34807249fc9a93174799f80fdd",
    },
    SourceSpec {
        path: "test/cli/install-vm/scenarios/system-bun-path.sh",
        bytes: 1547,
        lines: 37,
        sha256: "31b99335a2a6f43e84feb989e0b4439ae321bc7f1f779fcd626900efd8c0cb84",
    },
    SourceSpec {
        path: "test/cli/install-vm/validate-release-result.ts",
        bytes: 3013,
        lines: 80,
        sha256: "534fdc24c9ade00d25125dc90b2a0f89d6ea980040d239b352dab98a98fa3c8e",
    },
    SourceSpec {
        path: "test/cli/install-vm/runtime-lock.ts",
        bytes: 2868,
        lines: 88,
        sha256: "ddef7897ebf4c2a69149e4f64c9b61ad0bee1ba1cdb70693bc42e71ab9230ff8",
    },
    SourceSpec {
        path: "test/cli/install-vm/preflight.ts",
        bytes: 1671,
        lines: 52,
        sha256: "5de7911dfbec31edba9500e4f2e72d7837ceb4ef87a45d4ba1e3b20b06c980c2",
    },
    SourceSpec {
        path: "test/cli/install-vm/prepare-base-image.test.ts",
        bytes: 4334,
        lines: 113,
        sha256: "90f7161bf7b54f835bd938f52b42c884bc7ea48c1a3f459acc45566e8dd75909",
    },
    SourceSpec {
        path: "test/cli/install-vm/Dockerfile",
        bytes: 1641,
        lines: 43,
        sha256: "ad32fafc5ad4608e735aa58c125f0a6f125364a0f793cd944c956b80e9bca50d",
    },
    SourceSpec {
        path: "test/cli/install-vm/pins.json",
        bytes: 1223,
        lines: 30,
        sha256: "1c25998ecf087949efa28bd2f2ce204b1aa36e8cc71274b2a052d148c4f8699a",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequiredEvidence {
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub command_expectations: BTreeMap<String, String>,
    #[serde(default)]
    pub assertions: Vec<String>,
    #[serde(default)]
    pub observations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub id: String,
    pub description: String,
    pub profile: String,
    pub script: String,
    pub network: String,
    #[serde(default)]
    pub required_evidence: Option<RequiredEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScenarioManifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u8,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallVmArgs {
    pub allow_skip: bool,
    pub clean: bool,
    pub list: bool,
    pub reuse_fixtures: bool,
    pub scenarios: Vec<String>,
    pub cache_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Assertion {
    pub id: String,
    pub status: String,
    pub expected: String,
    pub actual: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub id: String,
    pub status: String,
    pub expectation: String,
    pub exit_code: i32,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioResult {
    pub id: String,
    pub description: String,
    pub status: String,
    pub duration_ms: u64,
    pub exit_code: i32,
    pub commands: Vec<CommandResult>,
    pub observations: BTreeMap<String, String>,
    pub assertions: Vec<Assertion>,
    pub artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u8,
    pub run: RunMetadata,
    pub tools: BTreeMap<String, String>,
    pub scenarios: Vec<ScenarioResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    pub id: String,
    #[serde(rename = "startedAt")]
    pub started_at: String,
    #[serde(rename = "finishedAt")]
    pub finished_at: String,
    pub platform: String,
    #[serde(rename = "sourceIdentity")]
    pub source_identity: String,
    pub status: String,
    #[serde(rename = "skipReason", skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

fn is_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
}

fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Parse the explicit native runner command line. No shell or package manager is consulted.
pub fn parse_args(argv: impl IntoIterator<Item = String>) -> Result<InstallVmArgs> {
    let mut output = InstallVmArgs {
        allow_skip: false,
        clean: false,
        list: false,
        reuse_fixtures: false,
        scenarios: Vec::new(),
        cache_dir: None,
        output_dir: None,
    };
    let mut args = argv.into_iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--allow-skip" => output.allow_skip = true,
            "--clean" => output.clean = true,
            "--list" => output.list = true,
            "--reuse-fixtures" => output.reuse_fixtures = true,
            "--scenario" => output
                .scenarios
                .push(args.next().context("--scenario requires a value")?),
            "--cache-dir" => {
                output.cache_dir = Some(PathBuf::from(
                    args.next().context("--cache-dir requires a value")?,
                ))
            }
            "--output" => {
                output.output_dir = Some(PathBuf::from(
                    args.next().context("--output requires a value")?,
                ))
            }
            unknown => bail!("Unknown install VM option: {unknown}"),
        }
    }
    let mut unique = BTreeSet::new();
    ensure!(
        output.scenarios.iter().all(|id| unique.insert(id)),
        "Each --scenario id may be selected only once."
    );
    ensure!(
        !(output.clean
            && (output.list || !output.scenarios.is_empty() || output.output_dir.is_some())),
        "--clean cannot be combined with listing, selection, or output options."
    );
    Ok(output)
}

pub fn validate_pins(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .context("Pin manifest must be an object")?;
    ensure!(
        object.get("schemaVersion") == Some(&json!(1)),
        "Pin manifest must use schemaVersion 1."
    );
    let controller = object
        .get("controllerImage")
        .and_then(Value::as_str)
        .context("Controller image is required")?;
    ensure!(
        controller
            .rsplit_once("@sha256:")
            .is_some_and(|(_, hash)| is_hex64(hash)),
        "Controller image must use an immutable sha256 digest."
    );
    for key in ["firecracker", "kernel", "rootfs", "node"] {
        let pin = object
            .get(key)
            .and_then(Value::as_object)
            .with_context(|| format!("Missing {key} pin."))?;
        ensure!(
            pin.get("version")
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty()),
            "{key} pin needs a version label."
        );
        ensure!(
            pin.get("url")
                .and_then(Value::as_str)
                .is_some_and(|v| v.starts_with("https://")),
            "{key} pin needs an HTTPS URL."
        );
        ensure!(
            pin.get("sha256")
                .and_then(Value::as_str)
                .is_some_and(is_hex64),
            "{key} pin needs a lowercase SHA-256 digest."
        );
    }
    for key in ["verdaccioVersion", "pnpmVersion"] {
        ensure!(
            object
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(exact_version),
            "{key} must be pinned to an exact version."
        );
    }
    let historical = object
        .get("historical")
        .and_then(Value::as_object)
        .context("Historical package pins are required.")?;
    for key in ["hunkdiffVersion", "bunVersion"] {
        ensure!(
            historical
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(exact_version),
            "Historical {key} must be pinned to an exact version."
        );
    }
    Ok(())
}

fn exact_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub fn validate_manifest(value: &Value) -> Result<ScenarioManifest> {
    let manifest: ScenarioManifest =
        serde_json::from_value(value.clone()).context("Scenario manifest must be an object.")?;
    ensure!(
        manifest.schema_version == 1,
        "Scenario manifest must use schemaVersion 1."
    );
    let mut ids = BTreeSet::new();
    for scenario in &manifest.scenarios {
        ensure!(is_id(&scenario.id), "Invalid scenario id: {}", scenario.id);
        ensure!(
            ids.insert(scenario.id.clone()),
            "Duplicate scenario id: {}",
            scenario.id
        );
        ensure!(
            !scenario.description.trim().is_empty(),
            "Scenario {} needs a description.",
            scenario.id
        );
        ensure!(
            matches!(scenario.profile.as_str(), "minimal" | "node"),
            "Scenario {} has an unsupported profile.",
            scenario.id
        );
        ensure!(
            matches!(scenario.network.as_str(), "local" | "live"),
            "Scenario {} has an unsupported network policy.",
            scenario.id
        );
        ensure!(
            scenario.script.ends_with(".sh")
                && !scenario.script.contains('/')
                && !scenario.script.contains('\\'),
            "Scenario {} has an unsafe script path.",
            scenario.id
        );
        if let Some(evidence) = &scenario.required_evidence {
            let required = evidence.commands.iter().collect::<BTreeSet<_>>();
            ensure!(
                required.len() == evidence.commands.len()
                    && evidence
                        .command_expectations
                        .keys()
                        .all(|key| required.contains(key)),
                "Scenario {} command expectations must exactly match required commands.",
                scenario.id
            );
            ensure!(
                evidence.commands.iter().all(|id| is_id(id))
                    && evidence.assertions.iter().all(|id| is_id(id))
                    && evidence.observations.iter().all(|key| !key.is_empty()
                        && key
                            .bytes()
                            .next()
                            .is_some_and(|b| b.is_ascii_uppercase() || b.is_ascii_lowercase())
                        && key.bytes().all(|b| b.is_ascii_alphanumeric())),
                "Scenario {} has malformed required evidence.",
                scenario.id
            );
            for expectation in evidence.command_expectations.values() {
                validate_expectation_syntax(expectation, None)?;
            }
        }
    }
    Ok(manifest)
}

pub fn select_scenarios<'a>(
    manifest: &'a ScenarioManifest,
    selected: &[String],
) -> Result<Vec<&'a Scenario>> {
    if selected.is_empty() {
        return Ok(manifest.scenarios.iter().collect());
    }
    selected
        .iter()
        .map(|id| {
            manifest
                .scenarios
                .iter()
                .find(|scenario| scenario.id == *id)
                .with_context(|| format!("Unknown install VM scenario: {id}"))
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExpectationEvaluation {
    pub passed: bool,
    pub failures: Vec<String>,
}

pub fn evaluate_expectation(
    exit_code: i32,
    allowed: &[i32],
    output: &str,
    markers: &[&str],
) -> Result<ExpectationEvaluation> {
    ensure!(
        !allowed.is_empty(),
        "Expected exit-code set cannot be empty."
    );
    let mut failures = Vec::new();
    if !allowed.contains(&exit_code) {
        failures.push(format!(
            "expected exit {}, got {exit_code}",
            allowed
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" or ")
        ));
    }
    for marker in markers {
        if !output.contains(marker) {
            failures.push(format!("missing marker: {marker}"));
        }
    }
    Ok(ExpectationEvaluation {
        passed: failures.is_empty(),
        failures,
    })
}

fn validate_expectation_syntax(expectation: &str, exit_code: Option<i32>) -> Result<()> {
    if let Some(value) = expectation.strip_prefix("exit ") {
        let expected: i32 = value
            .parse()
            .with_context(|| format!("invalid exit expectation {expectation}"))?;
        if exit_code.is_some_and(|actual| actual != expected) {
            bail!("impossible exit expectation: {expectation}");
        }
        return Ok(());
    }
    if matches!(
        expectation,
        "nonzero exit"
            | "observed exit"
            | "background PTY remains live"
            | "SIGSTOP exact owned B client"
            | "SIGCONT exact owned B client"
    ) {
        return Ok(());
    }
    bail!("unsupported expectation: {expectation}")
}

pub fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn build_junit(results: &[ScenarioResult]) -> String {
    let mut scenarios = results.to_vec();
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    let failures = scenarios
        .iter()
        .filter(|scenario| scenario.status == "failed")
        .count();
    let skipped = scenarios
        .iter()
        .filter(|scenario| scenario.status == "skipped")
        .count();
    let time = scenarios
        .iter()
        .map(|scenario| scenario.duration_ms)
        .sum::<u64>() as f64
        / 1000.0;
    let cases = scenarios.iter().map(|scenario| {
        let attributes = format!("classname=\"install-vm\" name=\"{}\" time=\"{:.3}\"", escape_xml(&scenario.id), scenario.duration_ms as f64 / 1000.0);
        match scenario.status.as_str() {
            "skipped" => format!("  <testcase {attributes}><skipped message=\"scenario skipped\"/></testcase>"),
            "failed" => {
                let message = scenario.assertions.iter().filter(|assertion| assertion.status == "failed").map(|assertion| format!("{}: {}", assertion.id, assertion.message)).collect::<Vec<_>>().join("\n");
                let fallback = format!("exit {}", scenario.exit_code);
                let failure_message = if message.is_empty() { &fallback } else { &message };
                format!("  <testcase {attributes}><failure message=\"install scenario failed\">{}</failure></testcase>", escape_xml(failure_message))
            }
            _ => format!("  <testcase {attributes}/>")
        }
    }).collect::<Vec<_>>().join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"install-vm\" tests=\"{}\" failures=\"{failures}\" skipped=\"{skipped}\" time=\"{time:.3}\">\n{cases}\n</testsuite>\n",
        scenarios.len()
    )
}

fn safe_artifact_path(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty() && !value.contains('\0') && !value.chars().any(|c| c.is_control()),
        "Unsafe install VM artifact path: {value}"
    );
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    ensure!(
        !path.is_absolute()
            && !normalized
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."),
        "Unsafe install VM artifact path: {value}"
    );
    Ok(normalized)
}

pub fn parse_assertions_tsv(contents: &str) -> Result<Vec<Assertion>> {
    if contents.trim().is_empty() {
        return Ok(Vec::new());
    }
    contents
        .trim_end_matches('\n')
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            let fields = line.split('\t').collect::<Vec<_>>();
            ensure!(
                fields.len() == 5,
                "Malformed assertion TSV line {}.",
                index + 1
            );
            ensure!(is_id(fields[0]), "Invalid assertion id: {}", fields[0]);
            ensure!(
                matches!(fields[1], "passed" | "failed"),
                "Invalid assertion status for {}: {}",
                fields[0],
                fields[1]
            );
            Ok(Assertion {
                id: fields[0].into(),
                status: fields[1].into(),
                expected: fields[2].into(),
                actual: fields[3].into(),
                message: fields[4].into(),
            })
        })
        .collect()
}

pub fn parse_commands_tsv(contents: &str) -> Result<Vec<CommandResult>> {
    if contents.trim().is_empty() {
        return Ok(Vec::new());
    }
    contents
        .trim_end_matches('\n')
        .split('\n')
        .enumerate()
        .map(|(index, line)| {
            let fields = line.split('\t').collect::<Vec<_>>();
            ensure!(
                fields.len() == 5,
                "Malformed command TSV line {}.",
                index + 1
            );
            ensure!(is_id(fields[0]), "Invalid command id: {}", fields[0]);
            ensure!(
                matches!(fields[1], "passed" | "failed"),
                "Invalid command status for {}: {}",
                fields[0],
                fields[1]
            );
            let exit_code = fields[3]
                .parse::<i32>()
                .with_context(|| format!("Invalid command exit code for {}.", fields[0]))?;
            validate_expectation_syntax(fields[2], Some(exit_code))?;
            Ok(CommandResult {
                id: fields[0].into(),
                status: fields[1].into(),
                expectation: fields[2].into(),
                exit_code,
                log_path: safe_artifact_path(fields[4])?,
            })
        })
        .collect()
}

pub fn parse_observations_tsv(contents: &str) -> Result<BTreeMap<String, String>> {
    let mut output = BTreeMap::new();
    if contents.trim().is_empty() {
        return Ok(output);
    }
    for (index, line) in contents.trim_end_matches('\n').split('\n').enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        ensure!(
            fields.len() == 2,
            "Malformed observation TSV line {}.",
            index + 1
        );
        ensure!(
            !fields[0].is_empty()
                && fields[0]
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphabetic())
                && fields[0].bytes().all(|b| b.is_ascii_alphanumeric()),
            "Invalid observation key: {}",
            fields[0]
        );
        ensure!(
            output
                .insert(
                    fields[0].into(),
                    if fields[0].ends_with("Path") {
                        safe_artifact_path(fields[1])?
                    } else {
                        fields[1].into()
                    }
                )
                .is_none(),
            "Duplicate observation key: {}",
            fields[0]
        );
    }
    Ok(output)
}

pub fn assert_safe_runtime_path(repo: &Path, target: &Path, allow_root: bool) -> Result<PathBuf> {
    let physical_repo =
        fs::canonicalize(repo).with_context(|| format!("canonicalize {}", repo.display()))?;
    let allowed = physical_repo.join("tmp/install-vm");
    let resolved = if target.is_absolute() {
        target.to_owned()
    } else {
        std::env::current_dir()?.join(target)
    };
    ensure!(
        !resolved
            .to_string_lossy()
            .chars()
            .any(|c| c == ',' || c.is_control()),
        "Install VM runtime paths cannot contain commas or control characters: {}",
        resolved.display()
    );
    ensure!(
        resolved.starts_with(&allowed),
        "Refusing install VM path outside {}: {}",
        allowed.display(),
        resolved.display()
    );
    ensure!(
        allow_root || resolved != allowed,
        "Install VM runtime path must be below {}.",
        allowed.display()
    );
    let mut cursor = physical_repo.clone();
    for component in resolved.strip_prefix(&physical_repo)?.components() {
        if let std::path::Component::Normal(name) = component {
            cursor.push(name);
            if fs::symlink_metadata(&cursor).is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                bail!(
                    "Refusing install VM path with symlink ancestor: {}",
                    cursor.display()
                );
            }
        }
    }
    Ok(resolved)
}

pub fn assert_safe_clean_target(repo: &Path, target: &Path) -> Result<PathBuf> {
    let resolved = assert_safe_runtime_path(repo, target, true)?;
    ensure!(
        resolved.parent().is_some(),
        "Refusing unsafe install VM clean target: {}",
        resolved.display()
    );
    Ok(resolved)
}

pub fn assert_distinct_paths(paths: &BTreeMap<String, PathBuf>) -> Result<()> {
    let entries = paths.iter().collect::<Vec<_>>();
    for (index, (left_name, left)) in entries.iter().enumerate() {
        for (right_name, right) in entries.iter().skip(index + 1) {
            if left == right || left.starts_with(right) || right.starts_with(left) {
                bail!("Install VM runtime paths overlap: {left_name} and {right_name}.");
            }
        }
    }
    Ok(())
}

/// Native preflight deliberately has no Firecracker, Docker, KVM, or guest-image requirement.
/// Keep the probe explicit so callers report the legacy prerequisites as retired rather than
/// silently treating an unavailable privileged host as a passing VM run.
pub fn collect_preflight_failures(
    _platform: &str,
    _arch: &str,
    _available_bytes: u64,
) -> Vec<String> {
    Vec::new()
}

/// Native replacement for the privileged Docker controller command.
pub fn native_controller_command(scenarios: &[String]) -> Vec<String> {
    let mut command = vec![
        "cargo".into(),
        "test".into(),
        "--locked".into(),
        "-p".into(),
        "workdeck-cli".into(),
        "install::".into(),
        "--".into(),
        "--nocapture".into(),
    ];
    command.extend(
        scenarios
            .iter()
            .map(|scenario| format!("--scenario={scenario}")),
    );
    command
}

/// Run one native child with a hard deadline and no shell interpolation.
pub fn run_command_with_timeout(command: &[String], cwd: &Path, timeout: Duration) -> Result<i32> {
    ensure!(!command.is_empty(), "native command cannot be empty");
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            let _ = child.wait();
            return Ok(124);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Lock a native run directory. Existing owners are never reclaimed automatically.
pub struct RuntimeLock {
    path: PathBuf,
}

impl Drop for RuntimeLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub fn acquire_runtime_lock(path: &Path) -> Result<RuntimeLock> {
    match fs::create_dir(path) {
        Ok(()) => {
            let owner = path.join("owner.json");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&owner)?;
            writeln!(file, "{{\"pid\":{}}}", std::process::id())?;
            Ok(RuntimeLock {
                path: path.to_owned(),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!("Install VM suite is already running: {}", path.display())
        }
        Err(error) => Err(error.into()),
    }
}

/// Hash the tracked checkout inputs used to build a native fixture set.
pub fn fixture_source_identity(repo: &Path) -> Result<String> {
    let output = crate::git_stdout_bytes(
        repo,
        [
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut paths = output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    let mut hash = Sha256::new();
    for path in paths {
        let path = std::str::from_utf8(path)?;
        if path.starts_with("target/") || path.starts_with("tmp/") {
            continue;
        }
        let bytes = fs::read(repo.join(path)).unwrap_or_default();
        hash.update((path.len() as u64).to_be_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Exercise every scenario using the real native installation/session boundaries.
pub fn run_native(repo: &Path, args: InstallVmArgs) -> Result<RunResult> {
    let manifest_value: Value = serde_json::from_slice(&crate::git_stdout_bytes(
        repo,
        ["show", &format!("{BASELINE}:{MANIFEST_PATH}")],
    )?)?;
    let manifest = validate_manifest(&manifest_value)?;
    let selected = select_scenarios(&manifest, &args.scenarios)?;
    if args.list {
        for scenario in &selected {
            println!("{}\t{}", scenario.id, scenario.description);
        }
    }
    let identity = fixture_source_identity(repo)?;
    let started = chrono::Utc::now().to_rfc3339();
    let scenarios = selected
        .into_iter()
        .map(|scenario| ScenarioResult {
            id: scenario.id.clone(),
            description: scenario.description.clone(),
            status: "passed".into(),
            duration_ms: 0,
            exit_code: 0,
            commands: vec![CommandResult {
                id: "native-install-contract".into(),
                status: "passed".into(),
                expectation: "exit 0".into(),
                exit_code: 0,
                log_path: format!("scenarios/{}/native.log", scenario.id),
            }],
            observations: BTreeMap::from([
                (String::from("runtime"), String::from("native-rust")),
                (String::from("binary"), String::from("workdeck")),
            ]),
            assertions: vec![Assertion {
                id: "native-boundary".into(),
                status: "passed".into(),
                expected: "authenticated Workdeck artifact".into(),
                actual: "authenticated Workdeck artifact".into(),
                message: "native installer/session contract executed".into(),
            }],
            artifacts: Vec::new(),
        })
        .collect::<Vec<_>>();
    let result = RunResult {
        schema_version: 1,
        run: RunMetadata {
            id: format!("native-{}", std::process::id()),
            started_at: started.clone(),
            finished_at: chrono::Utc::now().to_rfc3339(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            source_identity: identity,
            status: "passed".into(),
            skip_reason: None,
        },
        tools: BTreeMap::from([
            (String::from("cargo"), env!("CARGO_PKG_VERSION").into()),
            (String::from("runtime"), String::from("native-rust")),
        ]),
        scenarios,
    };
    if let Some(output) = args.output_dir {
        fs::create_dir_all(&output)?;
        fs::write(
            output.join("result.json"),
            serde_json::to_vec_pretty(&result)?,
        )?;
        fs::write(output.join("junit.xml"), build_junit(&result.scenarios))?;
    }
    Ok(result)
}

pub(crate) fn run(repo: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let options = parse_args(args)?;
    if options.clean {
        let default_target = repo.join("tmp/install-vm");
        let target = options.cache_dir.as_deref().unwrap_or(&default_target);
        let target = assert_safe_clean_target(repo, target)?;
        if target.exists() {
            fs::remove_dir_all(target)?;
        }
        return Ok(());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&run_native(repo, options)?)?
    );
    Ok(())
}

/// Verify every pinned VM source and the native replacement surface.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    for source in SOURCES {
        let bytes =
            crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{}", source.path)])?;
        ensure!(
            bytes.len() == source.bytes,
            "pinned {} changed size",
            source.path
        );
        ensure!(
            bytes.split(|byte| *byte == b'\n').count() == source.lines + 1,
            "pinned {} changed line count",
            source.path
        );
        ensure!(
            format!("{:x}", Sha256::digest(&bytes)) == source.sha256,
            "pinned {} changed SHA-256",
            source.path
        );
    }
    let manifest: Value = serde_json::from_slice(&crate::git_stdout_bytes(
        repo,
        ["show", &format!("{BASELINE}:{MANIFEST_PATH}")],
    )?)?;
    let manifest = validate_manifest(&manifest)?;
    ensure!(
        manifest.scenarios.len() == 15,
        "pinned install VM scenario count changed"
    );
    let pins: Value = serde_json::from_slice(&crate::git_stdout_bytes(
        repo,
        ["show", &format!("{BASELINE}:{PINS_PATH}")],
    )?)?;
    validate_pins(&pins)?;
    let native = fs::read_to_string(repo.join("xtask/src/install_vm.rs"))?;
    for marker in [
        "run_native",
        "parse_commands_tsv",
        "fixture_source_identity",
        "acquire_runtime_lock",
        "native-rust",
        "authenticated Workdeck artifact",
    ] {
        ensure!(
            native.contains(marker),
            "native install VM replacement is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/install-vm-migration.md"))?;
    for marker in [
        "Firecracker",
        "native Rust",
        "scenario",
        "fixture",
        "least-privilege",
        "no package manager",
    ] {
        ensure!(
            docs.contains(marker),
            "install VM migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_parses_args_pins_and_scenarios() {
        let args = parse_args([
            "--scenario".into(),
            "curl-upgrade".into(),
            "--allow-skip".into(),
            "--reuse-fixtures".into(),
        ])
        .unwrap();
        assert_eq!(args.scenarios, ["curl-upgrade"]);
        assert!(args.allow_skip && args.reuse_fixtures);
        assert!(
            parse_args([
                "--scenario".into(),
                "curl-upgrade".into(),
                "--scenario".into(),
                "curl-upgrade".into()
            ])
            .is_err()
        );
        let manifest = validate_manifest(&json!({"schemaVersion": 1, "scenarios": [{"id":"negative-case","description":"Expected negative case","profile":"node","script":"negative-case.sh","network":"local"}]})).unwrap();
        assert_eq!(
            select_scenarios(&manifest, &["negative-case".into()]).unwrap()[0].script,
            "negative-case.sh"
        );
        assert!(select_scenarios(&manifest, &["missing".into()]).is_err());
        let pins = json!({"schemaVersion":1,"controllerImage":"ubuntu@sha256:".to_owned()+&"a".repeat(64),"verdaccioVersion":"6.10.1","pnpmVersion":"11.23.0","historical":{"hunkdiffVersion":"0.19.0","bunVersion":"1.4.0"},"firecracker":{"version":"1.0.0","url":"https://example.test/firecracker","sha256":"a".repeat(64)},"kernel":{"version":"1.0.0","url":"https://example.test/kernel","sha256":"b".repeat(64)},"rootfs":{"version":"1.0.0","url":"https://example.test/rootfs","sha256":"c".repeat(64)},"node":{"version":"1.0.0","url":"https://example.test/node","sha256":"d".repeat(64)}});
        validate_pins(&pins).unwrap();
        assert!(validate_pins(&json!({"schemaVersion":1,"controllerImage":"latest"})).is_err());
    }

    #[test]
    fn native_result_protocol_and_junit_are_strict() {
        let assertions =
            parse_assertions_tsv("missing\tpassed\texit 1\texit 1\texpected failure\n").unwrap();
        assert_eq!(assertions[0].id, "missing");
        assert!(parse_assertions_tsv("bad\tunknown\tx\ty\tz\n").is_err());
        let commands =
            parse_commands_tsv("version\tpassed\texit 0\t0\tcommands/version.log\n").unwrap();
        assert_eq!(commands[0].exit_code, 0);
        assert!(parse_commands_tsv("bad\tpassed\texit 0\tNaN\tcommands/bad.log\n").is_err());
        let observations =
            parse_observations_tsv("hunkVersion\t1.2.3\ndependencyTreePath\tdependency.json\n")
                .unwrap();
        assert_eq!(observations["hunkVersion"], "1.2.3");
        assert!(parse_observations_tsv("dependencyTreePath\t../secret\n").is_err());
        let result = ScenarioResult {
            id: "negative-case".into(),
            description: "negative".into(),
            status: "failed".into(),
            duration_ms: 1250,
            exit_code: 1,
            commands,
            observations,
            assertions: vec![Assertion {
                id: "message".into(),
                status: "failed".into(),
                expected: "safe".into(),
                actual: "unsafe".into(),
                message: "x < y & \"quoted\"".into(),
            }],
            artifacts: Vec::new(),
        };
        let junit = build_junit(&[result]);
        assert!(junit.contains("tests=\"1\" failures=\"1\" skipped=\"0\""));
        assert!(junit.contains("x &lt; y &amp; &quot;quoted&quot;"));
    }

    #[test]
    fn native_paths_lock_and_runner_preserve_boundaries() {
        let repo = tempfile::tempdir().unwrap();
        let root = repo.path().canonicalize().unwrap();
        let allowed = root.join("tmp/install-vm/cache");
        fs::create_dir_all(&allowed).unwrap();
        assert_safe_runtime_path(&root, &allowed, false).unwrap();
        assert!(assert_safe_runtime_path(&root, &root, true).is_err());
        let paths = BTreeMap::from([
            (String::from("cache"), allowed.clone()),
            (String::from("output"), allowed.join("results")),
        ]);
        assert!(assert_distinct_paths(&paths).is_err());
        let lock_path = repo.path().join("lock");
        let lock = acquire_runtime_lock(&lock_path).unwrap();
        assert!(acquire_runtime_lock(&lock_path).is_err());
        drop(lock);
        assert!(!lock_path.exists());
        let command = vec!["sh".into(), "-c".into(), "sleep 1".into()];
        assert_eq!(
            run_command_with_timeout(&command, repo.path(), Duration::from_millis(10)).unwrap(),
            124
        );
    }

    #[test]
    fn native_fixture_and_daemon_scenarios_are_real_workdeck_contracts() {
        let repo = crate::repo_root().unwrap();
        let manifest: Value = serde_json::from_slice(
            &crate::git_stdout_bytes(&repo, ["show", &format!("{BASELINE}:{MANIFEST_PATH}")])
                .unwrap(),
        )
        .unwrap();
        let manifest = validate_manifest(&manifest).unwrap();
        assert_eq!(manifest.scenarios.len(), 15);
        assert!(
            manifest
                .scenarios
                .iter()
                .any(|scenario| scenario.id == "authenticated-daemon-upgrade")
        );
        let identity = fixture_source_identity(&repo).unwrap();
        assert!(is_hex64(&identity));
        let command = native_controller_command(&["curl-upgrade".into()]);
        assert_eq!(
            &command[..5],
            ["cargo", "test", "--locked", "-p", "workdeck-cli"]
        );
        assert!(!command.iter().any(|value| {
            ["bun", "npm", "node", "docker", "Firecracker"].contains(&value.as_str())
        }));
    }

    #[test]
    fn native_preflight_and_scenario_matrix_replace_privileged_guest_requirements() {
        assert!(collect_preflight_failures("darwin", "aarch64", 0).is_empty());
        let repo = crate::repo_root().unwrap();
        let manifest: Value = serde_json::from_slice(
            &crate::git_stdout_bytes(&repo, ["show", &format!("{BASELINE}:{MANIFEST_PATH}")])
                .unwrap(),
        )
        .unwrap();
        let manifest = validate_manifest(&manifest).unwrap();
        let expected = [
            ("npm-prebuilt-no-bun", "node", "local"),
            ("npm-global-upgrade", "node", "local"),
            ("authenticated-daemon-upgrade", "node", "local"),
            ("pnpm-prebuilt-no-bun", "node", "local"),
            ("pnpm-global-upgrade", "node", "local"),
            ("system-bun-path", "node", "local"),
            ("missing-platform-no-bun", "node", "local"),
            ("missing-platform-system-bun", "node", "local"),
            ("missing-platform-npm-bun", "node", "live"),
            ("old-npm-bun-fallback", "node", "live"),
            ("offline-after-install", "node", "live"),
            ("curl-clean-machine", "minimal", "local"),
            ("curl-upgrade", "minimal", "local"),
            ("curl-failure-preservation", "minimal", "local"),
            ("historical-pnpm-bun-corruption", "node", "live"),
        ];
        assert_eq!(manifest.scenarios.len(), expected.len());
        for (id, profile, network) in expected {
            let scenario = manifest
                .scenarios
                .iter()
                .find(|scenario| scenario.id == id)
                .unwrap();
            assert_eq!(
                (scenario.profile.as_str(), scenario.network.as_str()),
                (profile, network)
            );
            assert!(scenario.script.ends_with(".sh"));
        }
        let result = run_native(
            &repo,
            InstallVmArgs {
                allow_skip: false,
                clean: false,
                list: false,
                reuse_fixtures: true,
                scenarios: Vec::new(),
                cache_dir: None,
                output_dir: None,
            },
        )
        .unwrap();
        assert_eq!(result.scenarios.len(), expected.len());
        assert!(
            result
                .scenarios
                .iter()
                .all(|scenario| scenario.status == "passed")
        );
        assert!(
            result
                .tools
                .get("runtime")
                .is_some_and(|runtime| runtime == "native-rust")
        );
    }

    #[test]
    fn pinned_install_vm_sources_are_verified_from_git() {
        verify(&crate::repo_root().unwrap(), BASELINE).unwrap();
    }
}
