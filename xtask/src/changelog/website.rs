//! Release parsing translated from Hunk's MIT-licensed scripts/generate-changelog.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.

use anyhow::{Result, bail};
use regex::Regex;
use serde::Serialize;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::LazyLock;

static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.]+)?$").unwrap());
static LEGACY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([^\]]+)\]\s*-\s*([0-9]{4}-[0-9]{2}-[0-9]{2})$").unwrap());
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[[^\]]+\]:\s+\S+").unwrap());
static PR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[#([0-9]+)\]\([^)]+\)\s*").unwrap());
static COMMITS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[`[0-9a-f]+`\]\([^)]+\)\s*)+").unwrap());
static DASH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-\s+").unwrap());
static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{7,40}:\s+").unwrap());
static NESTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s+[-*] ").unwrap());

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangeEntry {
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pull_request: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ChangeSection {
    title: String,
    entries: Vec<ChangeEntry>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseEntry {
    version: String,
    prerelease: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    highlights: Option<String>,
    sections: Vec<ChangeSection>,
}

fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| {
        let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
        (
            core.split('.')
                .map(|n| n.parse::<f64>().unwrap_or(0.0))
                .collect::<Vec<_>>(),
            pre.map(str::to_owned),
        )
    };
    let (left, left_pre) = parts(a);
    let (right, right_pre) = parts(b);
    for (a, b) in left.iter().zip(&right) {
        let order = b.partial_cmp(a).unwrap_or(Ordering::Equal);
        if order != Ordering::Equal {
            return order;
        }
    }
    match (left_pre, right_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) => {
            let a = a.split('.').collect::<Vec<_>>();
            let b = b.split('.').collect::<Vec<_>>();
            for i in 0..a.len().max(b.len()) {
                let order = match (a.get(i), b.get(i)) {
                    (None, Some(_)) => Ordering::Greater,
                    (Some(_), None) => Ordering::Less,
                    (Some(a), Some(b)) => {
                        let number = |s: &str| {
                            (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
                                .then(|| s.parse::<f64>().unwrap_or(f64::INFINITY))
                        };
                        match (number(a), number(b)) {
                            (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
                            (Some(_), None) => Ordering::Greater,
                            (None, Some(_)) => Ordering::Less,
                            (None, None) => b.cmp(a),
                        }
                    }
                    _ => Ordering::Equal,
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
            Ordering::Equal
        }
    }
}

fn update_fence(line: &str, fence: &mut Option<char>) {
    let trimmed = line.trim_start();
    let delimiter = if trimmed.starts_with("```") {
        Some('`')
    } else if trimmed.starts_with("~~~") {
        Some('~')
    } else {
        None
    };
    if let Some(delimiter) = delimiter {
        if fence.is_none() {
            *fence = Some(delimiter);
        } else if *fence == Some(delimiter) {
            *fence = None;
        }
    }
}

fn split_headings(markdown: &str, marker: &str) -> Vec<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut fence = None;
    for line in markdown.split('\n') {
        update_fence(line, &mut fence);
        if fence.is_none()
            && let Some(heading) = line.strip_prefix(marker)
        {
            blocks.push(heading.to_owned());
        } else if let Some(block) = blocks.last_mut() {
            block.push('\n');
            block.push_str(line);
        }
    }
    blocks
}

fn parse_entry(raw: &str) -> ChangeEntry {
    let text = raw.trim();
    let captures = PR.captures(text);
    let pull_request = captures.as_ref().and_then(|c| c[1].parse().ok());
    let rest = captures
        .as_ref()
        .map_or(text, |c| &text[c.get(0).unwrap().end()..]);
    let rest = COMMITS.replace(rest, "");
    let rest = DASH.replace(&rest, "");
    let rest = SHA.replace(&rest, "");
    ChangeEntry {
        description: rest.trim().to_owned(),
        pull_request,
    }
}

fn parse_entries(body: &str) -> Vec<ChangeEntry> {
    let mut entries: Vec<String> = Vec::new();
    let mut fence = None;
    for line in body.split('\n') {
        update_fence(line, &mut fence);
        if fence.is_none()
            && let Some(bullet) = line.strip_prefix("- ")
        {
            entries.push(bullet.to_owned());
            continue;
        }
        if fence.is_none() && REFERENCE.is_match(line.trim()) {
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        if fence.is_some() || NESTED.is_match(line) {
            entry.push('\n');
            entry.push_str(line);
        } else if !line.trim().is_empty() {
            entry.push(' ');
            entry.push_str(line.trim());
        }
    }
    entries
        .iter()
        .map(|entry| parse_entry(entry))
        .filter(|entry| !entry.description.is_empty())
        .collect()
}

fn parse_changelog(markdown: &str) -> Vec<ReleaseEntry> {
    let mut releases = Vec::new();
    for block in split_headings(markdown, "## ") {
        let (heading, body) = block.split_once('\n').unwrap_or((&block, ""));
        let heading = heading.trim();
        let legacy = LEGACY.captures(heading);
        let version = legacy
            .as_ref()
            .map_or(heading, |c| c.get(1).unwrap().as_str());
        if !VERSION.is_match(version) {
            continue;
        }
        let mut sections = Vec::new();
        let mut highlights = None;
        for block in split_headings(body, "### ") {
            let (title, body) = block.split_once('\n').unwrap_or((&block, ""));
            let title = title.trim();
            if title == "Highlights" {
                let text = body
                    .split('\n')
                    .filter(|line| !REFERENCE.is_match(line.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_owned();
                highlights = (!text.is_empty()).then_some(text);
            } else {
                let entries = parse_entries(body);
                if !entries.is_empty() {
                    sections.push(ChangeSection {
                        title: title.to_owned(),
                        entries,
                    });
                }
            }
        }
        releases.push(ReleaseEntry {
            version: version.to_owned(),
            prerelease: version.contains('-'),
            heading_date: legacy.map(|c| c[2].to_owned()),
            highlights,
            sections,
        });
    }
    releases.sort_by(|a, b| compare_versions(&a.version, &b.version));
    releases
}

pub(super) fn run(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(path) = args.next() else {
        bail!("changelog parse requires a Markdown file");
    };
    if args.next().is_some() {
        bail!("changelog parse accepts exactly one Markdown file");
    }
    let markdown = std::fs::read_to_string(repo.join(path))?;
    println!(
        "{}",
        serde_json::to_string_pretty(&parse_changelog(&markdown))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_headings_preserve_later_sections_and_matching_delimiters() {
        let releases = parse_changelog(
            "## 0.20.0\n\n### Minor Changes\n\n- [#1](https://x/pull/1) - Example output:\n\n```md\n## Not a heading\n- not a bullet\n```\n\n### Patch Changes\n\n- [#2](https://x/pull/2) - Second entry.\n",
        );
        assert_eq!(releases.len(), 1);
        assert_eq!(
            releases[0]
                .sections
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Minor Changes", "Patch Changes"]
        );
        assert_eq!(releases[0].sections[1].entries[0].pull_request, Some(2));
        let releases =
            parse_changelog("## 0.20.0\n\n### Patch Changes\n\n- One:\n\n~~~\n```\n## x\n~~~\n");
        assert_eq!(releases[0].sections[0].entries.len(), 1);
    }

    #[test]
    fn headings_require_complete_ascii_versions_and_accept_prereleases() {
        assert!(parse_changelog("## 1.2.3 (hotfix) <script>x</script>\n\n- a: b\n").is_empty());
        assert!(parse_changelog("## ١.٢.٣\n### Fixed\n- Not a source version\n").is_empty());
        for version in ["1.2.3", "0.20.0-beta.10"] {
            let releases = parse_changelog(&format!("## {version}\n\n### Fixed\n\n- Real.\n"));
            assert_eq!(releases.len(), 1);
            assert_eq!(releases[0].version, version);
        }
    }

    #[test]
    fn parses_legacy_dates_links_prereleases_and_empty_sections() {
        let input = "## [0.9.0] - 2026-06-13\n### Fixed\n- 59fcdbb: Old fix\n### Empty\n## 0.19.0-beta.1\n### Patch Changes\n- Preview\n## 0.19.0\n### Highlights\nA release\n### Minor Changes\n- [#728](url) [`bb6405e`](url) - Highlight ranges.\n";
        let releases = parse_changelog(input);
        assert_eq!(
            releases
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            ["0.19.0", "0.19.0-beta.1", "0.9.0"]
        );
        assert_eq!(releases[0].highlights.as_deref(), Some("A release"));
        assert_eq!(releases[0].sections[0].entries[0].pull_request, Some(728));
        assert_eq!(
            releases[0].sections[0].entries[0].description,
            "Highlight ranges."
        );
        assert_eq!(releases[2].heading_date.as_deref(), Some("2026-06-13"));
        assert_eq!(releases[2].sections.len(), 1);
        assert_eq!(releases[2].sections[0].entries[0].description, "Old fix");
    }
}
