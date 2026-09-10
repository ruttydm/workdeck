use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn run(directory: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(directory)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn staging_cli_retains_verified_files_without_installing_or_creating_repo_state() {
    let input = tempfile::tempdir().unwrap();
    let archive = input.path().join("workdeck.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
    for name in [
        "workdeck",
        "LICENSE",
        "THIRD_PARTY_NOTICES",
        "licenses.json",
        "sbom.cdx.json",
        "provenance.json",
    ] {
        zip.start_file(
            format!("package/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(name.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    let archive_bytes = std::fs::read(&archive).unwrap();
    let checksum = format!("{:x} workdeck.zip\n", Sha256::digest(&archive_bytes));
    std::fs::write(input.path().join("SHA256SUMS"), &checksum).unwrap();
    let args = ["install-stage", "workdeck.zip", "SHA256SUMS"];
    let output = run(input.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["checksumVerified"], true);
    assert_eq!(report["signatureVerified"], false);
    assert_eq!(report["installed"], false);
    let staged = Path::new(report["stagingDirectory"].as_str().unwrap())
        .canonicalize()
        .unwrap();
    // Only remove the exact temporary directory produced by this invocation.
    assert_eq!(
        staged.parent().unwrap(),
        std::env::temp_dir().canonicalize().unwrap()
    );
    assert!(
        staged
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("workdeck-install-")
    );
    assert_eq!(
        std::fs::read(staged.join("package/workdeck")).unwrap(),
        b"workdeck"
    );
    assert_eq!(
        std::fs::read_dir(staged.join("package")).unwrap().count(),
        6
    );
    std::fs::remove_dir_all(&staged).unwrap();
    assert!(!staged.exists());
    std::fs::write(
        input.path().join("SHA256SUMS"),
        format!("{} workdeck.zip\n", "0".repeat(64)),
    )
    .unwrap();
    let failed = run(input.path(), &args);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("Checksum verification failed"));
    for invalid in [
        vec!["install-stage"],
        vec!["install-stage", "workdeck.zip"],
        vec!["install-stage", "workdeck.zip", "SHA256SUMS", "extra"],
    ] {
        let output = run(input.path(), &invalid);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    assert_eq!(std::fs::read(&archive).unwrap(), archive_bytes);
    assert_eq!(std::fs::read_dir(input.path()).unwrap().count(), 2);
    assert!(!input.path().join(".agents").exists());
}
