//! Shared release identity policy and bounded native verifier lifecycle.
use anyhow::{Context, Result, bail};

pub fn verification_args(repo: &str, digest: &str, reference: &str) -> Result<Vec<String>> {
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        bail!("expected explicit GitHub OWNER/REPO");
    }
    if !matches!(digest.len(), 40 | 64) || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("expected full source commit digest");
    }
    if !reference.starts_with("refs/tags/")
        || reference.len() == "refs/tags/".len()
        || reference.chars().any(char::is_whitespace)
    {
        bail!("release verification requires a full tag ref");
    }
    Ok(vec![
        "--hostname".into(),
        "github.com".into(),
        "--repo".into(),
        repo.into(),
        "--signer-workflow".into(),
        format!("{repo}/.github/workflows/release.yml"),
        "--source-digest".into(),
        digest.into(),
        "--source-ref".into(),
        reference.into(),
        "--cert-oidc-issuer".into(),
        "https://token.actions.githubusercontent.com".into(),
        "--predicate-type".into(),
        "https://slsa.dev/provenance/v1".into(),
        "--deny-self-hosted-runners".into(),
    ])
}

pub fn wait_verifier(child: &mut std::process::Child, timeout: std::time::Duration) -> Result<()> {
    use std::time::{Duration, Instant};
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    bail!("attestation verifier failed: {status}");
                }
                break;
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                if let Err(error) = result {
                    return Err(error).context("poll attestation verifier");
                }
                bail!("attestation verifier exceeded deadline {timeout:?}");
            }
        }
    }
    Ok(())
}
