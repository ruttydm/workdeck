//! Read-only validation of semantic-port commit trailer references.

use super::{LedgerRecord, git_output, git_stdout, git_stdout_bytes};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::Path;

const WORKDECK_BASE: &str = "dc2ac39";
const HUNK_BASELINE_REF: &str = "hunk-port/main-2c00f435^{}";

#[derive(Clone)]
struct Commit {
    id: String,
    records: Vec<String>,
    upstream: Vec<String>,
}

fn parse_log(bytes: &[u8]) -> Result<Vec<Commit>> {
    let text = std::str::from_utf8(bytes).context("decode port commit trailers")?;
    let mut fields = text.split('\0');
    let mut commits = Vec::new();
    while let Some(id) = fields.next() {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        let records = fields.next().context("missing Hunk-Port trailer field")?;
        let upstream = fields
            .next()
            .context("missing Hunk-Upstream trailer field")?;
        let values = |field: &str| {
            field
                .split('\u{1f}')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect()
        };
        commits.push(Commit {
            id: id.to_owned(),
            records: values(records),
            upstream: values(upstream),
        });
    }
    Ok(commits)
}

#[cfg(test)]
fn validate(commit: &Commit, records: &HashSet<String>, upstream: &HashSet<String>) -> Vec<String> {
    validate_with_context(commit, records, upstream, None, None)
}

const LEGACY_BASELINE_TRAILER_COMMITS: &[&str] = &[
    "76952ccc633bb999f34303829792160318d2c61b",
    "bd4e3c987dbced70602b9de29ff15f29f7a79bd2",
    "905baf277e8746d8c32e5c2001db4919fcb0ecce",
    "7ec69f889aea63cfac29cb73b3d8c2e7b08c4e71",
    "2bfecf88e52c9374007026ebaf9e53de46654f80",
    "d428b11c2dd7ffe944e17e05a09c4fdaa08853b7",
    "13209d8bc8c78eb381cdc7c5065e2cdb6aa078be",
];

// These commits predate the trailer-order check. Their exact source baseline
// is recoverable from their canonical Hunk-Port intervals, so the checker
// supplies a synthetic receipt rather than silently skipping provenance.
const LEGACY_NO_TRAILER_PROVENANCE: &[&str] = &[
    "ade3a4e3d4afd5ba3f037f1d1d766e5d1eb5184c",
    "194208fbbc9aa910eeb6b05724621216953b6135",
    "61430984eafd3136e6da387847c47827a3b74772",
];

// These labels were used by the first semantic-port implementation for
// executable parity receipts that intentionally do not correspond to one
// baseline blob. They remain explicit, finite aliases rather than a general
// waiver: each maps to a committed fixture and the checker requires that the
// fixture is present in the historical commit itself.
const SUPPLEMENTAL_EVIDENCE: &[(&str, &str)] = &[
    (
        "supplemental alpha-catalog-command-sequence",
        "port/hunk/alpha-catalog-command-sequence.md",
    ),
    (
        "supplemental alpha-file-navigation-round-trip",
        "port/hunk/alpha-file-navigation-round-trip.md",
    ),
    (
        "supplemental alpha-hunk-reload-clamp",
        "port/hunk/alpha-hunk-reload-clamp.md",
    ),
    (
        "supplemental alpha-selection-document-identity",
        "port/hunk/alpha-selection-document-identity.md",
    ),
    (
        "supplemental alpha-user-note-lifecycle",
        "port/hunk/alpha-user-note-lifecycle.md",
    ),
    (
        "supplemental counted-alpha-file-navigation",
        "port/hunk/counted-alpha-file-navigation.md",
    ),
    (
        "supplemental keyboard-draft-reveal",
        "port/hunk/alpha-note-draft-anchor.md",
    ),
    (
        "supplemental live-beta-annotated-navigation",
        "port/hunk/live-beta-annotated-navigation.md",
    ),
    (
        "supplemental rendered-note-deletion",
        "port/hunk/alpha-nested-user-notes.md",
    ),
    (
        "supplemental repeated-comment-reveal",
        "port/hunk/repeated-comment-reveal.md",
    ),
    (
        "supplemental semantic-note-projection-oracle",
        "port/hunk/semantic-note-size-oracle.json",
    ),
    (
        "supplemental semantic-note-size-boundaries",
        "port/hunk/semantic-note-size-boundaries.json",
    ),
    (
        "supplemental semantic-note-size-boundary",
        "port/hunk/composer-note-size.md",
    ),
    (
        "supplemental session-clear-human-notes",
        "port/hunk/session-clear-human-notes.md",
    ),
    (
        "verification-only",
        "port/hunk/cursor-note-workspace-verification.md",
    ),
    (
        "verification refresh",
        "port/hunk/cursor-note-workspace-verification.md",
    ),
];

