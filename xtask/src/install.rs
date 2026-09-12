//! Tooling entry points delegate to the native product implementation.
#[cfg(test)]
use workdeck_cli::install::{
    expected_checksum, hash_archive_bytes, inspect_archive_entries, open_archive_input,
    read_checksum_manifest, verify_package_paths,
};
pub(super) use workdeck_cli::install::{
    inspect, inspect_package_archive, run, stage, verify, verify_checksum_manifest,
};

/// Verify the native archive-installation workflow replacing Hunk's POSIX
/// install.sh E2E. The source workflow is read from Git; its download, PATH,
/// idempotency, custom-directory, and failure cases are covered by native
/// transaction/oracle tests without retaining a shell runtime.
pub(crate) fn verify_workflow(repo: &std::path::Path) -> anyhow::Result<()> {
    const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:.github/workflows/install-sh-e2e.yml"),
        ],
    )?;
    anyhow::ensure!(
        source.len() == 3_914,
        "pinned install-sh-e2e workflow changed size: {} != 3914",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "name: Install script E2E",
        "pull_request:",
        "install.sh",
        "schedule:",
        "workflow_dispatch",
        "permissions:",
        "contents: read",
        "install-e2e:",
        "ubuntu-latest",
        "macos-latest",
        "Resolve the newest published release",
        "gh release view",
        "HUNK_VERSION",
        "Install into a sandbox home",
        "GITHUB_PATH",
        "skill path hunk-review",
        "Re-run is idempotent",
        "Custom directory with spaces installs and resolves skills",
        "Unknown version fails loudly",
        "Could not download",
    ] {
        anyhow::ensure!(
            source.contains(marker),
            "pinned install-sh-e2e workflow lost marker {marker:?}"
        );
    }
    let native = std::fs::read_to_string(repo.join(".github/workflows/install-sh-e2e.yml"))?;
    for marker in [
        "name: Install archive E2E",
        "crates/workdeck-cli/src/install/**",
        "Native installation E2E",
        "ubuntu-latest",
        "macos-latest",
        "windows-latest",
        "cargo test --locked -p workdeck-cli install::",
        "cargo test --locked -p xtask install_oracle",
        "cargo xtask install-plan --no-modify-path",
        "cargo build --locked --release --package workdeck-cli --bin workdeck",
        "target/release/workdeck --version",
    ] {
        anyhow::ensure!(
            native.contains(marker),
            "native install workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm", "install.sh"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        anyhow::ensure!(
            !pattern.is_match(&native),
            "native install workflow retains forbidden token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/install-workflow-migration.md"))?;
    for marker in [
        "install-sh-e2e.yml",
        "native archive",
        "idempotency",
        "custom directory",
        "read-only installation plan",
        "no shell runtime",
    ] {
        anyhow::ensure!(
            migration.contains(marker),
            "install workflow migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Verify the manual-only replacement for Hunk's privileged Firecracker
/// installer workflow. Scenario selection and always-uploaded structured
/// oracle results remain, but execution is native Rust and has no Bun setup.
pub(crate) fn verify_vm_workflow(repo: &std::path::Path) -> anyhow::Result<()> {
    const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:.github/workflows/install-vm.yml"),
        ],
    )?;
    anyhow::ensure!(
        source.len() == 1_892,
        "pinned install-vm workflow changed size: {} != 1892",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "repository-controlled KVM code",
        "Never add pull_request",
        "name: Optional install VM suite",
        "workflow_dispatch",
        "scenario:",
        "required: false",
        "permissions:",
        "contents: read",
        "install-vm:",
        "Firecracker install compatibility",
        "timeout-minutes: 45",
        "Set up Bun",
        "bun install --frozen-lockfile",
        "INSTALL_VM_SCENARIO",
        "--allow-skip",
        "--scenario",
        "Upload structured install results",
        "tmp/install-vm/runs",
    ] {
        anyhow::ensure!(
            source.contains(marker),
            "pinned install-vm workflow lost marker {marker:?}"
        );
    }
    let native = std::fs::read_to_string(repo.join(".github/workflows/install-vm.yml"))?;
    for marker in [
        "Manual-only native installation compatibility",
        "workflow_dispatch",
        "scenario:",
        "permissions:",
        "contents: read",
        "install-vm:",
        "Native install compatibility",
        "timeout-minutes: 45",
        "INSTALL_VM_SCENARIO",
        "cargo test --locked -p workdeck-cli install::",
        "cargo test --locked -p xtask install_oracle -- --nocapture",
        "if: always()",
    ] {
        anyhow::ensure!(
            native.contains(marker),
            "native install-vm workflow is missing {marker:?}"
        );
    }
    anyhow::ensure!(
        !native.contains("pull_request")
            && !native.contains("setup-bun")
            && !native.contains("bun install")
            && !native.contains("node")
            && !native.contains("npm")
            && !native.contains("Firecracker"),
        "native install-vm workflow retained privileged or legacy runtime behavior"
    );
    let migration = std::fs::read_to_string(repo.join("docs/install-vm-workflow-migration.md"))?;
    for marker in [
        "install-vm.yml",
        "manual-only",
        "scenario",
        "structured",
        "native Rust",
        "Firecracker",
    ] {
        anyhow::ensure!(
            migration.contains(marker),
            "install-vm workflow migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Explicit first-install command; unlike inspection/staging this publishes a root.
pub(super) fn create(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let args: Vec<_> = args.collect();
    anyhow::ensure!(
        args.len() == 6,
        "install-create requires ARCHIVE CHECKSUM_FILE DESTINATION TARGET COMMIT TAG_REF"
    );
    let [archive, checksums, destination, target, commit, tag] = args.as_slice() else {
        unreachable!("argument count checked")
    };
    workdeck_cli::install::create_authenticated_installation(
        std::path::Path::new(archive),
        std::path::Path::new(checksums),
        std::path::Path::new(destination),
        target,
        workdeck_cli::install::ReleaseIdentity {
            repository: "ruttydm/workdeck",
            commit,
            tag_ref: tag,
        },
    )?;
    println!(
        "{}",
        serde_json::json!({
            "installed": true,
            "installationRoot": std::path::absolute(destination)?,
            "target": target,
            "sourceCommit": commit,
            "tagRef": tag,
            "archiveSignatureVerified": true,
            "pathModified": false,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_install_workflow_replaces_the_complete_pinned_shell_e2e() {
        let repo = super::super::repo_root().unwrap();
        super::verify_workflow(&repo).unwrap();
    }

    #[test]
    fn native_vm_install_workflow_replaces_the_complete_privileged_suite() {
        let repo = super::super::repo_root().unwrap();
        super::verify_vm_workflow(&repo).unwrap();
    }

    #[test]
    fn release_package_archives_pass_structural_checksum_and_provenance_inspection() {
        use super::*;
        use sha2::{Digest, Sha256};
        use std::{fs, io::Read};

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("source");
        fs::create_dir_all(root.join("third_party/themes")).unwrap();
        fs::create_dir_all(root.join("third_party/grammars")).unwrap();
        fs::write(root.join("LICENSE"), b"license\n").unwrap();
        fs::write(root.join("THIRD_PARTY_NOTICES"), b"notices\n").unwrap();
        for notice in [
            "tm-themes-LICENSE",
            "tm-themes-NOTICE",
            "pierre-theme-LICENSE",
            "pierre-theme-NOTICE.md",
        ] {
            fs::write(root.join("third_party/themes").join(notice), notice).unwrap();
        }
        fs::write(
            root.join("third_party/grammars/shikijs-langs-LICENSE"),
            b"shiki license\n",
        )
        .unwrap();
        for skill in [
            "workdeck-review",
            "workdeck-extensions",
            "workdeck-release",
            "workdeck-launch-video",
        ] {
            fs::create_dir_all(root.join("skills").join(skill)).unwrap();
            fs::write(
                root.join("skills").join(skill).join("SKILL.md"),
                format!("name: {skill}\n"),
            )
            .unwrap();
        }

        let binary = root.join("workdeck");
        let binary_bytes = b"synthetic release executable\n".to_vec();
        fs::write(&binary, &binary_bytes).unwrap();
        let binary_sha256 = format!("{:x}", Sha256::digest(&binary_bytes));
        let provenance = serde_json::to_vec_pretty(&serde_json::json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://slsa.dev/provenance/v1",
            "subject": [{"name": "workdeck", "digest": {"sha256": binary_sha256}}],
            "predicate": {
                "buildDefinition": {
                    "buildType": "https://example.invalid/workdeck-test",
                    "externalParameters": {}
                },
                "runDetails": {
                    "builder": {"id": "https://example.invalid/synthetic-builder"}
                }
            }
        }))
        .unwrap();
        let mut entries = super::super::release_entries(
            "workdeck-aarch64-apple-darwin",
            &binary,
            "workdeck",
            &root,
            br#"{"schema_version":1}"#,
            br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        )
        .unwrap();
        super::super::attach_release_provenance(
            &mut entries,
            "workdeck-aarch64-apple-darwin",
            "workdeck",
            provenance.clone(),
        )
        .unwrap();

        let archive_specs = [("tar.gz", false), ("zip", true)];
        for (extension, zip) in archive_specs {
            let archive = directory
                .path()
                .join(format!("workdeck-aarch64-apple-darwin.{extension}"));
            if zip {
                super::super::write_zip_archive(&archive, &entries).unwrap();
            } else {
                super::super::write_tar_archive(&archive, &entries).unwrap();
            }

            let (names, declared_bytes) = inspect_archive_entries(&archive).unwrap();
            verify_package_paths(&names).unwrap();
            assert!(declared_bytes >= binary_bytes.len() as u64);
            let (checked_entries, checked_bytes) = inspect_package_archive(&archive).unwrap();
            assert_eq!(checked_entries, names.len());
            assert_eq!(checked_bytes, declared_bytes);

            let archive_bytes = fs::metadata(&archive).unwrap().len();
            let archive_sha256 =
                hash_archive_bytes(open_archive_input(&archive).unwrap(), archive_bytes).unwrap();
            let checksum_path = directory
                .path()
                .join(format!("workdeck-aarch64-apple-darwin.{extension}.sha256"));
            fs::write(
                &checksum_path,
                format!(
                    "{archive_sha256}  {}\n",
                    archive.file_name().unwrap().to_string_lossy()
                ),
            )
            .unwrap();
            assert_eq!(
                expected_checksum(
                    &read_checksum_manifest(&checksum_path).unwrap(),
                    archive.file_name().unwrap().to_str().unwrap(),
                )
                .unwrap(),
                archive_sha256,
            );
            assert_eq!(
                verify_checksum_manifest(&archive, &checksum_path).unwrap(),
                archive_sha256
            );

            let mut extracted_provenance = None;
            let mut extracted_binary = None;
            if zip {
                let mut archive_reader =
                    zip::ZipArchive::new(fs::File::open(&archive).unwrap()).unwrap();
                for name in [
                    "workdeck-aarch64-apple-darwin/workdeck",
                    "workdeck-aarch64-apple-darwin/provenance.json",
                ] {
                    let mut bytes = Vec::new();
                    archive_reader
                        .by_name(name)
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .unwrap();
                    if name.ends_with("/workdeck") {
                        extracted_binary = Some(bytes);
                    } else {
                        extracted_provenance = Some(bytes);
                    }
                }
            } else {
                let file = fs::File::open(&archive).unwrap();
                let decoder = flate2::read::GzDecoder::new(file);
                let mut archive_reader = tar::Archive::new(decoder);
                for item in archive_reader.entries().unwrap() {
                    let mut item = item.unwrap();
                    let name = item.path().unwrap().to_string_lossy().into_owned();
                    if name.ends_with("/workdeck") || name.ends_with("/provenance.json") {
                        let mut bytes = Vec::new();
                        item.read_to_end(&mut bytes).unwrap();
                        if name.ends_with("/workdeck") {
                            extracted_binary = Some(bytes);
                        } else {
                            extracted_provenance = Some(bytes);
                        }
                    }
                }
            }
            assert_eq!(extracted_binary.as_deref(), Some(binary_bytes.as_slice()));
            assert_eq!(extracted_provenance.as_deref(), Some(provenance.as_slice()));
            super::super::provenance::check_binary_subject(
                extracted_provenance.as_deref().unwrap(),
                "workdeck",
                &binary_sha256,
            )
            .unwrap();
        }
    }
}
