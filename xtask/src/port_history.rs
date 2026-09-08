//! Read-only validation of semantic-port commit trailer references.

use super::{LedgerRecord, git_stdout, git_stdout_bytes};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::Path;

const WORKDECK_BASE: &str = "dc2ac39";

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

fn validate(commit: &Commit, records: &HashSet<String>, upstream: &HashSet<String>) -> Vec<String> {
    let mut errors = Vec::new();
    for record in &commit.records {
        if !records.contains(record) {
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
        if source.len() != 40 || !upstream.contains(source) {
            errors.push(format!(
                "{}: Hunk-Upstream is not a full commit reachable from preserved Hunk refs: {source}",
                commit.id
            ));
        }
    }
    errors
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
            "hunk-port/main-2c00f435^{commit}",
            "hunk-port/stable-v0.20.1^{commit}",
        ],
    )?
    .lines()
    .map(str::to_owned)
    .collect();
    let mut errors = Vec::new();
    let mut checked = 0;
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
        errors.extend(validate(commit, &records, &upstream));
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