fn supplemental_fixture(value: &str) -> Option<&'static str> {
    let label = value.split_once(" (").map_or(value, |(label, _)| label);
    SUPPLEMENTAL_EVIDENCE
        .iter()
        .find_map(|(candidate, fixture)| (*candidate == label).then_some(*fixture))
}

fn baseline_source_alias(
    value: &str,
    baseline_short: &str,
    baseline_records: &HashSet<String>,
) -> bool {
    // Historical records sometimes carried a suffix explaining that the
    // interval was a partial verification. Only baseline-prefixed intervals
    // with an explicit partial/supplemental suffix are accepted here, and the
    // normalized interval must be a real, fully mapped record in today's ledger.
    let Some((prefix, rest)) = value.split_once(':') else {
        return false;
    };
    if prefix != baseline_short && prefix != "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2" {
        return false;
    }
    let Some((canonical_rest, suffix)) = rest.split_once(" (") else {
        return false;
    };
    if !(suffix.starts_with("partial") || suffix.starts_with("supplemental")) {
        return false;
    }
    let canonical = format!("{baseline_short}:{canonical_rest}");
    baseline_records.contains(&canonical)
}

fn valid_upstream_source(
    repo: &Path,
    commit: &Commit,
    source: &str,
    upstream: &HashSet<String>,
) -> bool {
    if source == "baseline" {
        return LEGACY_BASELINE_TRAILER_COMMITS.contains(&commit.id.as_str());
    }
    let mut fields = source.splitn(2, char::is_whitespace);
    let hash = fields.next().unwrap_or_default();
    if hash.len() != 40 || !upstream.contains(hash) {
        return false;
    }
    let Some(path) = fields.next() else {
        return true;
    };
    let path = path.trim();
    if path.is_empty() || path.contains('\0') || path.starts_with('-') {
        return false;
    }
    let object = format!("{hash}:{path}");
    git_output(repo, ["cat-file", "-e", &object])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn validate_with_context(
    commit: &Commit,
    records: &HashSet<String>,
    upstream: &HashSet<String>,
    baseline_records: Option<(&str, &HashSet<String>)>,
    repo: Option<&Path>,
) -> Vec<String> {
    let mut errors = Vec::new();
    for record in &commit.records {
        let known = records.contains(record)
            || baseline_records
                .is_some_and(|(short, all)| baseline_source_alias(record, short, all))
            || supplemental_fixture(record).is_some();
        if !known {
            errors.push(format!(
                "{}: Hunk-Port record absent from that commit's ledger: {record}",
                commit.id
            ));
        }
    }
    if !commit.records.is_empty() && commit.upstream.is_empty() {
        errors.push(format!(
            "{}: Hunk-Port lacks Hunk-Upstream provenance",
            commit.id
        ));
    }
    for source in &commit.upstream {
        let known = repo.is_some_and(|repo| valid_upstream_source(repo, commit, source, upstream))
            || (repo.is_none() && source.len() == 40 && upstream.contains(source));
        if !known {
            errors.push(format!(
                "{}: Hunk-Upstream is not a full commit reachable from preserved Hunk refs: {source}",
                commit.id
            ));
        }
    }
    errors
}

fn historical_ledger_records(repo: &Path) -> Result<HashSet<String>> {
    let ledger = git_stdout(repo, ["show", "HEAD:port/hunk/ledger.jsonl"])?;
    Ok(ledger
        .lines()
        .filter_map(|line| serde_json::from_str::<LedgerRecord>(line).ok())
        .map(|record| record.id)
        .collect())
}

fn historical_commit_paths(repo: &Path, commit: &str) -> Result<HashSet<String>> {
    Ok(git_stdout(
        repo,
        ["diff-tree", "--no-commit-id", "--name-only", "-r", commit],
    )?
    .lines()
    .map(str::to_owned)
    .collect())
}

pub(super) fn check(repo: &Path) -> Result<()> {
    let base = git_stdout(repo, ["rev-parse", &format!("{WORKDECK_BASE}^{{commit}}")])?;
    let ancestor = super::git_output(repo, ["merge-base", "--is-ancestor", &base, "HEAD"])?;
    if !ancestor.status.success() {
        bail!("Workdeck port base {base} is not an ancestor of HEAD");
    }
    let log = git_stdout_bytes(
        repo,
        [
            "log",
            &format!("{base}..HEAD"),
            "--format=%H%x00%(trailers:key=Hunk-Port,valueonly,separator=%x1f)%x00%(trailers:key=Hunk-Upstream,valueonly,separator=%x1f)%x00",
        ],
    )?;
    let commits = parse_log(&log)?;
    let upstream: HashSet<_> = git_stdout(
        repo,
        [
            "rev-list",
            "--glob=refs/remotes/hunk-upstream/*",
            "--glob=refs/tags/hunk-upstream/*",
            "--glob=refs/upstream/hunk/archive/*",
            "hunk-port/main-2c00f435^{commit}",
            "hunk-port/stable-v0.20.1^{commit}",
        ],
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    let mut errors = Vec::new();
    let mut checked = 0;
    let hunk_baseline = git_stdout(repo, ["rev-parse", HUNK_BASELINE_REF])?;
    let baseline_short = &hunk_baseline[..12];
    let baseline_records = historical_ledger_records(repo)?;
    for commit in &commits {
        if commit.records.is_empty() && commit.upstream.is_empty() {
            continue;
        }
        checked += 1;
        let mut records = HashSet::new();
        if !commit.records.is_empty() {
            let ledger = git_stdout(
                repo,
                ["show", &format!("{}:port/hunk/ledger.jsonl", commit.id)],
            )?;
            for line in ledger.lines() {
                let record: LedgerRecord = serde_json::from_str(line)
                    .with_context(|| format!("decode ledger at {}", commit.id))?;
                records.insert(record.id);
            }
        }
        let mut contextual = commit.clone();
        if contextual.upstream.is_empty()
            && LEGACY_NO_TRAILER_PROVENANCE.contains(&contextual.id.as_str())
        {
            contextual.upstream.push(hunk_baseline.clone());
        }
        let paths = historical_commit_paths(repo, &contextual.id)?;
        for record in &contextual.records {
            if let Some(fixture) = supplemental_fixture(record)
                && !paths.contains(fixture)
            {
                errors.push(format!(
                    "{}: supplemental provenance fixture is not changed by that commit: {fixture}",
                    contextual.id
                ));
            }
        }
        errors.extend(validate_with_context(
            &contextual,
            &records,
            &upstream,
            Some((baseline_short, &baseline_records)),
            Some(repo),
        ));
    }
    println!(
        "Checked {checked} port-trailered commits among {} Workdeck commits",
        commits.len()
    );
    if !errors.is_empty() {
        bail!("{} provenance errors:\n{}", errors.len(), errors.join("\n"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repeated_trailers_and_unannotated_commits_without_field_drift() {
        let commits = parse_log(b"abc\0one\x1ftwo\0source\0\ndef\0\0\0\n").unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].records, ["one", "two"]);
        assert_eq!(commits[0].upstream, ["source"]);
        assert_eq!(commits[1].id, "def");
        assert!(commits[1].records.is_empty());
        assert!(parse_log(b"abc\0one").is_err());
    }

    #[test]
    fn validates_historical_ids_and_requires_reachable_full_source_commits() {
        let source = "a".repeat(40);
        let records = HashSet::from(["old-interval".into()]);
        let upstream = HashSet::from([source.clone()]);
        let mut commit = Commit {
            id: "local".into(),
            records: vec!["old-interval".into()],
            upstream: vec![source],
        };
        assert!(validate(&commit, &records, &upstream).is_empty());
        commit.records.push("invented".into());
        assert_eq!(validate(&commit, &records, &upstream).len(), 1);
        commit.upstream.clear();
        assert_eq!(validate(&commit, &records, &upstream).len(), 2);
        commit.upstream = vec!["aaaaaaa".into(), "b".repeat(40)];
        assert_eq!(validate(&commit, &records, &upstream).len(), 3);
    }

    #[test]
    fn git_trailer_extraction_ignores_body_mentions_and_preserves_repeated_values() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path();
        git_stdout(repo, ["init", "--quiet"]).unwrap();
        git_stdout(repo, ["config", "user.name", "Port history test"]).unwrap();
        git_stdout(repo, ["config", "user.email", "port-test@example.invalid"]).unwrap();
        git_stdout(repo, ["config", "commit.gpgsign", "false"]).unwrap();
        git_stdout(repo, ["config", "core.hooksPath", "absent-test-hooks"]).unwrap();
        git_stdout(
            repo,
            [
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "Port test\n\nHunk-Port: body mention, not a trailer\nThis paragraph continues.\n\nHunk-Port: one\nHunk-Port: two\nHunk-Upstream: source",
            ],
        )
        .unwrap();
        let log = git_stdout_bytes(
            repo,
            [
                "log",
                "-1",
                "--format=%H%x00%(trailers:key=Hunk-Port,valueonly,separator=%x1f)%x00%(trailers:key=Hunk-Upstream,valueonly,separator=%x1f)%x00",
            ],
        )
        .unwrap();
        let commits = parse_log(&log).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].id.len(), 40);
        assert_eq!(commits[0].records, ["one", "two"]);
        assert_eq!(commits[0].upstream, ["source"]);
    }
}
