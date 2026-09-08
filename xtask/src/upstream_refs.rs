//! Append-only Hunk ref archives, independent of mutable/prunable tracking refs.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::git_stdout;

const ARCHIVE: &str = "refs/upstream/hunk/archive/";
const RECEIPTS: &str = "port/hunk/upstream-refs.jsonl";
const SOURCES: [(&str, &str); 3] = [
    ("refs/remotes/hunk-upstream/", "heads"),
    ("refs/tags/hunk-upstream/", "tags"),
    ("refs/tags/hunk-port/", "anchors"),
];

pub struct ArchiveLock(File);

impl Drop for ArchiveLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Serialize cooperating archive/fetch commands across linked worktrees.
pub fn lock(repo: &Path) -> Result<ArchiveLock> {
    let common = git_stdout(repo, ["rev-parse", "--git-common-dir"])?;
    let path = repo.join(common).join("workdeck-hunk-archive.lock");
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.try_lock()
        .context("another Hunk archive/fetch command is running")?;
    Ok(ArchiveLock(file))
}

fn refs(repo: &Path, prefix: &str) -> Result<BTreeMap<String, String>> {
    let output = git_stdout(
        repo,
        [
            "for-each-ref",
            "--format=%(refname)%09%(objectname)%09%(symref)",
            prefix,
        ],
    )?;
    let mut refs = BTreeMap::new();
    for line in output.lines() {
        let mut fields = line.split('\t');
        let name = fields.next().context("missing ref name")?;
        let object = fields.next().context("missing ref object")?;
        // Symbolic remote HEAD is an alias, not an additional branch or tag.
        if fields.next().is_some_and(|symbolic| !symbolic.is_empty()) {
            if prefix == ARCHIVE {
                bail!("Hunk archive ref must not be symbolic: {name}");
            }
            continue;
        }
        refs.insert(name.into(), object.into());
    }
    Ok(refs)
}

fn archive_name(kind: &str, name: &str, object: &str) -> String {
    // One reversible component avoids file/directory collisions if an upstream
    // branch is deleted and a later branch reuses its name as a directory prefix.
    let encoded = name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{ARCHIVE}{kind}/{encoded}/{object}")
}

fn current(repo: &Path) -> Result<BTreeMap<String, String>> {
    let mut current = BTreeMap::new();
    for (prefix, kind) in SOURCES {
        for (name, object) in refs(repo, prefix)? {
            let suffix = name
                .strip_prefix(prefix)
                .context("unexpected source ref namespace")?;
            current.insert(archive_name(kind, suffix, &object), object);
        }
    }
    Ok(current)
}

fn validated_archive(repo: &Path) -> Result<BTreeMap<String, String>> {
    let archived = refs(repo, ARCHIVE)?;
    for (name, object) in &archived {
        let parts = name
            .strip_prefix(ARCHIVE)
            .unwrap()
            .split('/')
            .collect::<Vec<_>>();
        if parts.len() != 3
            || !matches!(parts[0], "heads" | "tags" | "anchors")
            || parts[1].is_empty()
            || parts[1].len() % 2 != 0
            || !parts[1]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || parts[2] != object
        {
            bail!("Hunk archive ref was changed or is malformed: {name}");
        }
        source_ref(name)?;
    }
    Ok(archived)
}

