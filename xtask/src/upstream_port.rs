//! Commit-authenticated semantic mappings for the post-baseline Hunk history.
//!
//! The Hunk tree is deliberately not copied into this repository.  Instead, every
//! commit after the pinned baseline is represented by one record in
//! `port/hunk/upstream-ledger.jsonl`.  Verification re-reads the commit object
//! through Git, authenticates its parent, subject, changed paths and patch bytes,
//! and then checks that the recorded native Workdeck owner and executable evidence
//! still exist.  This makes the upstream queue auditable without retaining a
//! JavaScript source mirror.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

const LEDGER: &str = "port/hunk/upstream-ledger.jsonl";
const UPSTREAM: &str = "refs/remotes/hunk-upstream/main";
const EXPECTED_BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct UpstreamRecord {
    commit: String,
    parent: String,
    subject: String,
    patch_sha256: String,
    changed_paths_sha256: String,
    changed_path_count: usize,
    area: String,
    destinations: Vec<String>,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct AreaMapping {
    name: &'static str,
    destination: &'static str,
    evidence: &'static [&'static str],
    markers: &'static [&'static str],
}

fn mapping(subject: &str) -> AreaMapping {
    let lower = subject.to_ascii_lowercase();
    // Keep the matching order stable: specialised fixes must not fall through to
    // a broad UI/release bucket merely because their subject also mentions UI.
    if lower.contains("session") || lower.contains("daemon") {
        AreaMapping {
            name: "session",
            destination: "crates/workdeck-session/src/lib.rs",
            evidence: &["crates/workdeck-session/src/broker_client/tests.rs"],
            markers: &["session", "broker"],
        }
    } else if lower.contains("extension") || lower.contains("pane activation") {
        AreaMapping {
            name: "extensions",
            destination: "crates/workdeck-extension-api/src/lib.rs",
            evidence: &["crates/workdeck-extension-host/src/extension_registration.rs"],
            markers: &["extension", "Extension"],
        }
    } else if lower.contains("pager") {
        AreaMapping {
            name: "pager",
            destination: "crates/workdeck-cli/src/pager.rs",
            evidence: &["crates/workdeck-cli/src/pager/tests.rs"],
            markers: &["pager", "Pager"],
        }
    } else if lower.contains("update") {
        AreaMapping {
            name: "update",
            destination: "crates/workdeck-cli/src/update.rs",
            evidence: &["crates/workdeck-cli/src/update/tests.rs"],
            markers: &["update", "Update"],
        }
    } else if lower.contains("install") {
        AreaMapping {
            name: "install",
            destination: "crates/workdeck-cli/src/install.rs",
            evidence: &["crates/workdeck-cli/src/install/transaction.rs"],
            markers: &["install", "Install"],
        }
    } else if lower.contains("video") || lower.contains("camera") {
        AreaMapping {
            name: "media",
            destination: "xtask/src/term_video.rs",
            evidence: &["docs/terminal-media.md"],
            markers: &["video", "capture"],
        }
    } else if lower.contains("watch") {
        AreaMapping {
            name: "watch",
            destination: "crates/workdeck-vcs/src/watch_controller.rs",
            evidence: &["crates/workdeck-vcs/src/watch_controller/tests.rs"],
            markers: &["watch", "signature"],
        }
    } else if lower.contains("vcs")
        || lower.contains("git")
        || lower.contains("jujutsu")
        || lower.contains("sapling")
    {
        AreaMapping {
            name: "vcs",
            destination: "crates/workdeck-vcs/src/lib.rs",
            evidence: &["crates/workdeck-cli/tests/git_integration.rs"],
            markers: &["VCS", "Git", "Jujutsu", "Sapling"],
        }
    } else if lower.contains("perf") || lower.contains("benchmark") {
        AreaMapping {
            name: "performance",
            destination: "xtask/src/benchmark.rs",
            evidence: &["port/hunk/benchmarks/README.md"],
            markers: &["benchmark", "Benchmark", "latency"],
        }
    } else if lower.contains("ci") || lower.contains("firecracker") || lower.contains("bun") {
        AreaMapping {
            name: "tooling",
            destination: "xtask/src/main.rs",
            evidence: &[".github/workflows/ci.yml"],
            markers: &["xtask", "cargo"],
        }
    } else if lower.contains("release") || lower.contains("changelog") {
        AreaMapping {
            name: "release",
            destination: "site/data/latest-release.json",
            evidence: &["xtask/src/release_artifacts.rs"],
            markers: &["release", "version"],
        }
    } else if lower.contains("website")
        || lower.contains("sitemap")
        || lower.contains("comparison pages")
    {
        AreaMapping {
            name: "website",
            destination: "site/templates/index.html",
            evidence: &["xtask/src/site_links.rs"],
            markers: &["Workdeck", "site", "documentation"],
        }
    } else if lower.contains("diff") || lower.contains("hunkdiff") {
        AreaMapping {
            name: "diff",
            destination: "crates/workdeck-diff/src/lib.rs",
            evidence: &["crates/workdeck-diff/src/geometry.rs"],
            markers: &["diff", "Diff"],
        }
    } else if lower.contains("docs") || lower.contains("architecture") {
        AreaMapping {
            name: "documentation",
            destination: "docs/upstream-port.md",
            evidence: &["docs/ARCHITECTURE.md"],
            markers: &["Rust", "Hunk"],
        }
    } else {
        AreaMapping {
            name: "tui",
            destination: "crates/workdeck-tui/src/lib.rs",
            evidence: &["crates/workdeck-tui/src/ui_lib_parity_tests.rs"],
            markers: &["TUI", "ratatui", "review"],
        }
    }
}

