//! Shared release identity policy and bounded native verifier lifecycle.
use anyhow::{Context, Result, bail};

/// Authenticate an owned binary snapshot with the declared GitHub CLI verifier.
/// Returns exactly the supplied bytes only after verification succeeds.
pub fn authenticate_binary(
    binary: Vec<u8>,
    bundle: &[u8],
    name: &str,
    repo: &str,
    digest: &str,
    reference: &str,
) -> Result<Vec<u8>> {
    let policy = verification_args(repo, digest, reference)?;
    authenticate_snapshot(binary, bundle, name, |binary, bundle| {
        use std::process::{Command, Stdio};
        let mut child = Command::new("gh")
            .args(["attestation", "verify"])
            .arg(binary)
            .arg("--bundle")
            .arg(bundle)
            .args(&policy)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .env("GH_PROMPT_DISABLED", "1")
            .spawn()
            .context("start required gh attestation verifier")?;
        wait_verifier(&mut child, std::time::Duration::from_secs(120))
    })
}

fn authenticate_snapshot(
    binary: Vec<u8>,
    bundle: &[u8],
    name: &str,
    verify: impl FnOnce(&std::path::Path, &std::path::Path) -> Result<()>,
) -> Result<Vec<u8>> {
    anyhow::ensure!(
        matches!(name, "workdeck" | "workdeck.exe"),
        "unexpected executable name"
    );
    anyhow::ensure!(
        !binary.is_empty() && binary.len() <= 2 * 1024 * 1024 * 1024,
        "invalid binary size"
    );
    anyhow::ensure!(
        !bundle.is_empty() && bundle.len() <= 1024 * 1024,
        "invalid attestation bundle size"
    );
    let snapshot = tempfile::Builder::new()
        .prefix("workdeck-authenticate-")
        .tempdir()?;
    let binary_path = snapshot.path().join(name);
    let bundle_path = snapshot.path().join("bundle.json");
    std::fs::write(&binary_path, &binary)?;
    std::fs::write(&bundle_path, bundle)?;
    verify(&binary_path, &bundle_path)?;
    // Detect accidental verifier-side mutation; verification must refer to the
    // snapshot that the caller receives, not another on-disk binary.
    anyhow::ensure!(
        std::fs::read(&binary_path)? == binary && std::fs::read(&bundle_path)? == bundle,
        "verification snapshot changed"
    );
    Ok(binary)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_returns_exact_bytes_and_cleans_snapshot() {
        let mut location = None;
        let bytes = authenticate_snapshot(
            b"binary\0bytes".to_vec(),
            b"bundle",
            "workdeck",
            |binary, bundle| {
                assert_eq!(std::fs::read(binary)?, b"binary\0bytes");
                assert_eq!(std::fs::read(bundle)?, b"bundle");
                location = Some(binary.parent().unwrap().to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(bytes, b"binary\0bytes");
        assert!(!location.unwrap().exists());
    }

    #[test]
    fn failed_or_mutating_verifier_never_returns_installable_bytes() {
        for mutate in [false, true] {
            let mut location = None;
            let result = authenticate_snapshot(
                b"binary".to_vec(),
                b"bundle",
                "workdeck.exe",
                |binary, _| {
                    location = Some(binary.parent().unwrap().to_owned());
                    if mutate {
                        std::fs::write(binary, b"changed")?;
                        Ok(())
                    } else {
                        bail!("verification failed")
                    }
                },
            );
            assert!(result.is_err());
            assert!(!location.unwrap().exists());
        }
    }
}
