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

#[derive(Debug, Serialize)]
struct ReleaseSeries {
    minor: String,
    releases: Vec<ReleaseEntry>,
}

fn resolve_tag_date(tagger: &str, commit: &str) -> Option<String> {
    static DATE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}$").unwrap());
    [tagger, commit]
        .into_iter()
        .map(str::trim)
        .find(|date| DATE.is_match(date))
        .map(str::to_owned)
}

fn tag_date(repo: &Path, version: &str) -> Option<String> {
    let read = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(repo)
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).ok())
            .flatten()
    };
    let tagger = read(&[
        "for-each-ref",
        "--format=%(taggerdate:short)",
        &format!("refs/tags/v{version}"),
    ])?;
    let commit = read(&["log", "-1", "--format=%as", &format!("v{version}")])?;
    resolve_tag_date(&tagger, &commit)
}

fn resolve_dates(
    releases: &[ReleaseEntry],
    recorded: &std::collections::BTreeMap<String, String>,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut dates = std::collections::BTreeMap::new();
    for release in releases {
        let date = recorded
            .get(&release.version)
            .cloned()
            .or_else(|| release.heading_date.clone())
            .or_else(|| lookup(&release.version));
        if let Some(date) = date.filter(|date| !date.is_empty()) {
            dates.insert(release.version.clone(), date);
        }
    }
    let mut dates = dates.into_iter().collect::<Vec<_>>();
    dates.sort_by(|(a, _), (b, _)| compare_versions(a, b));
    dates
        .into_iter()
        .map(|(version, date)| (version, serde_json::Value::String(date)))
        .collect()
}

pub(super) fn run_dates(repo: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let args = args.collect::<Vec<_>>();
    let [markdown_path, recorded_path] = args.as_slice() else {
        bail!("changelog dates requires <markdown-file> <recorded-dates.json>");
    };
    let recorded = match std::fs::read(repo.join(recorded_path)) {
        Ok(bytes) => serde_json::from_slice::<std::collections::BTreeMap<String, String>>(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(error) => return Err(error.into()),
    };
    let releases = parse_changelog(&std::fs::read_to_string(repo.join(markdown_path))?);
    let dates = resolve_dates(&releases, &recorded, |version| tag_date(repo, version));
    println!("{}", serde_json::to_string_pretty(&dates)?);
    Ok(())
}

fn group_into_series(releases: Vec<ReleaseEntry>) -> Vec<ReleaseSeries> {
    let mut groups = std::collections::BTreeMap::<String, Vec<ReleaseEntry>>::new();
    for release in releases {
        let minor = release
            .version
            .split('-')
            .next()
            .unwrap_or_default()
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join(".");
        groups.entry(minor).or_default().push(release);
    }
    let mut series = groups
        .into_iter()
        .map(|(minor, mut releases)| {
            releases.sort_by(|a, b| compare_versions(&a.version, &b.version));
            ReleaseSeries { minor, releases }
        })
        .collect::<Vec<_>>();
    series.sort_by(|a, b| compare_versions(&format!("{}.0", a.minor), &format!("{}.0", b.minor)));
    series
}

pub(super) fn run(
    repo: &Path,
    mut args: impl Iterator<Item = String>,
    grouped: bool,
) -> Result<()> {
    let command = if grouped { "series" } else { "parse" };
    let Some(path) = args.next() else {
        bail!("changelog {command} requires a Markdown file");
    };
    if args.next().is_some() {
        bail!("changelog {command} accepts exactly one Markdown file");
    }
    let markdown = std::fs::read_to_string(repo.join(path))?;
    let releases = parse_changelog(&markdown);
    let json = if grouped {
        serde_json::to_string_pretty(&group_into_series(releases))?
    } else {
        serde_json::to_string_pretty(&releases)?
    };
    println!("{json}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_preserve_recorded_values_prefer_headings_and_sort_without_stale_entries() {
        let releases =
            parse_changelog("## 0.19.0\n## 0.18.0\n## [0.15.3] - 2026-06-13\n## 0.14.0\n");
        let recorded = std::collections::BTreeMap::from([
            ("0.19.0".into(), "2020-01-01".into()),
            ("0.99.0".into(), "2026-01-01".into()),
        ]);
        let mut looked_up = Vec::new();
        let dates = resolve_dates(&releases, &recorded, |version| {
            looked_up.push(version.to_owned());
            (version == "0.18.0").then(|| "2026-08-08".into())
        });
        assert_eq!(looked_up, ["0.18.0", "0.14.0"]);
        assert_eq!(
            dates.keys().map(String::as_str).collect::<Vec<_>>(),
            ["0.19.0", "0.18.0", "0.15.3"]
        );
        assert_eq!(dates["0.19.0"], "2020-01-01");
        assert_eq!(dates["0.18.0"], "2026-08-08");
        assert_eq!(dates["0.15.3"], "2026-06-13");
        assert_eq!(
            resolve_tag_date("2026-08-29\n", "2026-08-28\n").as_deref(),
            Some("2026-08-29")
        );
        assert_eq!(
            resolve_tag_date("\n", "2026-08-28\n").as_deref(),
            Some("2026-08-28")
        );
        assert!(resolve_tag_date("not-a-date", "").is_none());
    }

    #[test]
    fn reverse_ordered_release_groups_match_both_pinned_oracles() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-series-oracle.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 2);
        for case in cases {
            let baseline = case["baseline"].as_str().unwrap();
            let source = std::process::Command::new("git")
                .current_dir(repo)
                .args(["show", &format!("{baseline}:CHANGELOG.md")])
                .output()
                .unwrap();
            assert!(source.status.success());
            let mut releases = parse_changelog(std::str::from_utf8(&source.stdout).unwrap());
            releases.reverse();
            let original = releases
                .iter()
                .map(|r| (r.version.clone(), serde_json::to_value(r).unwrap()))
                .collect::<std::collections::BTreeMap<_, _>>();
            let series = group_into_series(releases);
            let actual = series.iter().map(|s| serde_json::json!({"minor":s.minor,"versions":s.releases.iter().map(|r| &r.version).collect::<Vec<_>>()})).collect::<Vec<_>>();
            assert_eq!(serde_json::to_value(actual).unwrap(), case["expected"]);
            for release in series.iter().flat_map(|s| &s.releases) {
                assert_eq!(
                    serde_json::to_value(release).unwrap(),
                    original[&release.version]
                );
            }
        }
        assert!(group_into_series(Vec::new()).is_empty());
    }

    #[test]
    fn complete_pinned_changelogs_match_frozen_source_oracles() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-parse-oracle.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 2);
        for (case, (baseline, count)) in cases.iter().zip([
            ("2c00f4358b89cfc0a6b04459ffc538ba601aa3c2", 48),
            ("4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd", 47),
        ]) {
            assert_eq!(case["baseline"], baseline);
            assert_eq!(case["inputPath"], "CHANGELOG.md");
            let source = std::process::Command::new("git")
                .current_dir(repo)
                .args(["show", &format!("{baseline}:CHANGELOG.md")])
                .output()
                .unwrap();
            assert!(
                source.status.success(),
                "missing pinned changelog {baseline}"
            );
            let releases = parse_changelog(std::str::from_utf8(&source.stdout).unwrap());
            assert_eq!(releases.len(), count);
            assert_eq!(
                serde_json::to_value(releases).unwrap(),
                case["expected"],
                "{baseline}"
            );
        }
    }

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