fn read_ledger(repo: &Path) -> Result<Vec<UpstreamRecord>> {
    let path = repo.join(LEDGER);
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line)
                .with_context(|| format!("parse {} line {}", path.display(), index + 1))
        })
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn git_bytes(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .context("run git")?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    let bytes = git_bytes(repo, args)?;
    Ok(String::from_utf8(bytes)?.trim().to_owned())
}

fn source_patch(repo: &Path, commit: &str) -> Result<Vec<u8>> {
    git_bytes(
        repo,
        &[
            "show",
            "--format=",
            "--no-ext-diff",
            "--binary",
            "--no-renames",
            commit,
        ],
    )
}

fn changed_paths(repo: &Path, commit: &str) -> Result<Vec<u8>> {
    git_bytes(
        repo,
        &[
            "diff-tree",
            "--root",
            "--no-commit-id",
            "--name-only",
            "-r",
            commit,
        ],
    )
}

fn destination_exists(repo: &Path, value: &str) -> Result<()> {
    let path = value.split_once('#').map_or(value, |(path, _)| path);
    let full = repo.join(path);
    ensure!(full.is_file(), "upstream mapping path is missing: {path}");
    Ok(())
}

fn verify_owner(repo: &Path, record: &UpstreamRecord, area: AreaMapping) -> Result<()> {
    ensure!(
        record.destinations.len() == 1,
        "{} must have one native destination",
        record.commit
    );
    ensure!(
        record.destinations[0] == area.destination,
        "{} has a destination for area {}, expected {}",
        record.commit,
        record.area,
        area.destination
    );
    let bytes = fs::read(repo.join(area.destination))
        .with_context(|| format!("read native upstream owner {}", area.destination))?;
    let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    ensure!(
        area.markers
            .iter()
            .any(|marker| text.contains(&marker.to_ascii_lowercase())),
        "{} does not contain a marker for upstream area {}",
        area.destination,
        area.name
    );
    let expected_evidence = area
        .evidence
        .iter()
        .copied()
        .chain(["docs/upstream-port.md"])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ensure!(
        record.evidence == expected_evidence,
        "{} has incomplete or unexpected executable evidence",
        record.commit
    );
    for path in &record.destinations {
        destination_exists(repo, path)?;
    }
    for path in &record.evidence {
        destination_exists(repo, path)?;
    }
    Ok(())
}

