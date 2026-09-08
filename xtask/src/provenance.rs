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
