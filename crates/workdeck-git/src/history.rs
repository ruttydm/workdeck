use anyhow::{Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use git2::{BranchType, DiffOptions, Repository, Sort, Status, StatusOptions};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitReferenceKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
    Head,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitReference {
    pub name: String,
    pub full_name: String,
    pub target: String,
    pub kind: GitReferenceKind,
    pub upstream: Option<String>,
    pub checked_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommit {
    pub oid: String,
    pub summary: String,
    pub body: String,
    pub author_name: String,
    pub author_email: String,
    pub authored_at: DateTime<Utc>,
    pub parents: Vec<String>,
    pub references: Vec<GitReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChangedFile {
    pub path: PathBuf,
    pub previous_path: Option<PathBuf>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommitDetail {
    pub commit: GitCommit,
    pub committer_name: String,
    pub committer_email: String,
    pub committed_at: DateTime<Utc>,
    /// Git object signature presence only. Cryptographic trust is deliberately
    /// not implied because Workdeck does not own a trust-store verifier.
    pub signature: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<GitChangedFile>,
    pub files_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitGraphEdge {
    pub from_lane: usize,
    pub to_lane: usize,
    pub parent_oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitGraphRow {
    pub commit: GitCommit,
    pub lane: usize,
    pub edges: Vec<GitGraphEdge>,
    /// Occupied lanes entering and leaving this row. The renderer uses these
    /// explicit topology snapshots instead of guessing continuity from text.
    pub lanes_before: Vec<usize>,
    pub lanes_after: Vec<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitStatusSummary {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub conflicted: usize,
}

impl GitStatusSummary {
    pub fn total(&self) -> usize {
        self.staged + self.unstaged + self.untracked + self.conflicted
    }

    pub fn is_dirty(&self) -> bool {
        self.total() > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitGraphSnapshot {
    pub root: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub rows: Vec<GitGraphRow>,
    pub references: Vec<GitReference>,
    pub status: GitStatusSummary,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GitSearchCancellation(Arc<AtomicBool>);

impl GitSearchCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitSearch {
    pub commits: Vec<GitCommit>,
    pub scanned: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommitPathMatches {
    pub matching_oids: BTreeSet<String>,
    pub scanned: usize,
    pub cancelled: bool,
}

/// Finds which already-loaded commits touched a path fragment. The operation
/// reads Git objects only, compares each commit with its first parent, and is
/// both bounded by the supplied commit list and cooperatively cancellable.
///
/// Path matching deliberately checks both sides of a rename and uses a plain,
/// case-insensitive fragment instead of exposing libgit2 pathspec quirks in the
/// product query language.
pub fn commits_touching_path(
    root: &Path,
    commit_oids: &[String],
    path_fragment: &str,
    cancellation: &GitSearchCancellation,
) -> Result<GitCommitPathMatches> {
    let fragment = path_fragment.trim().replace('\\', "/").to_ascii_lowercase();
    if fragment.is_empty() || cancellation.is_cancelled() {
        return Ok(GitCommitPathMatches {
            matching_oids: BTreeSet::new(),
            scanned: 0,
            cancelled: cancellation.is_cancelled(),
        });
    }

    let repository = Repository::open(root)
        .with_context(|| format!("{} is not a readable Git repository", root.display()))?;
    let mut matching_oids = BTreeSet::new();
    let mut scanned = 0;
    for oid_text in commit_oids.iter().take(20_000) {
        if cancellation.is_cancelled() {
            return Ok(GitCommitPathMatches {
                matching_oids,
                scanned,
                cancelled: true,
            });
        }
        let oid = git2::Oid::from_str(oid_text)
            .with_context(|| format!("commit id {oid_text} is not a valid Git object id"))?;
        let commit = repository
            .find_commit(oid)
            .with_context(|| format!("commit {oid_text} is unavailable"))?;
        let tree = commit
            .tree()
            .with_context(|| format!("tree for commit {oid_text} is unavailable"))?;
        let parent_tree = commit.parent(0).ok().and_then(|parent| parent.tree().ok());
        let diff = repository
            .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
            .with_context(|| format!("could not inspect paths for commit {oid_text}"))?;
        scanned += 1;
        let touched = diff.deltas().any(|delta| {
            [delta.old_file().path(), delta.new_file().path()]
                .into_iter()
                .flatten()
                .any(|path| {
                    path.to_string_lossy()
                        .replace('\\', "/")
                        .to_ascii_lowercase()
                        .contains(&fragment)
                })
        });
        if touched {
            matching_oids.insert(oid_text.clone());
        }
    }
    Ok(GitCommitPathMatches {
        matching_oids,
        scanned,
        cancelled: false,
    })
}

/// Searches commit metadata across all local and remote refs without reading
/// the worktree or mutating repository state. Both the scan and result set are
/// explicitly bounded so portfolio-wide search stays predictable.
pub fn search_commits(
    root: &Path,
    query: &str,
    scan_limit: usize,
    result_limit: usize,
    cancellation: &GitSearchCancellation,
) -> Result<GitCommitSearch> {
    let terms = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    if terms.is_empty() || cancellation.is_cancelled() {
        return Ok(GitCommitSearch {
            commits: Vec::new(),
            scanned: 0,
            truncated: false,
        });
    }

    let repository = Repository::open(root)
        .with_context(|| format!("{} is not a readable Git repository", root.display()))?;
    let mut walk = repository
        .revwalk()
        .context("failed to create revision walker")?;
    for glob in ["refs/heads/*", "refs/remotes/*", "refs/tags/*"] {
        let _ = walk.push_glob(glob);
    }
    let _ = walk.push_ref("refs/stash");
    // Detached-only repositories may not have branch or remote refs. Pushing
    // HEAD as well is harmless because the revwalk de-duplicates OIDs.
    let _ = walk.push_head();
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;

    let scan_limit = scan_limit.max(1);
    let result_limit = result_limit.max(1);
    let mut seen = BTreeSet::new();
    let mut commits = Vec::new();
    let mut scanned = 0;
    let mut exhausted = true;
    for oid in walk {
        if cancellation.is_cancelled() {
            exhausted = false;
            break;
        }
        if scanned >= scan_limit {
            exhausted = false;
            break;
        }
        let oid = oid.context("failed to walk commit search")?;
        if !seen.insert(oid) {
            continue;
        }
        let commit = repository.find_commit(oid)?;
        scanned += 1;
        let signature = commit.author();
        let summary = commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("Untitled commit")
            .to_string();
        let body = commit.body().ok().flatten().unwrap_or_default().to_string();
        let author_name = signature.name().unwrap_or("Unknown author").to_string();
        let author_email = signature.email().unwrap_or_default().to_string();
        let oid_text = oid.to_string();
        let haystack = format!(
            "{}\n{}\n{}\n{}\n{}",
            summary, body, author_name, author_email, oid_text
        )
        .to_ascii_lowercase();
        if terms.iter().all(|term| haystack.contains(term)) {
            let authored_at = Utc
                .timestamp_opt(commit.time().seconds(), 0)
                .single()
                .unwrap_or_else(Utc::now);
            commits.push(GitCommit {
                oid: oid_text,
                summary,
                body,
                author_name,
                author_email,
                authored_at,
                parents: commit
                    .parent_ids()
                    .map(|parent| parent.to_string())
                    .collect(),
                references: Vec::new(),
            });
            if commits.len() >= result_limit {
                exhausted = false;
                break;
            }
        }
    }
    Ok(GitCommitSearch {
        commits,
        scanned,
        truncated: !exhausted,
    })
}

pub fn load_graph(root: &Path, limit: usize) -> Result<GitGraphSnapshot> {
    load_graph_inner(root, limit, true)
}

/// Loads refs and history without walking the working directory. This is the
/// latency-sensitive desktop path; Workdeck merges its bounded attention cache
/// into the result and refreshes detailed status independently.
pub fn load_graph_without_status(root: &Path, limit: usize) -> Result<GitGraphSnapshot> {
    load_graph_inner(root, limit, false)
}

/// Loads bounded details for one selected commit without touching the
/// worktree. Signature presence is reported honestly and is never presented
/// as cryptographic verification.
pub fn load_commit_detail(root: &Path, oid: &str, max_files: usize) -> Result<GitCommitDetail> {
    let mut repository = Repository::open(root)
        .with_context(|| format!("{} is not a readable Git repository", root.display()))?;
    let oid = git2::Oid::from_str(oid).context("commit id is not a valid Git object id")?;
    let references = collect_references(&mut repository, None)?
        .into_iter()
        .filter(|reference| reference.target == oid.to_string())
        .collect::<Vec<_>>();
    let commit = repository
        .find_commit(oid)
        .with_context(|| format!("commit {oid} is unavailable"))?;
    let author = commit.author();
    let authored_at = Utc
        .timestamp_opt(author.when().seconds(), 0)
        .single()
        .unwrap_or_else(Utc::now);
    let commit_summary = GitCommit {
        oid: oid.to_string(),
        summary: commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("Untitled commit")
            .to_string(),
        body: commit.body().ok().flatten().unwrap_or_default().to_string(),
        author_name: author.name().unwrap_or("Unknown author").to_string(),
        author_email: author.email().unwrap_or_default().to_string(),
        authored_at,
        parents: commit
            .parent_ids()
            .map(|parent| parent.to_string())
            .collect(),
        references,
    };
    let committer = commit.committer();
    let committed_at = Utc
        .timestamp_opt(commit.time().seconds(), 0)
        .single()
        .unwrap_or_else(Utc::now);
    let signature = match repository.extract_signature(&oid, None) {
        Ok(_) => "Present · not verified".to_string(),
        Err(error) if error.code() == git2::ErrorCode::NotFound => "Unsigned".to_string(),
        Err(error) => format!("Unavailable · {}", error.message()),
    };

    let tree = commit
        .tree()
        .context("selected commit tree is unavailable")?;
    let parent_tree = if commit.parent_count() > 0 {
        Some(
            commit
                .parent(0)
                .context("selected commit parent is unavailable")?
                .tree()
                .context("selected commit parent tree is unavailable")?,
        )
    } else {
        None
    };
    let mut options = DiffOptions::new();
    let diff = repository
        .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut options))
        .context("could not build selected commit diff")?;
    let stats = diff
        .stats()
        .context("could not calculate commit statistics")?;
    let max_files = max_files.clamp(1, 2_000);
    let mut files = Vec::with_capacity(stats.files_changed().min(max_files));
    for delta in diff.deltas().take(max_files) {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .unwrap_or_else(|| Path::new("unknown"))
            .to_path_buf();
        let previous_path = delta
            .old_file()
            .path()
            .filter(|previous| *previous != path.as_path())
            .map(Path::to_path_buf);
        files.push(GitChangedFile {
            path,
            previous_path,
            status: format!("{:?}", delta.status()).to_ascii_lowercase(),
        });
    }
    Ok(GitCommitDetail {
        commit: commit_summary,
        committer_name: committer.name().unwrap_or("Unknown committer").to_string(),
        committer_email: committer.email().unwrap_or_default().to_string(),
        committed_at,
        signature,
        files_changed: stats.files_changed(),
        insertions: stats.insertions(),
        deletions: stats.deletions(),
        files_truncated: stats.files_changed() > files.len(),
        files,
    })
}

fn load_graph_inner(root: &Path, limit: usize, include_status: bool) -> Result<GitGraphSnapshot> {
    let mut repository = Repository::open(root)
        .with_context(|| format!("{} is not a readable Git repository", root.display()))?;
    let (head_oid, branch) = {
        let head = repository.head().ok();
        (
            head.as_ref()
                .and_then(|head| head.target())
                .map(|oid| oid.to_string()),
            head.as_ref()
                .and_then(|head| head.shorthand().ok())
                .map(ToOwned::to_owned),
        )
    };
    let references = collect_references(&mut repository, head_oid.as_deref())?;
    let refs_by_target = references.iter().cloned().fold(
        BTreeMap::<String, Vec<GitReference>>::new(),
        |mut values, reference| {
            values
                .entry(reference.target.clone())
                .or_default()
                .push(reference);
            values
        },
    );

    let mut walk = repository
        .revwalk()
        .context("failed to create revision walker")?;
    if head_oid.is_some() {
        walk.push_glob("refs/heads/*")?;
        walk.push_glob("refs/remotes/*")?;
        for reference in references
            .iter()
            .filter(|reference| reference.kind == GitReferenceKind::Stash)
        {
            if let Ok(oid) = git2::Oid::from_str(&reference.target) {
                let _ = walk.push(oid);
            }
        }
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
    }

    let bounded = limit.max(1);
    let mut commits = Vec::with_capacity(bounded.min(2_000));
    for oid in walk.take(bounded + 1) {
        let oid = oid.context("failed to walk commit graph")?;
        let commit = repository.find_commit(oid)?;
        let signature = commit.author();
        let authored_at = Utc
            .timestamp_opt(commit.time().seconds(), 0)
            .single()
            .unwrap_or_else(Utc::now);
        commits.push(GitCommit {
            oid: oid.to_string(),
            summary: commit
                .summary()
                .ok()
                .flatten()
                .unwrap_or("Untitled commit")
                .to_string(),
            body: commit.body().ok().flatten().unwrap_or_default().to_string(),
            author_name: signature.name().unwrap_or("Unknown author").to_string(),
            author_email: signature.email().unwrap_or_default().to_string(),
            authored_at,
            parents: commit
                .parent_ids()
                .map(|parent| parent.to_string())
                .collect(),
            references: refs_by_target
                .get(&oid.to_string())
                .cloned()
                .unwrap_or_default(),
        });
    }
    let truncated = commits.len() > bounded;
    commits.truncate(bounded);
    let rows = layout_rows(commits);

    Ok(GitGraphSnapshot {
        root: root.to_path_buf(),
        head: head_oid,
        branch,
        rows,
        references,
        status: if include_status {
            status_summary(&repository)?
        } else {
            GitStatusSummary::default()
        },
        truncated,
    })
}

fn collect_references(
    repository: &mut Repository,
    head_oid: Option<&str>,
) -> Result<Vec<GitReference>> {
    let mut values = Vec::new();
    for branch in repository.branches(None)? {
        let (branch, branch_type) = branch?;
        let Some(target) = branch.get().target() else {
            continue;
        };
        let name = branch.name()?.unwrap_or("unnamed").to_string();
        let kind = match branch_type {
            BranchType::Local => GitReferenceKind::LocalBranch,
            BranchType::Remote => GitReferenceKind::RemoteBranch,
        };
        let full_name = branch.get().name().unwrap_or(&name).to_string();
        let upstream = if branch_type == BranchType::Local {
            branch
                .upstream()
                .ok()
                .and_then(|upstream| upstream.name().ok().flatten().map(ToOwned::to_owned))
        } else {
            None
        };
        values.push(GitReference {
            name,
            full_name,
            target: target.to_string(),
            kind,
            upstream,
            checked_out: head_oid.is_some_and(|head| head == target.to_string()),
        });
    }
    for reference in repository.references_glob("refs/tags/*")? {
        let reference = reference?;
        let Some(target) = reference.peel_to_commit().ok().map(|commit| commit.id()) else {
            continue;
        };
        let full_name = reference.name().unwrap_or("refs/tags/unnamed").to_string();
        values.push(GitReference {
            name: full_name.trim_start_matches("refs/tags/").to_string(),
            full_name,
            target: target.to_string(),
            kind: GitReferenceKind::Tag,
            upstream: None,
            checked_out: false,
        });
    }
    let mut stashes = Vec::new();
    repository.stash_foreach(|index, message, oid| {
        stashes.push(GitReference {
            name: format!("stash@{{{index}}} · {message}"),
            full_name: format!("refs/stash/{index}"),
            target: oid.to_string(),
            kind: GitReferenceKind::Stash,
            upstream: None,
            checked_out: false,
        });
        true
    })?;
    values.extend(stashes);
    values.sort_by(|left, right| {
        reference_order(left.kind)
            .cmp(&reference_order(right.kind))
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
    });
    Ok(values)
}

fn reference_order(kind: GitReferenceKind) -> u8 {
    match kind {
        GitReferenceKind::Head => 0,
        GitReferenceKind::LocalBranch => 1,
        GitReferenceKind::RemoteBranch => 2,
        GitReferenceKind::Tag => 3,
        GitReferenceKind::Stash => 4,
    }
}

fn status_summary(repository: &Repository) -> Result<GitStatusSummary> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let mut summary = GitStatusSummary::default();
    for entry in repository.statuses(Some(&mut options))?.iter() {
        let status = entry.status();
        if status.intersects(Status::CONFLICTED) {
            summary.conflicted += 1;
        } else if status.intersects(
            Status::INDEX_NEW
                | Status::INDEX_MODIFIED
                | Status::INDEX_DELETED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE,
        ) {
            summary.staged += 1;
        }
        if status.intersects(Status::WT_NEW) {
            summary.untracked += 1;
        } else if status.intersects(
            Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE,
        ) {
            summary.unstaged += 1;
        }
    }
    Ok(summary)
}

fn layout_rows(commits: Vec<GitCommit>) -> Vec<GitGraphRow> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        let existing_lane = lanes
            .iter()
            .position(|value| value.as_deref() == Some(commit.oid.as_str()));
        // Capture the frontier before introducing a disconnected ref tip. A
        // newly discovered tip must start at its node; drawing it from the top
        // of the row invents an edge to the previous commit.
        let lanes_before = lanes
            .iter()
            .enumerate()
            .filter_map(|(index, value)| value.as_ref().map(|_| index))
            .collect();
        let lane = existing_lane.unwrap_or_else(|| {
            let lane = lanes
                .iter()
                .position(Option::is_none)
                .unwrap_or(lanes.len());
            if lane == lanes.len() {
                lanes.push(Some(commit.oid.clone()));
            } else {
                lanes[lane] = Some(commit.oid.clone());
            }
            lane
        });
        lanes[lane] = None;
        let mut edges = Vec::with_capacity(commit.parents.len());
        for (index, parent) in commit.parents.iter().enumerate() {
            let parent_lane = lanes
                .iter()
                .position(|value| value.as_deref() == Some(parent.as_str()))
                .unwrap_or_else(|| {
                    let preferred = (index == 0 && lanes[lane].is_none()).then_some(lane);
                    let destination = preferred
                        .or_else(|| lanes.iter().position(Option::is_none))
                        .unwrap_or(lanes.len());
                    if destination == lanes.len() {
                        lanes.push(Some(parent.clone()));
                    } else {
                        lanes[destination] = Some(parent.clone());
                    }
                    destination
                });
            edges.push(GitGraphEdge {
                from_lane: lane,
                to_lane: parent_lane,
                parent_oid: parent.clone(),
            });
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        rows.push(GitGraphRow {
            commit,
            lane,
            edges,
            lanes_before,
            lanes_after: lanes
                .iter()
                .enumerate()
                .filter_map(|(index, value)| value.as_ref().map(|_| index))
                .collect(),
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};

    fn git(root: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn graph_contains_refs_merges_and_worktree_state() {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "-q", "-b", "main"]);
        git(fixture.path(), &["config", "user.name", "Workdeck"]);
        git(
            fixture.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        fs::write(fixture.path().join("README.md"), "base\n").unwrap();
        git(fixture.path(), &["add", "README.md"]);
        git(fixture.path(), &["commit", "-qm", "base"]);
        git(fixture.path(), &["checkout", "-qb", "feature"]);
        fs::write(fixture.path().join("feature.txt"), "feature\n").unwrap();
        git(fixture.path(), &["add", "feature.txt"]);
        git(fixture.path(), &["commit", "-qm", "feature"]);
        git(fixture.path(), &["checkout", "-q", "main"]);
        git(
            fixture.path(),
            &["merge", "--no-ff", "feature", "-m", "merge feature"],
        );
        fs::write(fixture.path().join("README.md"), "stashed\n").unwrap();
        git(fixture.path(), &["stash", "push", "-m", "review later"]);
        fs::write(fixture.path().join("incoming.txt"), "incoming\n").unwrap();

        let graph = load_graph(fixture.path(), 100).unwrap();
        assert_eq!(graph.branch.as_deref(), Some("main"));
        assert!(graph.rows.len() >= 3);
        assert!(graph.rows.iter().any(|row| row.commit.parents.len() == 2));
        assert!(
            graph
                .references
                .iter()
                .any(|reference| reference.name == "feature")
        );
        assert!(
            graph
                .references
                .iter()
                .any(|reference| reference.kind == GitReferenceKind::Stash)
        );
        assert_eq!(graph.status.untracked, 1);
        assert!(!graph.rows[0].lanes_before.contains(&graph.rows[0].lane));
        assert!(graph.rows.iter().skip(1).all(|row| {
            row.lanes_before.contains(&row.lane) || !row.commit.references.is_empty()
        }));
        assert!(
            graph
                .rows
                .windows(2)
                .all(|rows| { rows[0].lanes_after == rows[1].lanes_before })
        );
        assert!(graph.rows.iter().all(|row| {
            row.edges.iter().all(|edge| {
                edge.from_lane == row.lane
                    && row.lanes_after.contains(&edge.to_lane)
                    && row.commit.parents.contains(&edge.parent_oid)
            })
        }));
        assert!(!graph.truncated);
    }

    #[test]
    fn octopus_merge_exposes_every_parent_lane_to_the_gpu_renderer() {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "-q", "-b", "main"]);
        git(fixture.path(), &["config", "user.name", "Workdeck"]);
        git(
            fixture.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        fs::write(fixture.path().join("base.txt"), "base\n").unwrap();
        git(fixture.path(), &["add", "base.txt"]);
        git(fixture.path(), &["commit", "-qm", "base"]);
        for branch in ["one", "two", "three"] {
            git(fixture.path(), &["checkout", "-qb", branch, "main"]);
            fs::write(
                fixture.path().join(format!("{branch}.txt")),
                format!("{branch}\n"),
            )
            .unwrap();
            git(fixture.path(), &["add", &format!("{branch}.txt")]);
            git(fixture.path(), &["commit", "-qm", branch]);
        }
        git(fixture.path(), &["checkout", "-q", "main"]);
        git(
            fixture.path(),
            &["merge", "--no-ff", "one", "two", "three", "-m", "octopus"],
        );

        let graph = load_graph(fixture.path(), 100).unwrap();
        let merge = graph
            .rows
            .iter()
            .find(|row| row.commit.summary == "octopus")
            .expect("octopus merge");
        assert_eq!(merge.commit.parents.len(), 4);
        assert_eq!(merge.edges.len(), 4);
        assert!(merge.lanes_after.len() >= 4);
    }

    fn commit(oid: &str, parents: &[&str]) -> GitCommit {
        GitCommit {
            oid: oid.into(),
            summary: oid.into(),
            body: String::new(),
            author_name: "Workdeck".into(),
            author_email: "workdeck@example.test".into(),
            authored_at: Utc::now(),
            parents: parents.iter().map(|parent| (*parent).into()).collect(),
            references: Vec::new(),
        }
    }

    #[test]
    fn merge_topology_preserves_parallel_lane_and_converges_without_gaps() {
        let rows = layout_rows(vec![
            commit("merge", &["main", "feature"]),
            commit("feature", &["base"]),
            commit("main", &["base"]),
            commit("base", &[]),
        ]);

        assert_eq!(rows[0].lane, 0);
        assert!(rows[0].lanes_before.is_empty());
        assert_eq!(
            rows[0]
                .edges
                .iter()
                .map(|edge| (edge.parent_oid.as_str(), edge.to_lane))
                .collect::<Vec<_>>(),
            vec![("main", 0), ("feature", 1)]
        );
        assert_eq!(rows[0].lanes_after, vec![0, 1]);

        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].lanes_before, vec![0, 1]);
        assert_eq!(rows[1].edges[0].to_lane, 1);
        assert_eq!(rows[1].lanes_after, vec![0, 1]);

        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[2].lanes_before, vec![0, 1]);
        assert_eq!(rows[2].edges[0].to_lane, 1);
        assert_eq!(rows[2].lanes_after, vec![1]);

        assert_eq!(rows[3].lane, 1);
        assert_eq!(rows[3].lanes_before, vec![1]);
        assert!(rows[3].lanes_after.is_empty());
    }

    #[test]
    fn disconnected_ref_tip_starts_at_its_node_and_reuses_an_empty_lane() {
        let rows = layout_rows(vec![
            commit("main", &["base"]),
            commit("side", &["side-base"]),
            commit("base", &[]),
            commit("side-base", &[]),
        ]);

        assert_eq!(rows[0].lane, 0);
        assert!(rows[0].lanes_before.is_empty());
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].lanes_before, vec![0]);
        assert!(!rows[1].lanes_before.contains(&rows[1].lane));
        assert_eq!(rows[2].lane, 0);
        assert_eq!(rows[3].lane, 1);
    }

    #[test]
    fn graph_limit_is_explicit_and_lane_layout_is_stable() {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "-q", "-b", "main"]);
        git(fixture.path(), &["config", "user.name", "Workdeck"]);
        git(
            fixture.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        for index in 0..4 {
            fs::write(fixture.path().join("file.txt"), format!("{index}\n")).unwrap();
            git(fixture.path(), &["add", "file.txt"]);
            git(
                fixture.path(),
                &["commit", "-qm", &format!("commit {index}")],
            );
        }
        let graph = load_graph(fixture.path(), 2).unwrap();
        assert_eq!(graph.rows.len(), 2);
        assert!(graph.truncated);
        assert!(graph.rows.iter().all(|row| row.lane == 0));
    }

    #[test]
    fn selected_commit_detail_is_bounded_and_does_not_claim_signature_verification() {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "-q", "-b", "main"]);
        git(fixture.path(), &["config", "user.name", "Workdeck"]);
        git(
            fixture.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        fs::write(fixture.path().join("one.txt"), "one\n").unwrap();
        fs::write(fixture.path().join("two.txt"), "two\n").unwrap();
        git(fixture.path(), &["add", "one.txt", "two.txt"]);
        git(fixture.path(), &["commit", "-qm", "bounded detail"]);
        let graph = load_graph_without_status(fixture.path(), 10).unwrap();
        let detail = load_commit_detail(fixture.path(), &graph.rows[0].commit.oid, 1).unwrap();
        assert_eq!(detail.commit.summary, "bounded detail");
        assert_eq!(detail.files_changed, 2);
        assert_eq!(detail.files.len(), 1);
        assert!(detail.files_truncated);
        assert!(detail.signature == "Unsigned" || detail.signature.contains("not verified"));
    }

    #[test]
    fn commit_search_is_read_only_bounded_and_cancellable() {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "-q", "-b", "main"]);
        git(fixture.path(), &["config", "user.name", "Workdeck"]);
        git(
            fixture.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        for (index, message) in ["initial shell", "polish search", "finish review"]
            .into_iter()
            .enumerate()
        {
            fs::write(fixture.path().join("file.txt"), format!("{index}\n")).unwrap();
            git(fixture.path(), &["add", "file.txt"]);
            git(fixture.path(), &["commit", "-qm", message]);
        }

        let cancellation = GitSearchCancellation::default();
        let result =
            search_commits(fixture.path(), "polish search", 20, 10, &cancellation).unwrap();
        assert_eq!(result.commits.len(), 1);
        assert_eq!(result.commits[0].summary, "polish search");
        assert!(result.scanned <= 20);

        cancellation.cancel();
        let cancelled = search_commits(fixture.path(), "review", 20, 10, &cancellation).unwrap();
        assert!(cancelled.commits.is_empty());
        assert_eq!(cancelled.scanned, 0);
    }

    #[test]
    fn path_filter_matches_add_modify_and_rename_without_touching_worktree_state() {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "-q", "-b", "main"]);
        git(fixture.path(), &["config", "user.name", "Workdeck"]);
        git(
            fixture.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        fs::create_dir_all(fixture.path().join("Sources/App")).unwrap();
        fs::write(fixture.path().join("Sources/App/main.rs"), "fn main() {}\n").unwrap();
        fs::write(fixture.path().join("README.md"), "docs\n").unwrap();
        git(fixture.path(), &["add", "."]);
        git(fixture.path(), &["commit", "-qm", "initial"]);
        fs::write(
            fixture.path().join("Sources/App/main.rs"),
            "fn main() { println!(\"workdeck\"); }\n",
        )
        .unwrap();
        git(fixture.path(), &["add", "Sources/App/main.rs"]);
        git(fixture.path(), &["commit", "-qm", "modify app"]);
        git(
            fixture.path(),
            &["mv", "Sources/App/main.rs", "Sources/App/entry.rs"],
        );
        git(fixture.path(), &["commit", "-qm", "rename app"]);

        let graph = load_graph_without_status(fixture.path(), 20).unwrap();
        let oids = graph
            .rows
            .iter()
            .map(|row| row.commit.oid.clone())
            .collect::<Vec<_>>();
        let cancellation = GitSearchCancellation::default();
        let matches =
            commits_touching_path(fixture.path(), &oids, "sources/app/main", &cancellation)
                .unwrap();
        assert_eq!(matches.matching_oids.len(), 3);
        assert_eq!(matches.scanned, 3);
        assert!(!matches.cancelled);

        cancellation.cancel();
        let cancelled =
            commits_touching_path(fixture.path(), &oids, "README", &cancellation).unwrap();
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.scanned, 0);
    }
}