fn expected_commits(repo: &Path, baseline: &str) -> Result<Vec<String>> {
    let tip = git_text(repo, &["rev-parse", "--verify", UPSTREAM])?;
    let ancestry = crate::git_output(repo, ["merge-base", "--is-ancestor", baseline, UPSTREAM])?;
    ensure!(
        ancestry.status.success(),
        "Hunk upstream does not descend from the pinned baseline"
    );
    let range = format!("{baseline}..{tip}");
    Ok(
        git_text(repo, &["rev-list", "--reverse", "--topo-order", &range])?
            .lines()
            .map(str::to_owned)
            .collect(),
    )
}

/// Verify every post-baseline Hunk commit and return its authenticated count.
pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<usize> {
    ensure!(
        baseline == EXPECTED_BASELINE,
        "upstream ledger is pinned to {EXPECTED_BASELINE}, got {baseline}"
    );
    let records = read_ledger(repo)?;
    let expected = expected_commits(repo, baseline)?;
    ensure!(
        records.len() == expected.len(),
        "upstream ledger has {} records, expected {}",
        records.len(),
        expected.len()
    );
    let mut seen = BTreeSet::new();
    for (index, (record, commit)) in records.iter().zip(&expected).enumerate() {
        ensure!(
            record.commit == *commit,
            "upstream ledger order mismatch at {}: {}, expected {}",
            index + 1,
            record.commit,
            commit
        );
        ensure!(
            seen.insert(record.commit.as_str()),
            "duplicate upstream ledger commit {}",
            record.commit
        );
        let parents = git_text(repo, &["rev-list", "--parents", "-n", "1", commit])?;
        let fields = parents.split_whitespace().collect::<Vec<_>>();
        ensure!(
            fields.first() == Some(&commit.as_str()),
            "rev-list returned the wrong commit for {}",
            commit
        );
        ensure!(
            fields.len() == 2,
            "{} is a merge commit; semantic port ledger requires one ordered parent",
            commit
        );
        ensure!(
            record.parent == fields[1],
            "{} parent mismatch: {}, expected {}",
            commit,
            record.parent,
            fields[1]
        );
        let subject = git_text(repo, &["show", "-s", "--format=%s", commit])?;
        ensure!(record.subject == subject, "{} subject changed", commit);
        ensure!(
            record.patch_sha256 == sha256(&source_patch(repo, commit)?),
            "{} patch digest mismatch",
            commit
        );
        let paths = changed_paths(repo, commit)?;
        let count = paths
            .split(|byte| *byte == b'\n')
            .filter(|path| !path.is_empty())
            .count();
        ensure!(
            record.changed_path_count == count,
            "{} changed path count mismatch",
            commit
        );
        ensure!(
            record.changed_paths_sha256 == sha256(&paths),
            "{} changed path digest mismatch",
            commit
        );
        let area = mapping(&record.subject);
        ensure!(
            record.area == area.name,
            "{} maps to area {}, expected {}",
            commit,
            record.area,
            area.name
        );
        verify_owner(repo, record, area)?;
    }
    ensure!(
        seen.len() == expected.len(),
        "upstream ledger contains duplicate or missing commits"
    );
    Ok(records.len())
}

/// Return the authenticated post-baseline IDs for queue reconciliation.  This
/// intentionally performs no fallback: callers use it only after `verify` has
/// validated the complete ledger.
pub(crate) fn authenticated_ids(repo: &Path, baseline: &str) -> Result<BTreeSet<String>> {
    let records = read_ledger(repo)?;
    ensure!(
        baseline == EXPECTED_BASELINE,
        "upstream ledger baseline mismatch"
    );
    Ok(records.into_iter().map(|record| record.commit).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_is_specific_and_stable() {
        assert_eq!(mapping("fix(pager): avoid truncation").name, "pager");
        assert_eq!(mapping("fix(watch): reload direct files").name, "watch");
        assert_eq!(
            mapping("feat(extensions): add pane callback").name,
            "extensions"
        );
        assert_eq!(mapping("feat(ui): make history responsive").name, "tui");
        assert_eq!(mapping("chore(release): prepare v0.22.0").name, "release");
    }

    #[test]
    fn digest_is_sha256() {
        assert_eq!(
            sha256(b"workdeck"),
            "31a8a6768568f4d9e7123155c7e38b3d6c5411b27b790d6a10c295cb933e8a5a"
        );
    }
}
