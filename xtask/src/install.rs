//! Tooling entry points delegate to the native product implementation.
pub(super) use workdeck_cli::install::{inspect, run, stage, verify};

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
