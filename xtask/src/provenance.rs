//! Structural/subject checks only: these do not authenticate a builder or signature.
use anyhow::{Context, Result, bail};
use serde_json::Value;

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
