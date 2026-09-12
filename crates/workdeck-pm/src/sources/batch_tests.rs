use super::*;
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(|name| name.starts_with("GIT_")) {
            command.env_remove(name);
        }
    }
    let output = command
        .current_dir(root)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn source_capture_batches_git_objects_instead_of_spawning_per_document() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    let repo = crate::Repository::init(temp.path(), "WD").unwrap();
    fs::create_dir_all(repo.root().join("wiki")).unwrap();
    for i in 0..64 {
        fs::write(
            repo.root().join(format!("wiki/page-{i}.md")),
            format!("Inert document {i}\n"),
        )
        .unwrap();
    }
    git(temp.path(), &["add", "--", ".workdeck"]);
    let index = fs::read(temp.path().join(".git/index")).unwrap();
    process::take_invocations();
    let view = capture(
        temp.path(),
        &SourceSelector::Staged {
            index: IndexSelection::Default,
        },
        &SourceCaptureLimits::default(),
    )
    .unwrap();
    let count = process::take_invocations();
    eprintln!("65-document capture used {count} Git processes");
    assert_eq!(view.snapshot.files().len(), 65);
    assert!(
        count <= 40,
        "capturing 65 documents spawned {count} Git processes"
    );
    assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
}

#[test]
fn real_git_batch_preserves_per_file_and_complete_membership_byte_limits() {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    fs::write(temp.path().join("blob"), b"a\0b\nc").unwrap();
    let oid: GitOid = String::from_utf8(git(temp.path(), &["hash-object", "-w", "blob"]))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let limits = SourceCaptureLimits {
        max_file_bytes: 5,
        max_total_bytes: 10,
        ..Default::default()
    };
    let reader = git::BoundGit::with_limits(temp.path(), &limits).unwrap();
    assert_eq!(
        reader.blobs(&[oid.clone(), oid.clone()]).unwrap(),
        vec![b"a\0b\nc".to_vec(); 2]
    );
    let too_small = git::BoundGit::with_limits(
        temp.path(),
        &SourceCaptureLimits {
            max_file_bytes: 4,
            ..limits.clone()
        },
    )
    .unwrap();
    assert_eq!(
        too_small
            .blobs(std::slice::from_ref(&oid))
            .unwrap_err()
            .code,
        crate::ErrorCode::InvalidInput
    );
    let too_total = git::BoundGit::with_limits(
        temp.path(),
        &SourceCaptureLimits {
            max_total_bytes: 9,
            ..limits
        },
    )
    .unwrap();
    assert_eq!(
        too_total.blobs(&[oid.clone(), oid]).unwrap_err().code,
        crate::ErrorCode::InvalidInput
    );
}

fn inventory(root: &Path) -> std::collections::BTreeMap<std::path::PathBuf, crate::ContentHash> {
    fn visit(
        root: &Path,
        path: &Path,
        files: &mut std::collections::BTreeMap<std::path::PathBuf, crate::ContentHash>,
    ) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    crate::ContentHash::of(&fs::read(entry.path()).unwrap()),
                );
            }
        }
    }
    let mut files = std::collections::BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn partial_clone_read_does_not_contact_promisor_or_hydrate_objects() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let seed = temp.path().join("seed");
    let remote = temp.path().join("remote.git");
    let partial = temp.path().join("partial");
    fs::create_dir(&seed).unwrap();
    git(&seed, &["init", "-b", "main"]);
    git(&seed, &["config", "user.name", "Fixture"]);
    git(&seed, &["config", "user.email", "fixture@example.invalid"]);
    let repo = crate::Repository::init(&seed, "WD").unwrap();
    let mut config = repo.config().unwrap();
    config.sources = Some(SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/proposals".parse().unwrap(),
    });
    let config = serde_yaml_ng::to_string(&config).unwrap();
    fs::write(repo.root().join("config.yml"), &config).unwrap();
    git(&seed, &["add", "--", ".workdeck"]);
    git(&seed, &["commit", "-m", "fixture"]);
    let oid = String::from_utf8(git(&seed, &["rev-parse", "HEAD:.workdeck/config.yml"])).unwrap();
    git(
        temp.path(),
        &[
            "clone",
            "--bare",
            seed.to_str().unwrap(),
            remote.to_str().unwrap(),
        ],
    );
    git(&remote, &["config", "uploadpack.allowFilter", "true"]);
    git(
        &remote,
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    );
    let url = format!("file://{}", remote.canonicalize().unwrap().display());
    git(
        temp.path(),
        &[
            "clone",
            "--filter=blob:none",
            "--no-checkout",
            &url,
            partial.to_str().unwrap(),
        ],
    );
    // The working planning config is separately available; accepted blobs are absent.
    fs::create_dir(partial.join(".workdeck")).unwrap();
    fs::write(partial.join(".workdeck/config.yml"), config).unwrap();
    git(&partial, &["config", "protocol.file.allow", "always"]);
    let marker = temp.path().join("promisor-contacted");
    let helper = temp.path().join("upload-pack");
    fs::write(
        &helper,
        format!(
            "#!/bin/sh\nprintf contacted > '{}'\nexec git-upload-pack \"$@\"\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &partial,
        &[
            "config",
            "remote.origin.uploadpack",
            helper.to_str().unwrap(),
        ],
    );
    let missing = Command::new("git")
        .current_dir(&partial)
        .args(["--no-lazy-fetch", "cat-file", "-e", oid.trim()])
        .output()
        .unwrap();
    assert!(
        !missing.status.success(),
        "fixture unexpectedly already contains promised blob"
    );
    let before = inventory(&partial.join(".git/objects"));
    let result = capture(
        &partial,
        &SourceSelector::Accepted,
        &SourceCaptureLimits::default(),
    );
    let after = inventory(&partial.join(".git/objects"));
    assert!(
        result.is_err(),
        "ordinary accepted-source read hydrated missing objects instead of failing closed"
    );
    assert!(
        !marker.exists(),
        "ordinary source read contacted its promisor remote"
    );
    assert_eq!(
        after, before,
        "ordinary source read changed the object store"
    );

    // Explicit transport remains functional under the same no-lazy-fetch guard.
    let bound = git::BoundGit::open_shared(&partial).unwrap();
    let result = bound
        .run(
            &[
                "fetch".into(),
                "--no-write-fetch-head".into(),
                "origin".into(),
                oid.trim().into(),
            ],
            None,
            None,
            64 * 1024,
        )
        .unwrap();
    assert!(result.status.success(), "explicit fetch failed");
    assert!(
        marker.exists(),
        "fixture did not observe explicit remote transport"
    );
    let source = capture(
        &partial,
        &SourceSelector::Accepted,
        &SourceCaptureLimits::default(),
    )
    .unwrap();
    assert!(
        source
            .snapshot
            .files()
            .contains_key(Path::new("config.yml"))
    );
}