/// Retain exact branch tips and tag objects. Never update or delete an archive ref.
/// The caller holds `lock` across this operation and any subsequent fetch.
pub fn preserve(repo: &Path) -> Result<usize> {
    let archived = validated_archive(repo)?;
    check_receipts(repo, &archived, false)?;
    let current = current(repo)?;
    let missing = current
        .iter()
        .filter(|(name, _)| !archived.contains_key(*name))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        write_receipts(repo, &archived)?;
        return Ok(0);
    }
    let mut transaction = String::from("start\n");
    for (name, object) in &archived {
        transaction.push_str(&format!("verify {name} {object}\n"));
    }
    for (name, object) in &missing {
        transaction.push_str(&format!("create {name} {object}\n"));
    }
    transaction.push_str("prepare\ncommit\n");
    let mut child = Command::new("git")
        .args(["update-ref", "--stdin"])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start atomic Hunk ref archive transaction")?;
    let written = child
        .stdin
        .take()
        .context("archive transaction stdin")?
        .write_all(transaction.as_bytes());
    let output = child.wait_with_output()?;
    written?;
    if !output.status.success() {
        bail!(
            "Hunk ref archive transaction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    write_receipts(repo, &validated_archive(repo)?)?;
    audit(repo)?;
    Ok(missing.len())
}

/// Read-only integrity and coverage gate; never repairs missing or altered refs.
pub fn audit(repo: &Path) -> Result<usize> {
    let archived = validated_archive(repo)?;
    check_receipts(repo, &archived, true)?;
    for (name, object) in current(repo)? {
        if archived.get(&name) != Some(&object) {
            bail!(
                "Hunk tracking ref is not archived: {name}; run cargo xtask port preserve-upstream"
            );
        }
    }
    Ok(archived.len())
}

/// Caller holds the shared archive lock and has validated the remote identity.
pub fn fetch(repo: &Path) -> Result<()> {
    preserve(repo)?;
    // Git's atomic ref transaction cannot prune `topic` and create `topic/child`
    // in one update. Preserve history independently, and archive partial fetch
    // results too, so a failed fetch never leaves newly observed tips unrecorded.
    let fetched = crate::run_checked(
        repo,
        "git",
        &[
            "fetch",
            "--no-tags",
            "--prune",
            "hunk-upstream",
            "+refs/heads/*:refs/remotes/hunk-upstream/*",
            "+refs/tags/*:refs/tags/hunk-upstream/*",
        ],
    );
    preserve(repo).context("archive Hunk refs after fetch attempt")?;
    fetched
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    archive_ref: String,
    source_ref: String,
    object: String,
}

fn source_ref(archive_ref: &str) -> Result<String> {
    let mut parts = archive_ref
        .strip_prefix(ARCHIVE)
        .context("archive namespace")?
        .split('/');
    let kind = parts.next().context("archive kind")?;
    let encoded = parts.next().context("archive source name")?;
    let prefix = SOURCES
        .iter()
        .find(|(_, candidate)| *candidate == kind)
        .map(|(prefix, _)| *prefix)
        .context("unknown archive kind")?;
    let bytes = (0..encoded.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&encoded[offset..offset + 2], 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(format!("{prefix}{}", String::from_utf8(bytes)?))
}

fn check_receipts(repo: &Path, archived: &BTreeMap<String, String>, exact: bool) -> Result<()> {
    let path = repo.join(RECEIPTS);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !exact => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let mut seen = std::collections::BTreeSet::new();
    for line in raw.lines() {
        let receipt: Receipt = serde_json::from_str(line).context("parse Hunk archive receipt")?;
        if !seen.insert(receipt.archive_ref.clone())
            || archived.get(&receipt.archive_ref) != Some(&receipt.object)
            || receipt.source_ref != source_ref(&receipt.archive_ref)?
        {
            bail!(
                "Hunk archive receipt is duplicated, missing, or changed: {}",
                receipt.archive_ref
            );
        }
    }
    if exact && seen.len() != archived.len() {
        bail!("Hunk archive has unrecorded refs; run cargo xtask port preserve-upstream");
    }
    Ok(())
}

fn write_receipts(repo: &Path, archived: &BTreeMap<String, String>) -> Result<()> {
    let path = repo.join(RECEIPTS);
    let parent = path.parent().context("archive receipt directory")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    for (archive_ref, object) in archived {
        serde_json::to_writer(
            &mut file,
            &Receipt {
                archive_ref: archive_ref.clone(),
                source_ref: source_ref(archive_ref)?,
                object: object.clone(),
            },
        )?;
        file.write_all(b"\n")?;
    }
    file.as_file().sync_all()?;
    file.persist(path)
        .context("atomically save Hunk archive receipts")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=Archive Test",
                "-c",
                "user.email=archive@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }

    fn repository() -> (tempfile::TempDir, String, String) {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        git(repo, &["init", "--quiet"]);
        let tree = git(repo, &["mktree"]);
        let first = git(repo, &["commit-tree", &tree, "-m", "first"]);
        let second = git(repo, &["commit-tree", &tree, "-p", &first, "-m", "second"]);
        (temp, first, second)
    }

    #[test]
    fn retains_deleted_and_rewritten_heads_and_exact_annotated_tag_objects() {
        let (temp, first, second) = repository();
        let repo = temp.path();
        let head = "refs/remotes/hunk-upstream/topic";
        git(repo, &["update-ref", head, &first]);
        git(
            repo,
            &[
                "tag",
                "-a",
                "hunk-upstream/v1",
                &first,
                "-m",
                "annotated tag",
            ],
        );
        let tag = git(repo, &["rev-parse", "refs/tags/hunk-upstream/v1"]);
        assert_ne!(tag, first);
        assert!(audit(repo).is_err());
        assert!(!repo.join("port").exists(), "audit must remain read-only");
        assert_eq!(preserve(repo).unwrap(), 2);
        assert_eq!(preserve(repo).unwrap(), 0);
        git(repo, &["update-ref", head, &second]);
        assert!(audit(repo).is_err());
        assert_eq!(preserve(repo).unwrap(), 1);
        git(repo, &["update-ref", "-d", head]);
        git(repo, &["update-ref", "-d", "refs/tags/hunk-upstream/v1"]);
        // A deleted branch name may become the prefix of a new branch.
        git(
            repo,
            &[
                "update-ref",
                "refs/remotes/hunk-upstream/topic/child",
                &second,
            ],
        );
        assert_eq!(preserve(repo).unwrap(), 1);
        assert_eq!(audit(repo).unwrap(), 4);
        for (kind, name, object) in [
            ("heads", "topic", &first),
            ("heads", "topic", &second),
            ("tags", "v1", &tag),
        ] {
            assert_eq!(
                git(repo, &["rev-parse", &archive_name(kind, name, object)]),
                *object
            );
        }
    }

    #[test]
    fn rejects_changed_archive_instead_of_repairing_or_fetching_over_it() {
        let (temp, first, second) = repository();
        let repo = temp.path();
        git(
            repo,
            &["update-ref", "refs/remotes/hunk-upstream/main", &first],
        );
        preserve(repo).unwrap();
        let archived = archive_name("heads", "main", &first);
        git(repo, &["update-ref", &archived, &second]);
        assert!(audit(repo).is_err());
        assert!(preserve(repo).is_err());
        assert_eq!(git(repo, &["rev-parse", &archived]), second);
    }

    #[test]
    fn receipt_detects_deleted_historical_ref_after_upstream_branch_disappears() {
        let (temp, first, _) = repository();
        let repo = temp.path();
        let head = "refs/remotes/hunk-upstream/retired";
        git(repo, &["update-ref", head, &first]);
        preserve(repo).unwrap();
        git(repo, &["update-ref", "-d", head]);
        assert_eq!(audit(repo).unwrap(), 1);
        git(
            repo,
            &[
                "update-ref",
                "-d",
                &archive_name("heads", "retired", &first),
            ],
        );
        assert!(audit(repo).is_err());
        assert!(preserve(repo).is_err());
    }

    #[test]
    fn rejects_symbolic_archives_even_when_their_target_object_matches() {
        let (temp, first, _) = repository();
        let repo = temp.path();
        let head = "refs/remotes/hunk-upstream/main";
        git(repo, &["update-ref", head, &first]);
        preserve(repo).unwrap();
        let archived = archive_name("heads", "main", &first);
        git(repo, &["symbolic-ref", &archived, head]);
        assert!(audit(repo).is_err());
        assert!(preserve(repo).is_err());
    }

    #[test]
    fn serializes_archive_writers_and_preserves_anchors_without_remote_head_aliases() {
        let (temp, first, _) = repository();
        let repo = temp.path();
        let guard = lock(repo).unwrap();
        assert!(lock(repo).is_err());
        drop(guard);
        let _guard = lock(repo).unwrap();
        git(
            repo,
            &["update-ref", "refs/remotes/hunk-upstream/main", &first],
        );
        git(
            repo,
            &[
                "symbolic-ref",
                "refs/remotes/hunk-upstream/HEAD",
                "refs/remotes/hunk-upstream/main",
            ],
        );
        git(
            repo,
            &["update-ref", "refs/tags/hunk-port/main-pinned", &first],
        );
        assert_eq!(preserve(repo).unwrap(), 2);
        assert_eq!(audit(repo).unwrap(), 2);
    }

    #[test]
    fn failed_fetch_still_archives_successfully_updated_tracking_refs() {
        let (source, first, second) = repository();
        let destination = tempfile::tempdir().unwrap();
        let repo = destination.path();
        git(repo, &["init", "--quiet"]);
        git(
            repo,
            &[
                "remote",
                "add",
                "hunk-upstream",
                source.path().to_str().unwrap(),
            ],
        );
        git(source.path(), &["update-ref", "refs/heads/main", &first]);
        let _guard = lock(repo).unwrap();
        fetch(repo).unwrap();
        git(
            source.path(),
            &["update-ref", "refs/heads/blocked", &second],
        );
        git(source.path(), &["update-ref", "refs/heads/good", &second]);
        let lock_path = repo.join(".git/refs/remotes/hunk-upstream/blocked.lock");
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        std::fs::write(lock_path, b"test-owned ref lock").unwrap();
        assert!(fetch(repo).is_err());
        assert_eq!(
            git(repo, &["rev-parse", "refs/remotes/hunk-upstream/good"]),
            second
        );
        assert_eq!(audit(repo).unwrap(), 2);
        assert_eq!(
            git(
                repo,
                &["rev-parse", &archive_name("heads", "good", &second)]
            ),
            second
        );
    }

    #[test]
    fn actual_pruning_fetch_retains_force_pushed_and_deleted_branch_history() {
        let (source, first, _) = repository();
        let destination = tempfile::tempdir().unwrap();
        let repo = destination.path();
        git(repo, &["init", "--quiet"]);
        git(
            repo,
            &[
                "remote",
                "add",
                "hunk-upstream",
                source.path().to_str().unwrap(),
            ],
        );
        git(source.path(), &["update-ref", "refs/heads/topic", &first]);
        let _guard = lock(repo).unwrap();
        fetch(repo).unwrap();
        let tree = git(source.path(), &["mktree"]);
        let replacement = git(
            source.path(),
            &["commit-tree", &tree, "-m", "unrelated replacement"],
        );
        git(
            source.path(),
            &["update-ref", "refs/heads/topic", &replacement],
        );
        fetch(repo).unwrap();
        git(source.path(), &["update-ref", "-d", "refs/heads/topic"]);
        git(
            source.path(),
            &["update-ref", "refs/heads/topic/child", &replacement],
        );
        fetch(repo).unwrap();
        git(repo, &["gc", "--prune=now"]);
        assert_eq!(audit(repo).unwrap(), 3);
        assert_eq!(
            git(
                repo,
                &["rev-parse", &archive_name("heads", "topic", &first)]
            ),
            first
        );
        assert_eq!(git(repo, &["cat-file", "-t", &first]), "commit");
        assert_eq!(
            git(
                repo,
                &["rev-parse", "refs/remotes/hunk-upstream/topic/child"]
            ),
            replacement
        );
    }
}
