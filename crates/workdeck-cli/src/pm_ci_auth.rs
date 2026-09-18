//! Offline producer authentication with caller-pinned policy and source identity.
use super::{
    pm_ci::CiOptions,
    pm_cli::{self, CommandFailure},
};
use serde_json::json;
use std::path::Path;
use workdeck_pm::*;

pub(super) fn run(
    cwd: &Path,
    options: &CiOptions,
    report_file: &Path,
    policy_file: &Path,
    expected_policy: &str,
    expected_commit: &str,
) -> anyhow::Result<()> {
    let mut source = json!({"repository":null,"root":cwd,"role":"producer_authentication"});
    (|| -> workdeck_pm::Result<()> {
        let expected_policy = expected_policy.parse()?;
        let expected_commit = expected_commit.parse()?;
        let policy = ProducerTrustPolicy::from_json(&pm_cli::read_regular_input(
            cwd,
            policy_file,
            MAX_PRODUCER_POLICY_BYTES as u64,
        )?)?;
        let envelope = SignedCheckReport::from_json(&pm_cli::read_regular_input(
            cwd,
            report_file,
            MAX_SIGNED_REPORT_BYTES as u64,
        )?)?;
        let authenticated = authenticate_check_report(
            &envelope,
            &policy,
            &expected_policy,
            &expected_commit,
            chrono::Utc::now(),
        )?;
        source["repository"] = json!(authenticated.source.repository);
        source["revision"] = json!(authenticated.source);
        source["policy"] = json!(authenticated.policy);
        pm_cli::emit(
            options.json,
            "ci.authenticate",
            &source,
            &authenticated,
            None,
        )
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: source,
    })?;
    Ok(())
}

pub(super) fn inspect(cwd: &Path, options: &CiOptions, policy_file: &Path) -> anyhow::Result<()> {
    let source = json!({"repository":null,"root":cwd,"role":"producer_policy_inspection"});
    (|| -> workdeck_pm::Result<()> {
        let policy = ProducerTrustPolicy::from_json(&pm_cli::read_regular_input(
            cwd,
            policy_file,
            MAX_PRODUCER_POLICY_BYTES as u64,
        )?)?;
        pm_cli::emit(
            options.json,
            "ci.policy",
            &source,
            &json!({"policy":policy,"fingerprint":policy.fingerprint()?}),
            None,
        )
    })()
    .map_err(|error| CommandFailure {
        error,
        source_identity: source,
    })?;
    Ok(())
}
