//! Structural/subject checks only: these do not authenticate a builder or signature.
use anyhow::{Context, Result, bail};
use serde_json::Value;
use workdeck_cli::install::attestation::{verification_args, wait_verifier};

pub(crate) fn verify_ci_snapshot(binary: &[u8], bundle: &[u8], name: &str) -> Result<()> {
    // Private temporary copies prevent later changes to original inputs from changing
    // what the verifier reads. The caller archives these same in-memory bytes.
    let snapshot = verification_snapshot(binary, bundle, name)?;
    verify(
        [
            "--ci".to_owned(),
            snapshot
                .path()
                .join(name)
                .to_str()
                .context("non-UTF-8 verifier snapshot path")?
                .to_owned(),
            snapshot
                .path()
                .join("bundle.json")
                .to_str()
                .context("non-UTF-8 verifier bundle path")?
                .to_owned(),
        ]
        .into_iter(),
    )
}

fn verification_snapshot(binary: &[u8], bundle: &[u8], name: &str) -> Result<tempfile::TempDir> {
    if !matches!(name, "workdeck" | "workdeck.exe") {
        bail!("unexpected executable name for verification snapshot");
    }
    decode_bundle(bundle)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix("workdeck-provenance-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir()?;
    std::fs::write(directory.path().join(name), binary)?;
    std::fs::write(directory.path().join("bundle.json"), bundle)?;
    Ok(directory)
}

/// Invoke the declared native GitHub CLI verifier; decoding alone is insufficient.
pub(crate) fn verify(args: impl Iterator<Item = String>) -> Result<()> {
    use std::{
        process::{Command, Stdio},
        time::Duration,
    };
    let mut args: Vec<_> = args.collect();
    if args.first().is_some_and(|arg| arg == "--ci") {
        if args.len() != 3 {
            bail!("usage: cargo xtask release provenance-verify --ci BINARY BUNDLE");
        }
        args.remove(0);
        for name in ["GITHUB_REPOSITORY", "GITHUB_SHA", "GITHUB_REF"] {
            args.push(std::env::var(name).with_context(|| format!("missing {name}"))?);
        }
    }
    if args.len() != 5 {
        bail!(
            "usage: cargo xtask release provenance-verify BINARY BUNDLE OWNER/REPO SOURCE_COMMIT refs/tags/TAG"
        );
    }
    let policy = verification_args(&args[2], &args[3], &args[4])?;
    let binary = std::fs::canonicalize(&args[0]).context("resolve verification binary")?;
    let bundle = std::fs::canonicalize(&args[1]).context("resolve verification bundle")?;
    let mut child = Command::new("gh")
        .args(["attestation", "verify"])
        .arg(&binary)
        .arg("--bundle")
        .arg(&bundle)
        .args(policy)
        .stdin(Stdio::null())
        .env("GH_PROMPT_DISABLED", "1")
        .spawn()
        .context("start required gh attestation verifier")?;
    wait_verifier(&mut child, Duration::from_secs(120))?;
    println!(
        "Attestation verified by gh for the specified repository, release workflow, commit and tag; other release gates remain independent."
    );
    Ok(())
}

/// Decode a supported Sigstore bundle without authenticating its contents.
/// Retain the original bundle separately; the extracted statement alone loses its signature.
pub(crate) fn decode_bundle(bytes: &[u8]) -> Result<Vec<u8>> {
    use base64::Engine;
    if bytes.len() > 1024 * 1024 {
        bail!("provenance bundle exceeds 1 MiB");
    }
    let bundle: Value = serde_json::from_slice(bytes).context("parse Sigstore bundle")?;
    if !matches!(
        bundle["mediaType"].as_str(),
        Some(
            "application/vnd.dev.sigstore.bundle.v0.3+json"
                | "application/vnd.dev.sigstore.bundle+json;version=0.3"
                | "application/vnd.dev.sigstore.bundle+json;version=0.2"
        )
    ) {
        bail!("unsupported Sigstore bundle media type");
    }
    if !bundle["verificationMaterial"].is_object() || bundle.get("messageSignature").is_some() {
        bail!("expected DSSE bundle with verification material");
    }
    let envelope = &bundle["dsseEnvelope"];
    if envelope["payloadType"] != "application/vnd.in-toto+json" {
        bail!("unsupported DSSE payload type");
    }
    let signatures = envelope["signatures"]
        .as_array()
        .context("missing DSSE signatures")?;
    if signatures.len() != 1 {
        bail!("Sigstore DSSE bundle must have exactly one signature");
    }
    let signature = signatures[0]["sig"]
        .as_str()
        .context("missing DSSE signature bytes")?;
    if base64::engine::general_purpose::STANDARD
        .decode(signature)?
        .is_empty()
    {
        bail!("empty DSSE signature");
    }
    let payload = envelope["payload"]
        .as_str()
        .context("missing DSSE payload")?;
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .context("decode DSSE payload")
}

/// Check a plain in-toto Statement carrying SLSA v1 provenance against binary bytes.
/// Deliberately not a DSSE/Sigstore verifier or a claim of SLSA compliance.
pub(crate) fn check_binary_subject(bytes: &[u8], name: &str, sha256: &str) -> Result<()> {
    if bytes.len() > 1024 * 1024 {
        bail!("provenance exceeds 1 MiB");
    }
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid expected binary SHA-256");
    }
    let statement: Value = serde_json::from_slice(bytes).context("parse provenance statement")?;
    if statement["_type"] != "https://in-toto.io/Statement/v1"
        || statement["predicateType"] != "https://slsa.dev/provenance/v1"
    {
        bail!("expected plain in-toto Statement v1 with SLSA provenance v1");
    }
    for pointer in [
        "/predicate/buildDefinition/buildType",
        "/predicate/runDetails/builder/id",
    ] {
        if statement
            .pointer(pointer)
            .and_then(Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
        {
            bail!("missing nonempty provenance field {pointer}");
        }
    }
    let subjects = statement["subject"]
        .as_array()
        .context("provenance subject must be an array")?;
    let matches: Vec<_> = subjects
        .iter()
        .filter(|subject| subject["name"] == name)
        .collect();
    if matches.len() != 1 {
        bail!("provenance must identify the binary exactly once");
    }
    let digest = matches[0]["digest"]["sha256"]
        .as_str()
        .context("binary subject lacks SHA-256")?;
    if !digest.eq_ignore_ascii_case(sha256) {
        bail!("provenance binary subject SHA-256 mismatch");
    }
    Ok(())
}

pub(crate) fn inspect(args: impl Iterator<Item = String>) -> Result<()> {
    use std::io::Read;
    let args: Vec<_> = args.collect();
    if args.len() != 2 {
        bail!("usage: cargo xtask release provenance-check BINARY STATEMENT");
    }
    let binary = std::path::Path::new(&args[0]);
    let name = binary
        .file_name()
        .and_then(|name| name.to_str())
        .context("binary needs a UTF-8 file name")?;
    let digest = super::sha256_file(binary)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&args[1])?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    check_binary_subject(&bytes, name, &digest)?;
    println!(
        "{}",
        serde_json::json!({
            "binary": binary, "sha256": digest, "subjectBindingChecked": true,
            "signatureVerified": false, "builderTrusted": false, "releaseReady": false
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verifier_process_fixture() {
        match std::env::var("WORKDECK_TEST_VERIFIER_CHILD").as_deref() {
            Ok("fail") => std::process::exit(7),
            Ok("hang") => std::thread::sleep(std::time::Duration::from_secs(30)),
            _ => (),
        }
    }

    #[test]
    fn verifier_child_failure_and_timeout_are_not_success() {
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        for mode in ["success", "fail", "hang"] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "provenance::tests::verifier_process_fixture"])
                .env("WORKDECK_TEST_VERIFIER_CHILD", mode)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let start = Instant::now();
            let timeout = if mode == "hang" {
                Duration::from_millis(150)
            } else {
                Duration::from_secs(10)
            };
            let result = wait_verifier(&mut child, timeout);
            if mode == "success" {
                result.unwrap();
            } else {
                let message = result.unwrap_err().to_string();
                assert!(
                    message.contains(if mode == "hang" { "deadline" } else { "failed" }),
                    "{message}"
                );
            }
            assert!(child.try_wait().unwrap().is_some(), "child must be reaped");
            assert!(start.elapsed() < Duration::from_secs(12));
        }
    }
    #[test]
    fn verification_policy_requires_explicit_release_identity() {
        let digest = "a".repeat(40);
        let args = verification_args("owner/workdeck", &digest, "refs/tags/v1.0.0").unwrap();
        for (flag, expected) in [
            ("--hostname", "github.com"),
            ("--repo", "owner/workdeck"),
            (
                "--signer-workflow",
                "owner/workdeck/.github/workflows/release.yml",
            ),
            ("--source-digest", digest.as_str()),
            ("--source-ref", "refs/tags/v1.0.0"),
            (
                "--cert-oidc-issuer",
                "https://token.actions.githubusercontent.com",
            ),
            ("--predicate-type", "https://slsa.dev/provenance/v1"),
        ] {
            let position = args.iter().position(|arg| arg == flag).unwrap();
            assert_eq!(args[position + 1], expected);
        }
        assert!(args.iter().any(|arg| arg == "--deny-self-hosted-runners"));
        for repo in [
            "owner",
            "owner/repo/extra",
            "/repo",
            "owner/",
            "owner/repo --owner evil",
        ] {
            assert!(verification_args(repo, &digest, "refs/tags/v1").is_err());
        }
        assert!(verification_args("owner/repo", "short", "refs/tags/v1").is_err());
        for reference in ["main", "refs/heads/main", "refs/tags/", "refs/tags/v1\n"] {
            assert!(verification_args("owner/repo", &digest, reference).is_err());
        }
    }
    fn synthetic_statement() -> Value {
        serde_json::json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://slsa.dev/provenance/v1",
            "subject": [{"name": "workdeck", "digest": {"sha256": "a".repeat(64)}}],
            "predicate": {"buildDefinition": {"buildType": "https://example.invalid/test", "externalParameters": {}},
                "runDetails": {"builder": {"id": "https://example.invalid/synthetic-test-only"}}}
        })
    }
    fn check(value: &Value) -> Result<()> {
        check_binary_subject(&serde_json::to_vec(value)?, "workdeck", &"a".repeat(64))
    }
    #[test]
    fn synthetic_subject_binding_is_not_authentication() {
        let mut value = synthetic_statement();
        assert!(check(&value).is_ok());
        value["subject"][0]["digest"]["sha256"] = Value::String("b".repeat(64));
        assert!(check(&value).is_err());
        value = synthetic_statement();
        value["subject"][0]["name"] = "other".into();
        assert!(check(&value).is_err());
        value = synthetic_statement();
        let duplicate = value["subject"][0].clone();
        value["subject"].as_array_mut().unwrap().push(duplicate);
        assert!(check(&value).is_err());
    }
    #[test]
    fn archives_retain_exact_statement_and_checked_binary_snapshot() {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let directory = tempfile::tempdir().unwrap();
        let binary = b"synthetic executable bytes".to_vec();
        let mut value = synthetic_statement();
        value["subject"][0]["digest"]["sha256"] = format!("{:x}", Sha256::digest(&binary)).into();
        let statement = serde_json::to_vec_pretty(&value).unwrap();
        let mut entries = vec![("workdeck-test/workdeck".to_owned(), binary.clone(), 0o755)];
        super::super::attach_release_provenance(
            &mut entries,
            "workdeck-test",
            "workdeck",
            statement.clone(),
        )
        .unwrap();
        assert!(
            super::super::attach_release_provenance(
                &mut entries,
                "workdeck-test",
                "workdeck",
                statement.clone()
            )
            .is_err()
        );
        let tar_path = directory.path().join("test.tar.gz");
        super::super::write_tar_archive(&tar_path, &entries).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(
            std::fs::File::open(tar_path).unwrap(),
        ));
        let mut contents = std::collections::BTreeMap::new();
        for entry in tar.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            contents.insert(name, bytes);
        }
        assert_eq!(contents["workdeck-test/workdeck"], binary);
        assert_eq!(contents["workdeck-test/provenance.json"], statement);
        let zip_path = directory.path().join("test.zip");
        super::super::write_zip_archive(&zip_path, &entries).unwrap();
        let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
        for (name, expected) in contents {
            let mut actual = Vec::new();
            zip.by_name(&name)
                .unwrap()
                .read_to_end(&mut actual)
                .unwrap();
            assert_eq!(actual, expected);
        }
        let mut wrong = vec![(
            "workdeck-test/workdeck".to_owned(),
            b"different binary".to_vec(),
            0o755,
        )];
        let original = wrong.clone();
        assert!(
            super::super::attach_release_provenance(
                &mut wrong,
                "workdeck-test",
                "workdeck",
                statement
            )
            .is_err()
        );
        assert_eq!(wrong, original);
    }

    #[test]
    fn packaging_requires_explicit_provenance_input() {
        let parse = |args: &[&str]| {
            super::super::parse_package_options(args.iter().map(|arg| (*arg).to_owned()))
        };
        assert!(parse(&["--target", "aarch64-apple-darwin"]).is_err());
        assert!(parse(&["--target", "aarch64-apple-darwin", "--provenance"]).is_err());
        let options = parse(&[
            "--target",
            "aarch64-apple-darwin",
            "--provenance",
            "statement.json",
        ])
        .unwrap();
        assert_eq!(
            options.provenance,
            std::path::PathBuf::from("statement.json")
        );
    }
    #[test]
    fn bundle_decode_retains_original_evidence_and_rejects_bad_envelopes() {
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let mut statement = synthetic_statement();
        statement["subject"][0]["digest"]["sha256"] =
            format!("{:x}", Sha256::digest(b"binary")).into();
        let statement = serde_json::to_vec_pretty(&statement).unwrap();
        let bundle = serde_json::json!({
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": {},
            "dsseEnvelope": {
                "payloadType": "application/vnd.in-toto+json",
                "payload": base64::engine::general_purpose::STANDARD.encode(&statement),
                "signatures": [{"sig": "dGVzdA=="}]
            }
        });
        let bytes = serde_json::to_vec_pretty(&bundle).unwrap();
        assert_eq!(decode_bundle(&bytes).unwrap(), statement);
        for name in ["workdeck", "workdeck.exe"] {
            let snapshot = verification_snapshot(b"binary", &bytes, name).unwrap();
            let path = snapshot.path().to_owned();
            assert_eq!(std::fs::read(path.join(name)).unwrap(), b"binary");
            assert_eq!(std::fs::read(path.join("bundle.json")).unwrap(), bytes);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
                    0
                );
            }
            drop(snapshot);
            assert!(!path.exists());
        }
        assert!(verification_snapshot(b"binary", &bytes, "../workdeck").is_err());
        assert!(verification_snapshot(b"binary", b"{}", "workdeck").is_err());
        let mut entries = vec![("root/workdeck".into(), b"binary".to_vec(), 0o755)];
        super::super::attach_release_provenance(&mut entries, "root", "workdeck", bytes.clone())
            .unwrap();
        assert_eq!(
            entries
                .iter()
                .find(|e| e.0 == "root/provenance.json")
                .unwrap()
                .1,
            statement
        );
        assert_eq!(
            entries
                .iter()
                .find(|e| e.0 == "root/provenance.sigstore.json")
                .unwrap()
                .1,
            bytes
        );
        for (pointer, replacement) in [
            ("/mediaType", Value::String("unknown".into())),
            ("/verificationMaterial", Value::Null),
            (
                "/dsseEnvelope/payloadType",
                Value::String("text/plain".into()),
            ),
            ("/dsseEnvelope/payload", Value::String("!bad".into())),
            ("/dsseEnvelope/signatures", serde_json::json!([])),
            (
                "/dsseEnvelope/signatures",
                serde_json::json!([{"sig":"dGVzdA=="},{"sig":"dGVzdA=="}]),
            ),
            (
                "/dsseEnvelope/signatures/0/sig",
                Value::String(String::new()),
            ),
        ] {
            let mut invalid = bundle.clone();
            *invalid.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                decode_bundle(&serde_json::to_vec(&invalid).unwrap()).is_err(),
                "{pointer}"
            );
        }
        assert!(decode_bundle(&vec![b' '; 1024 * 1024 + 1]).is_err());
    }
    #[test]
    fn rejects_wrong_envelopes_missing_identity_and_oversized_inputs() {
        for pointer in [
            "/_type",
            "/predicateType",
            "/predicate/buildDefinition/buildType",
            "/predicate/runDetails/builder/id",
        ] {
            let mut value = synthetic_statement();
            *value.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(check(&value).is_err(), "{pointer}");
        }
        assert!(
            check_binary_subject(&vec![b' '; 1024 * 1024 + 1], "workdeck", &"a".repeat(64))
                .is_err()
        );
        assert!(check_binary_subject(b"{}", "workdeck", "invalid").is_err());
    }
}
