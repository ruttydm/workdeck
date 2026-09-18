//! Release parsing translated from Hunk's MIT-licensed scripts/generate-changelog.ts.
//! Copyright (c) Modem Labs Inc. See THIRD_PARTY_NOTICES.

use anyhow::{Result, bail};
use regex::Regex;
use serde::Serialize;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::LazyLock;

mod artifacts;
mod feed;
mod index;
mod json;
mod latest;
mod pages;

fn release_card_metadata(metadata: &mut toml_edit::DocumentMut, card: &serde_json::Value) {
    metadata["template"] = toml_edit::value("release.html");
    metadata["extra"]["social_image"] = toml_edit::value(format!(
        "https://workdeck.dev/changelog/og/{}.png",
        card["slug"].as_str().unwrap()
    ));
    metadata["extra"]["social_image_alt"] = toml_edit::value(card["alt"].as_str().unwrap());
}
pub(super) use artifacts::{run_artifacts, verify_pinned_editorial_inputs};
pub(super) use feed::run_feed;
pub(super) use index::run_index;
pub(super) use latest::run_latest;
pub(super) use pages::run_pages;

// Keep regular-expression whitespace aligned with the pinned JavaScript parser.
fn parser_regex(pattern: &str) -> Regex {
    const SPACE: &str = r"\x09-\x0d\x20\u{00a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";
    Regex::new(
        &pattern
            .replace(r"\S", &format!("[^{SPACE}]"))
            .replace(r"\s", &format!("[{SPACE}]")),
    )
    .unwrap()
}

static VERSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.]+)?$").unwrap());
static LEGACY: LazyLock<Regex> =
    LazyLock::new(|| parser_regex(r"^\[([^\]]+)\]\s*-\s*([0-9]{4}-[0-9]{2}-[0-9]{2})$"));
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| parser_regex(r"^\[[^\]]+\]:\s+\S+"));
static PR: LazyLock<Regex> = LazyLock::new(|| parser_regex(r"^\[#([0-9]+)\]\([^)]+\)\s*"));
static COMMITS: LazyLock<Regex> =
    LazyLock::new(|| parser_regex(r"^(?:\[`[0-9a-f]+`\]\([^)]+\)\s*)+"));
static DASH: LazyLock<Regex> = LazyLock::new(|| parser_regex(r"^-\s+"));
static SHA: LazyLock<Regex> = LazyLock::new(|| parser_regex(r"^[0-9a-f]{7,40}:\s+"));
static NESTED: LazyLock<Regex> = LazyLock::new(|| parser_regex(r"^\s+[-*] "));

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

fn ecmascript_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
        | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

fn update_fence(line: &str, fence: &mut Option<char>) {
    let trimmed = line.trim_start_matches(ecmascript_whitespace);
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
    let text = raw.trim_matches(ecmascript_whitespace);
    let captures = PR.captures(text);
    let pull_request = captures.as_ref().and_then(|c| c[1].parse().ok());
    let rest = captures
        .as_ref()
        .map_or(text, |c| &text[c.get(0).unwrap().end()..]);
    let rest = COMMITS.replace(rest, "");
    let rest = DASH.replace(&rest, "");
    let rest = SHA.replace(&rest, "");
    ChangeEntry {
        description: rest.trim_matches(ecmascript_whitespace).to_owned(),
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
        if fence.is_none() && REFERENCE.is_match(line.trim_matches(ecmascript_whitespace)) {
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        if fence.is_some() || NESTED.is_match(line) {
            entry.push('\n');
            entry.push_str(line);
        } else if !line.trim_matches(ecmascript_whitespace).is_empty() {
            entry.push(' ');
            entry.push_str(line.trim_matches(ecmascript_whitespace));
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
        // JavaScript trim includes BOM but excludes NEL, unlike Rust's trim.
        let heading = heading.trim_matches(ecmascript_whitespace);
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
            let title = title.trim_matches(ecmascript_whitespace);
            if title == "Highlights" {
                let text = body
                    .split('\n')
                    .filter(|line| !REFERENCE.is_match(line.trim_matches(ecmascript_whitespace)))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim_matches(ecmascript_whitespace)
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

fn minor_series_of(version: &str) -> String {
    let mut parts = version.split('-').next().unwrap_or_default().split('.');
    format!(
        "{}.{}",
        parts.next().unwrap_or("0"),
        parts.next().unwrap_or("0")
    )
}

fn version_anchor(version: &str) -> String {
    format!("v{}", version.replace('.', "-"))
}

fn group_into_series(releases: Vec<ReleaseEntry>) -> Vec<ReleaseSeries> {
    let mut groups = std::collections::BTreeMap::<String, Vec<ReleaseEntry>>::new();
    for release in releases {
        let minor = minor_series_of(&release.version);
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

#[derive(Debug, Serialize)]
struct Highlights {
    #[serde(skip_serializing_if = "Option::is_none")]
    lead: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

fn is_published(
    release: &ReleaseEntry,
    dates: &std::collections::BTreeMap<String, String>,
) -> bool {
    dates.contains_key(&release.version)
}

fn is_stable_published(
    release: &ReleaseEntry,
    dates: &std::collections::BTreeMap<String, String>,
) -> bool {
    !release.prerelease && is_published(release, dates)
}

fn publication_state(
    releases: &[ReleaseEntry],
    dates: &std::collections::BTreeMap<String, String>,
) -> serde_json::Value {
    serde_json::json!({
        "published": releases.iter().filter(|r| is_published(r, dates)).map(|r| &r.version).collect::<Vec<_>>(),
        "stable": releases.iter().filter(|r| is_stable_published(r, dates)).map(|r| &r.version).collect::<Vec<_>>(),
        "latestStable": releases.iter().find(|r| is_stable_published(r, dates)).map(|r| &r.version),
    })
}

pub(super) fn run_publication(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(markdown) = args.next() else {
        bail!("changelog publication requires Markdown and dates JSON files");
    };
    let Some(dates) = args.next() else {
        bail!("changelog publication requires Markdown and dates JSON files");
    };
    if args.next().is_some() {
        bail!("changelog publication accepts exactly two files");
    }
    let releases = parse_changelog(&std::fs::read_to_string(repo.join(markdown))?);
    let dates = serde_json::from_str(&std::fs::read_to_string(repo.join(dates))?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&publication_state(&releases, &dates))?
    );
    Ok(())
}

fn radix_date_number(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut significant_bits = 0_u32;
    let mut mantissa = 0_u64;
    let mut round = false;
    let mut sticky = false;
    for c in digits.chars() {
        let Some(digit) = c.to_digit(radix) else {
            return f64::NAN;
        };
        for shift in (0..radix.trailing_zeros()).rev() {
            let bit = (digit >> shift) & 1;
            if significant_bits == 0 && bit == 0 {
                continue;
            }
            significant_bits = (significant_bits + 1).min(1025);
            match significant_bits {
                1..=53 => mantissa = (mantissa << 1) | u64::from(bit),
                54 => round = bit != 0,
                _ => sticky |= bit != 0,
            }
        }
    }
    if significant_bits <= 53 {
        return mantissa as f64;
    }
    if significant_bits > 1024 {
        return f64::INFINITY;
    }
    // Round once, ties to even. Per-digit floating-point accumulation can
    // discard a low bit before later digits establish which way to round.
    if round && (sticky || mantissa & 1 != 0) {
        mantissa += 1;
    }
    (mantissa as f64) * 2_f64.powi(significant_bits as i32 - 53)
}

fn date_number(text: &str) -> f64 {
    let text = text.trim_matches(ecmascript_whitespace);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return radix_date_number(digits, radix);
        }
    }
    static DECIMAL: LazyLock<Regex> = LazyLock::new(|| {
        parser_regex(r"^[+-]?(?:Infinity|(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)$")
    });
    if DECIMAL.is_match(text) {
        text.parse().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

fn format_release_date(iso: &str) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut parts = iso.split('-');
    let year = parts.next().unwrap_or_default();
    let month = date_number(parts.next().unwrap_or_default());
    let day = parts.next().unwrap_or_default();
    match ((1.0..=12.0).contains(&month) && month.fract() == 0.0)
        .then(|| MONTHS[(month as usize) - 1])
    {
        Some(month) if !year.is_empty() && !day.is_empty() => {
            let day = date_number(day);
            let mut buffer = ryu_js::Buffer::new();
            let day = if day.is_nan() {
                "NaN"
            } else if day == f64::INFINITY {
                "Infinity"
            } else if day == f64::NEG_INFINITY {
                "-Infinity"
            } else {
                buffer.format_finite(day)
            };
            format!("{month} {day}, {year}")
        }
        _ => iso.to_owned(),
    }
}

fn render_release_body(
    series: &ReleaseSeries,
    dates: &std::collections::BTreeMap<String, String>,
    repository: &str,
) -> String {
    let mut lines = vec!["## Releases in this series".to_owned(), String::new()];
    for release in &series.releases {
        let meta = dates
            .get(&release.version)
            .filter(|date| !date.is_empty())
            .map(|date| format_release_date(date))
            .unwrap_or_else(|| "Unreleased".into());
        lines.extend([
            format!(
                "<a class=\"release-separator\" id=\"{}\"></a>",
                version_anchor(&release.version)
            ),
            String::new(),
            format!("### {}", release.version),
            String::new(),
            meta,
            String::new(),
        ]);
        if release.sections.is_empty() {
            lines.extend(["No user-facing changes.".into(), String::new()]);
            continue;
        }
        for section in &release.sections {
            lines.extend([format!("#### {}", section.title), String::new()]);
            lines.extend(section.entries.iter().map(|entry| {
                let suffix = entry
                    .pull_request
                    .map(|pr| format!(" ([#{pr}]({repository}/pull/{pr}))"))
                    .unwrap_or_default();
                format!("- {}{suffix}", entry.description)
            }));
            lines.push(String::new());
        }
    }
    lines.join("\n")
}

pub(super) fn run_release_notes(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(markdown) = args.next() else {
        bail!("changelog release-notes requires Markdown and dates JSON files");
    };
    let Some(dates) = args.next() else {
        bail!("changelog release-notes requires Markdown and dates JSON files");
    };
    if args.next().is_some() {
        bail!("changelog release-notes accepts exactly two files");
    }
    let releases = parse_changelog(&std::fs::read_to_string(repo.join(markdown))?);
    let dates = serde_json::from_str(&std::fs::read_to_string(repo.join(dates))?)?;
    let rendered = group_into_series(releases).iter().map(|series| serde_json::json!({
        "minor": series.minor,
        "markdown": render_release_body(series, &dates, "https://github.com/ruttydm/workdeck"),
    })).collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&rendered)?);
    Ok(())
}

fn split_highlights(text: &str) -> Highlights {
    let paragraphs = text.split("\n\n").collect::<Vec<_>>();
    let first = paragraphs[0];
    let bullets_first = first
        .trim_start_matches(ecmascript_whitespace)
        .starts_with('-');
    let lead = (!bullets_first)
        .then(|| first.trim_matches(ecmascript_whitespace).to_owned())
        .filter(|s| !s.is_empty());
    let body = paragraphs[usize::from(!bullets_first)..].join("\n\n");
    let body = body.trim_matches(ecmascript_whitespace);
    Highlights {
        lead,
        body: (!body.is_empty()).then(|| body.to_owned()),
    }
}

fn to_plain_text(markdown: &str) -> String {
    static LINKS: LazyLock<Regex> = LazyLock::new(|| parser_regex(r"\[([^\]]+)\]\([^)]*\)"));
    static SPACES: LazyLock<Regex> = LazyLock::new(|| parser_regex(r"\s+"));
    let text = LINKS
        .replace_all(markdown, "$1")
        .replace("**", "")
        .replace('`', "");
    SPACES
        .replace_all(&text, " ")
        .trim_matches(ecmascript_whitespace)
        .to_owned()
}

fn series_summary(series: &ReleaseSeries, overlay: Option<&str>) -> Option<String> {
    if let Some(summary) = overlay.filter(|s| !s.is_empty()) {
        return Some(summary.to_owned());
    }
    series
        .releases
        .iter()
        .filter_map(|r| r.highlights.as_deref())
        .find_map(|text| split_highlights(text).lead)
        .map(|lead| to_plain_text(&lead))
}

#[derive(serde::Deserialize)]
struct ReleaseVideo {
    mp4: String,
    webm: Option<String>,
    poster: Option<String>,
    duration: Option<String>,
    title: Option<String>,
}

fn html_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

fn render_video(
    video: &ReleaseVideo,
    minor: &str,
    summary: &str,
    product: &str,
    origin: &str,
) -> String {
    let mut schema = serde_json::Map::new();
    schema.insert("@context".into(), "https://schema.org".into());
    schema.insert("@type".into(), "VideoObject".into());
    schema.insert(
        "name".into(),
        video
            .title
            .clone()
            .unwrap_or_else(|| format!("What's new in {product} {minor}"))
            .into(),
    );
    schema.insert("description".into(), summary.into());
    schema.insert("contentUrl".into(), video.mp4.clone().into());
    if let Some(poster) = video.poster.as_deref().filter(|s| !s.is_empty()) {
        schema.insert("thumbnailUrl".into(), format!("{origin}{poster}").into());
    }
    if let Some(duration) = video.duration.as_deref().filter(|s| !s.is_empty()) {
        schema.insert("duration".into(), duration.into());
    }
    let poster = video
        .poster
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!(" poster=\"{}\"", html_attribute(s)))
        .unwrap_or_default();
    let mut lines = vec![format!(
        "<video class=\"changelog-video\" controls preload=\"none\"{poster}>"
    )];
    if let Some(webm) = video.webm.as_deref().filter(|s| !s.is_empty()) {
        lines.push(format!(
            "    <source src=\"{}\" type=\"video/webm\" />",
            html_attribute(webm)
        ));
    }
    lines.push(format!(
        "    <source src=\"{}\" type=\"video/mp4\" />",
        html_attribute(&video.mp4)
    ));
    lines.extend(["  </video>".into(), String::new()]);
    let json = serde_json::to_string(&schema)
        .unwrap()
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028");
    lines.push(format!(
        "<script type=\"application/ld+json\" is:inline>{json}</script>"
    ));
    lines.join("\n")
}

pub(super) fn run_video(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(path) = args.next() else {
        bail!("changelog video requires a JSON input file");
    };
    if args.next().is_some() {
        bail!("changelog video accepts exactly one JSON input file");
    }
    #[derive(serde::Deserialize)]
    struct Input {
        minor: String,
        summary: String,
        video: ReleaseVideo,
    }
    let input: Input = serde_json::from_str(&std::fs::read_to_string(repo.join(path))?)?;
    println!(
        "{}",
        render_video(
            &input.video,
            &input.minor,
            &input.summary,
            "Workdeck",
            "https://workdeck.dev"
        )
    );
    Ok(())
}

fn series_card(
    series: &ReleaseSeries,
    overlay: Option<&str>,
    dates: &std::collections::BTreeMap<String, String>,
    latest: bool,
    product: &str,
) -> serde_json::Value {
    let published = series
        .releases
        .iter()
        .filter(|r| is_published(r, dates))
        .collect::<Vec<_>>();
    let prerelease_only = !published.is_empty() && !published.iter().any(|r| !r.prerelease);
    let releases = series.releases.len();
    let changes: usize = series
        .releases
        .iter()
        .flat_map(|r| &r.sections)
        .map(|s| s.entries.len())
        .sum();
    let meta = format!(
        "{}{} · {releases} release{} · {changes} change{}",
        if prerelease_only {
            "Prerelease · "
        } else {
            ""
        },
        series_span(series, dates).unwrap_or_else(|| "Unreleased".into()),
        if releases == 1 { "" } else { "s" },
        if changes == 1 { "" } else { "s" }
    );
    let mut card = serde_json::Map::new();
    card.insert("slug".into(), series.minor.clone().into());
    card.insert("title".into(), format!("{product} {}", series.minor).into());
    if let Some(tagline) = series_summary(series, overlay).filter(|s| !s.is_empty()) {
        card.insert(
            "tagline".into(),
            truncate_description(&to_plain_text(&tagline), 140).into(),
        );
    }
    card.insert("meta".into(), meta.clone().into());
    if published.len() > 1 {
        card.insert(
            "chips".into(),
            serde_json::json!(published.iter().map(|r| &r.version).collect::<Vec<_>>()),
        );
    }
    if latest {
        card.insert("latest".into(), true.into());
    }
    card.insert(
        "alt".into(),
        format!("{product} {} release notes — {meta}", series.minor).into(),
    );
    card.into()
}

fn month_and_year(iso: &str) -> String {
    let mut parts = iso.split('-');
    let year = parts.next().unwrap_or_default();
    let month = date_number(parts.next().unwrap_or_default());
    if !year.is_empty() && (1.0..=12.0).contains(&month) && month.fract() == 0.0 {
        let date = format_release_date(&format!("{year}-{}-1", month as usize));
        date.replacen(" 1,", "", 1)
    } else {
        iso.to_owned()
    }
}

fn index_card(
    series: &[ReleaseSeries],
    dates: &std::collections::BTreeMap<String, String>,
    product: &str,
) -> serde_json::Value {
    let published = series
        .iter()
        .filter(|s| s.releases.iter().any(|r| is_published(r, dates)))
        .collect::<Vec<_>>();
    let mut all_dates = published
        .iter()
        .flat_map(|s| {
            s.releases
                .iter()
                .filter(|r| is_published(r, dates))
                .map(|r| dates[&r.version].as_str())
        })
        .collect::<Vec<_>>();
    all_dates.sort();
    let span = all_dates
        .first()
        .zip(all_dates.last())
        .filter(|(a, b)| !a.is_empty() && !b.is_empty())
        .map(|(a, b)| format!(" · {} – {}", month_and_year(a), month_and_year(b)))
        .unwrap_or_default();
    let meta = format!("{} release series{span}", published.len());
    let mut card = serde_json::json!({"slug":"index","title":"Changelog","tagline":format!("Every {product} release, grouped by minor series."),"meta":meta});
    let mut chips = published
        .iter()
        .take(5)
        .map(|s| s.minor.as_str())
        .collect::<Vec<_>>();
    if published.len() > chips.len() {
        chips.push("…");
    }
    if !chips.is_empty() {
        card["chips"] = serde_json::json!(chips);
    }
    card["alt"] = format!("{product} changelog — {meta}").into();
    card
}

pub(super) fn run_index_card(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(markdown) = args.next() else {
        bail!("changelog index-card requires Markdown and dates JSON files");
    };
    let Some(dates) = args.next() else {
        bail!("changelog index-card requires Markdown and dates JSON files");
    };
    if args.next().is_some() {
        bail!("changelog index-card accepts exactly two files");
    }
    let series = group_into_series(parse_changelog(&std::fs::read_to_string(
        repo.join(markdown),
    )?));
    let dates = serde_json::from_str(&std::fs::read_to_string(repo.join(dates))?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&index_card(&series, &dates, "Workdeck"))?
    );
    Ok(())
}

fn yaml_string(value: &str) -> String {
    if value.contains('"') && !value.contains('\'') {
        format!("'{value}'")
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

// Generated files encode JavaScript strings as UTF-8. If slicing bisects a
// surrogate pair, the source writer emits the replacement character too.
fn truncate_description(text: &str, limit: usize) -> String {
    let units = text.encode_utf16().collect::<Vec<_>>();
    if units.len() <= limit {
        return text.to_owned();
    }
    let clipped = &units[..limit];
    let end = clipped
        .iter()
        .rposition(|c| *c == u16::from(b' '))
        .filter(|pos| *pos > 0)
        .unwrap_or(clipped.len());
    let body = String::from_utf16_lossy(&clipped[..end]);
    format!(
        "{}…",
        body.trim_end_matches(|c| matches!(c, ',' | ';' | ':' | '.') || ecmascript_whitespace(c))
    )
}

pub(super) fn run_metadata(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let Some(markdown) = args.next() else {
        bail!("changelog metadata requires Markdown and dates JSON files");
    };
    let Some(dates) = args.next() else {
        bail!("changelog metadata requires Markdown and dates JSON files");
    };
    if args.next().is_some() {
        bail!("changelog metadata accepts exactly two files");
    }
    let releases = parse_changelog(&std::fs::read_to_string(repo.join(markdown))?);
    let dates = serde_json::from_str(&std::fs::read_to_string(repo.join(dates))?)?;
    let metadata = group_into_series(releases).iter().map(|series| {
        let description = truncate_description(&to_plain_text(&resolve_summary(series, None, &dates, "Workdeck")), 155);
        serde_json::json!({"minor": series.minor, "description": description, "yamlDescription": yaml_string(&description)})
    }).collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&metadata)?);
    Ok(())
}

fn series_span(
    series: &ReleaseSeries,
    dates: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let published = series
        .releases
        .iter()
        .filter(|r| is_published(r, dates))
        .map(|r| dates[&r.version].as_str())
        .collect::<Vec<_>>();
    let newest = *published.first()?;
    let oldest = *published.last()?;
    if newest.is_empty() || oldest.is_empty() {
        return None;
    }
    Some(if newest == oldest {
        format_release_date(newest)
    } else {
        format!(
            "{} – {}",
            format_release_date(oldest),
            format_release_date(newest)
        )
    })
}

fn resolve_summary(
    series: &ReleaseSeries,
    overlay: Option<&str>,
    dates: &std::collections::BTreeMap<String, String>,
    product: &str,
) -> String {
    series_summary(series, overlay).unwrap_or_else(|| {
        let count = series.releases.len();
        let plural = if count == 1 { "" } else { "s" };
        let span = series_span(series, dates)
            .map(|span| format!(", {span}"))
            .unwrap_or_default();
        format!(
            "Release notes for {product} {}: {count} release{plural}{span}.",
            series.minor
        )
    })
}

pub(super) fn run_summaries(
    repo: &Path,
    mut args: impl Iterator<Item = String>,
    resolved: bool,
) -> Result<()> {
    let Some(path) = args.next() else {
        bail!("changelog summaries requires a Markdown file");
    };
    let dates = if resolved {
        let Some(path) = args.next() else {
            bail!("changelog resolved-summaries requires a dates JSON file");
        };
        Some(serde_json::from_str::<
            std::collections::BTreeMap<String, String>,
        >(&std::fs::read_to_string(repo.join(path))?)?)
    } else {
        None
    };
    let notes = args.next();
    if args.next().is_some() {
        bail!("changelog summaries accepts a Markdown file and optional notes JSON file");
    }
    #[derive(serde::Deserialize)]
    struct Notes {
        summary: Option<String>,
    }
    let notes: std::collections::BTreeMap<String, Notes> = match notes {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(repo.join(path))?)?,
        None => Default::default(),
    };
    let markdown = std::fs::read_to_string(repo.join(path))?;
    let summaries = group_into_series(parse_changelog(&markdown))
        .iter()
        .map(|series| {
            let overlay = notes.get(&series.minor).and_then(|n| n.summary.as_deref());
            let summary = match &dates {
                Some(dates) => Some(resolve_summary(series, overlay, dates, "Workdeck")),
                None => series_summary(series, overlay),
            };
            serde_json::json!({"minor": series.minor, "summary": summary})
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
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
    fn pinned_release_dates_are_valid_input_for_native_changelog_commands() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let bytes = crate::git_stdout_bytes(
            repo,
            [
                "show",
                "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:website/releases/dates.json",
            ],
        )
        .unwrap();
        assert_eq!(bytes.len(), 1248);
        let dates: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&bytes).unwrap();
        assert_eq!(dates.len(), 48);
        assert_eq!(dates["0.20.1"], "2026-08-29");
        assert_eq!(dates["0.1.0"], "2026-03-20");
        assert!(dates.values().all(|date| {
            let bytes = date.as_bytes();
            bytes.len() == 10
                && bytes[4] == b'-'
                && bytes[7] == b'-'
                && bytes
                    .iter()
                    .enumerate()
                    .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
        }));
    }

    #[test]
    fn index_card_counts_and_chips_match_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-index-card-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 5);
            for case in cases {
                let series = group_into_series(parse_changelog(case["input"].as_str().unwrap()));
                let dates = serde_json::from_value(case["dates"].clone()).unwrap();
                assert_eq!(index_card(&series, &dates, "Hunk"), case["expected"]);
            }
        }
    }

    #[test]
    fn prerelease_series_cards_match_main_oracle() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-prerelease-card-oracle.json"
        ))
        .unwrap();
        assert_eq!(
            fixture["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 6);
        for case in cases {
            let series = group_into_series(parse_changelog(case["input"].as_str().unwrap()))
                .pop()
                .unwrap();
            let dates = serde_json::from_value(case["dates"].clone()).unwrap();
            assert_eq!(
                series_card(
                    &series,
                    Some("An **editorial** summary."),
                    &dates,
                    false,
                    "Hunk"
                ),
                case["expected"]
            );
        }
    }

    #[test]
    fn stable_series_cards_match_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-card-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 8);
            for case in cases {
                let series = group_into_series(parse_changelog(case["input"].as_str().unwrap()))
                    .pop()
                    .unwrap();
                let dates = serde_json::from_value(case["dates"].clone()).unwrap();
                assert_eq!(
                    series_card(
                        &series,
                        None,
                        &dates,
                        case["latest"].as_bool().unwrap(),
                        "Hunk"
                    ),
                    case["expected"]
                );
            }
        }
    }

    fn original_series_page(latest: Option<&str>, neighbors: bool) -> String {
        let series = group_into_series(parse_changelog(SOURCE_SAMPLE));
        let dates = std::collections::BTreeMap::from([
            ("0.19.0".into(), "2026-08-16".into()),
            ("0.19.0-beta.0".into(), "2026-08-10".into()),
        ]);
        pages::render_page(
            &series[0],
            &Default::default(),
            &dates,
            neighbors.then_some("0.20"),
            neighbors.then_some("0.18"),
            latest,
        )
        .unwrap()
    }

    #[test]
    fn original_page_marks_current_only_for_latest_release() {
        assert!(
            original_series_page(Some("0.19.0"), false).contains("This is the current release")
        );
        assert!(
            original_series_page(Some("0.20.0"), false).contains("no longer the current release")
        );
    }

    #[test]
    fn original_page_emits_stable_release_anchor() {
        assert!(
            original_series_page(None, false)
                .contains("<a class=\"release-separator\" id=\"v0-19-0\"></a>")
        );
    }

    #[test]
    fn original_page_does_not_repeat_lead_inside_highlights() {
        let page = original_series_page(None, false);
        let highlights = page
            .split("## Highlights")
            .nth(1)
            .unwrap()
            .split("\n## ")
            .next()
            .unwrap();
        assert!(highlights.contains("**Richer extensions.**"));
        assert!(!highlights.contains("expands the extension platform"));
    }

    #[test]
    fn original_page_links_neighboring_series() {
        let page = original_series_page(None, true);
        assert!(page.contains("[Older: Workdeck 0.18](/changelog/0.18/)"));
        assert!(page.contains("[Newer: Workdeck 0.20](/changelog/0.20/)"));
    }

    fn source_video_page(video: serde_json::Value) -> String {
        let series = group_into_series(parse_changelog(SOURCE_SAMPLE));
        let dates = std::collections::BTreeMap::from([("0.19.0".into(), "2026-08-16".into())]);
        let notes = serde_json::from_value(serde_json::json!({"video":video})).unwrap();
        pages::render_page(&series[0], &notes, &dates, None, None, None).unwrap()
    }

    #[test]
    fn source_video_page_contains_source_and_schema() {
        let fragment = source_video_page(serde_json::json!({"mp4":"/v.mp4"}));
        assert!(fragment.contains("<source src=\"/v.mp4\" type=\"video/mp4\" />"));
        assert!(fragment.contains("application/ld+json"));
        assert!(fragment.contains("\"@type\":\"VideoObject\""));
    }

    #[test]
    fn source_video_page_escapes_script_closing_title() {
        let fragment = source_video_page(
            serde_json::json!({"mp4":"/v.mp4", "title":"</script><script>alert(1)</script>"}),
        );
        assert!(!fragment.contains("</script><script>alert(1)"));
        assert!(fragment.contains("\\u003c/script"));
    }

    #[test]
    fn source_video_page_escapes_attribute_breakout_url() {
        let fragment = source_video_page(serde_json::json!({"mp4":"/v.mp4\" onerror=\"x"}));
        assert!(!fragment.contains("onerror=\"x\""));
        assert!(fragment.contains("&quot;"));
    }

    #[test]
    fn video_fragments_match_both_pinned_page_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-video-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 4);
            for case in cases {
                let video = serde_json::from_value(case["video"].clone()).unwrap();
                assert_eq!(
                    render_video(
                        &video,
                        case["minor"].as_str().unwrap(),
                        case["summary"].as_str().unwrap(),
                        "Hunk",
                        "https://hunk.dev"
                    ),
                    case["expected"]
                );
            }
        }
    }

    #[test]
    fn source_text_reduces_markdown_to_plain_text() {
        assert_eq!(
            to_plain_text("Use **`hunk diff`** and [the docs](/docs/)."),
            "Use hunk diff and the docs."
        );
    }

    #[test]
    fn source_text_truncates_on_word_boundary() {
        assert_eq!(truncate_description("one two three four", 12), "one two…");
    }

    #[test]
    fn source_text_leaves_short_text_alone() {
        assert_eq!(truncate_description("short", 12), "short");
    }

    #[test]
    fn source_text_quotes_frontmatter_like_source_formatter() {
        // Generated scalars must retain the source formatter's quote choice.
        assert_eq!(yaml_string("plain text"), "\"plain text\"");
        assert_eq!(
            yaml_string("vcs = \"jj\" support"),
            "'vcs = \"jj\" support'"
        );
        assert_eq!(
            yaml_string("Hunk's \"jj\" support"),
            "\"Hunk's \\\"jj\\\" support\""
        );
    }

    #[test]
    fn source_text_formats_iso_date() {
        assert_eq!(format_release_date("2026-08-16"), "August 16, 2026");
    }

    #[test]
    fn description_encoding_matches_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-description-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 50);
            for case in cases {
                let input = case["input"].as_str().unwrap();
                assert_eq!(yaml_string(input), case["yaml"]);
                assert_eq!(
                    truncate_description(input, case["limit"].as_u64().unwrap() as usize),
                    case["truncatedUtf8"]
                );
            }
        }
    }

    #[test]
    fn source_versions_order_releases_newest_first() {
        let mut versions = ["0.9.0", "0.18.2", "0.10.0"];
        versions.sort_by(|a, b| compare_versions(a, b));
        assert_eq!(versions, ["0.18.2", "0.10.0", "0.9.0"]);
    }

    #[test]
    fn source_versions_sort_stable_above_own_prereleases() {
        let mut versions = ["0.19.0-beta.0", "0.19.0", "0.19.0-beta.1"];
        versions.sort_by(|a, b| compare_versions(a, b));
        assert_eq!(versions, ["0.19.0", "0.19.0-beta.1", "0.19.0-beta.0"]);
    }

    #[test]
    fn source_versions_derive_minor_series() {
        assert_eq!(minor_series_of("0.18.2"), "0.18");
        assert_eq!(minor_series_of("0.19.0-beta.0"), "0.19");
    }

    #[test]
    fn source_versions_build_readable_anchor() {
        assert_eq!(version_anchor("0.18.2"), "v0-18-2");
    }

    #[test]
    fn source_dates_prefer_annotated_tag_and_fall_back_to_commit() {
        assert_eq!(
            resolve_tag_date("2026-08-29\n", "2026-08-28\n").as_deref(),
            Some("2026-08-29")
        );
        assert_eq!(
            resolve_tag_date("\n", "2026-08-28\n").as_deref(),
            Some("2026-08-28")
        );
        assert_eq!(resolve_tag_date("not-a-date", ""), None);
    }

    #[test]
    fn source_dates_preserve_recorded_date() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        let recorded = std::collections::BTreeMap::from([("0.19.0".into(), "2020-01-01".into())]);
        let resolved = resolve_dates(&releases, &recorded, |_| Some("2099-12-31".into()));
        assert_eq!(resolved["0.19.0"], "2020-01-01");
    }

    #[test]
    fn source_dates_fill_missing_from_lookup() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        let resolved = resolve_dates(&releases, &Default::default(), |version| {
            (version == "0.19.0").then(|| "2026-08-16".into())
        });
        assert_eq!(resolved["0.19.0"], "2026-08-16");
    }

    #[test]
    fn source_dates_legacy_heading_precedes_lookup() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        let resolved = resolve_dates(&releases, &Default::default(), |_| {
            Some("2099-12-31".into())
        });
        assert_eq!(resolved["0.15.3"], "2026-06-13");
    }

    #[test]
    fn source_dates_unresolved_version_remains_absent() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        assert!(
            resolve_dates(&releases, &Default::default(), |_| None)
                .get("0.19.0")
                .is_none()
        );
    }

    #[test]
    fn source_dates_recorded_keys_are_newest_first() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        let recorded = std::collections::BTreeMap::from([
            ("0.18.0".into(), "2026-08-08".into()),
            ("0.19.0".into(), "2026-08-16".into()),
        ]);
        let resolved = resolve_dates(&releases, &recorded, |_| None);
        assert_eq!(
            resolved.keys().map(String::as_str).collect::<Vec<_>>(),
            ["0.19.0", "0.18.0", "0.15.3"]
        );
    }

    const SOURCE_SAMPLE: &str = include_str!("../../../port/hunk/website-changelog-test-sample.md");

    #[test]
    fn source_sample_parses_every_release_newest_first() {
        assert_eq!(
            parse_changelog(SOURCE_SAMPLE)
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            ["0.19.0", "0.19.0-beta.0", "0.18.0", "0.15.3"]
        );
    }

    #[test]
    fn source_sample_marks_prereleases() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        assert!(
            releases
                .iter()
                .find(|r| r.version == "0.19.0-beta.0")
                .unwrap()
                .prerelease
        );
        assert!(
            !releases
                .iter()
                .find(|r| r.version == "0.19.0")
                .unwrap()
                .prerelease
        );
    }

    #[test]
    fn source_sample_captures_legacy_heading_date() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        assert_eq!(
            releases
                .iter()
                .find(|r| r.version == "0.15.3")
                .unwrap()
                .heading_date
                .as_deref(),
            Some("2026-06-13")
        );
    }

    #[test]
    fn source_sample_lifts_highlights_out_of_sections() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        let release = releases.iter().find(|r| r.version == "0.19.0").unwrap();
        assert!(
            release
                .highlights
                .as_ref()
                .unwrap()
                .contains("expands the extension platform")
        );
        assert_eq!(
            release
                .sections
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Minor Changes"]
        );
    }

    #[test]
    fn source_sample_drops_empty_legacy_sections() {
        let releases = parse_changelog(SOURCE_SAMPLE);
        let release = releases.iter().find(|r| r.version == "0.15.3").unwrap();
        assert_eq!(
            release
                .sections
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Added", "Fixed"]
        );
    }

    #[test]
    fn source_sample_groups_series_newest_first() {
        let series = group_into_series(parse_changelog(SOURCE_SAMPLE));
        assert_eq!(
            series.iter().map(|s| s.minor.as_str()).collect::<Vec<_>>(),
            ["0.19", "0.18", "0.15"]
        );
        assert_eq!(
            series[0]
                .releases
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            ["0.19.0", "0.19.0-beta.0"]
        );
    }

    #[test]
    fn source_sample_highlights_separates_lead_and_bullets() {
        assert_eq!(
            serde_json::to_value(split_highlights("A lead sentence.\n\n- **One.** Detail."))
                .unwrap(),
            serde_json::json!({"lead":"A lead sentence.", "body":"- **One.** Detail."})
        );
    }

    #[test]
    fn source_sample_highlights_bullet_only_has_no_lead() {
        assert_eq!(
            serde_json::to_value(split_highlights("- **One.** Detail.")).unwrap(),
            serde_json::json!({"body":"- **One.** Detail."})
        );
    }

    #[test]
    fn source_sample_highlights_lead_becomes_summary() {
        let series = group_into_series(parse_changelog(SOURCE_SAMPLE));
        assert_eq!(
            series_summary(&series[0], None).as_deref(),
            Some("Hunk 0.19.0 expands the extension platform.")
        );
    }

    #[test]
    fn source_sample_highlights_editorial_summary_wins() {
        let series = group_into_series(parse_changelog(SOURCE_SAMPLE));
        assert_eq!(
            series_summary(&series[0], Some("Overlay wins.")).as_deref(),
            Some("Overlay wins.")
        );
    }

    #[test]
    fn source_sample_highlights_absent_means_no_summary() {
        let series = group_into_series(parse_changelog(SOURCE_SAMPLE));
        assert_eq!(
            series_summary(series.iter().find(|s| s.minor == "0.15").unwrap(), None),
            None
        );
    }

    #[test]
    fn published_policy_preserves_dated_prereleases_without_stable_promotion() {
        let dated = ReleaseEntry {
            version: "0.19.0".into(),
            prerelease: false,
            heading_date: None,
            highlights: None,
            sections: vec![],
        };
        let prerelease = ReleaseEntry {
            version: "0.19.0-beta.0".into(),
            prerelease: true,
            heading_date: None,
            highlights: None,
            sections: vec![],
        };
        let dates = std::collections::BTreeMap::from([("0.19.0".into(), "2026-08-16".into())]);
        let beta_dates =
            std::collections::BTreeMap::from([("0.19.0-beta.0".into(), "2026-08-10".into())]);
        assert!(is_published(&dated, &dates));
        assert!(!is_published(&dated, &Default::default()));
        assert!(is_published(&prerelease, &beta_dates));
        assert!(is_stable_published(&dated, &dates));
        assert!(!is_stable_published(&prerelease, &beta_dates));
    }

    #[test]
    fn prerelease_comparison_has_defined_order_for_source_nan_regressions() {
        // Ordering cannot represent NaN; also check the exact ordering of each
        // source regression pair rather than only its finite-number property.
        for (a, b, expected) in [
            ("0.19.0-rc", "0.19.0", Ordering::Greater),
            ("0.19.0-rc", "0.19.0-beta.1", Ordering::Less),
            ("0.19.0", "0.19.0", Ordering::Equal),
        ] {
            assert_eq!(compare_versions(a, b), expected);
        }
    }

    #[test]
    fn prerelease_ordering_places_stable_above_unnumbered_channel() {
        let mut versions = ["0.19.0-rc", "0.19.0-beta.1", "0.19.0"];
        versions.sort_by(|a, b| compare_versions(a, b));
        assert_eq!(versions, ["0.19.0", "0.19.0-rc", "0.19.0-beta.1"]);
    }

    #[test]
    fn prerelease_ordering_compares_channels_and_numbers() {
        let mut versions = ["0.19.0-alpha.99", "0.19.0-beta.2", "0.19.0-beta.10"];
        versions.sort_by(|a, b| compare_versions(a, b));
        assert_eq!(
            versions,
            ["0.19.0-beta.10", "0.19.0-beta.2", "0.19.0-alpha.99"]
        );
    }

    // Legacy compare-link reference tables belong to no release. Do not fold
    // them into the oldest release's prose, but retain fenced syntax examples.
    #[test]
    fn references_stay_out_of_last_entry() {
        let input = [
            "## 0.1.0",
            "",
            "### Fixed",
            "",
            "- Stabilized diff repainting.",
            "",
            "[Unreleased]: https://github.com/modem-dev/hunk/compare/v0.15.3...HEAD",
            "[0.1.0]: https://github.com/modem-dev/hunk/tree/v0.1.0",
            "",
        ]
        .join("\n");
        let releases = parse_changelog(&input);
        assert_eq!(
            serde_json::to_value(&releases[0].sections[0].entries).unwrap(),
            serde_json::json!([{"description":"Stabilized diff repainting."}])
        );
    }

    #[test]
    fn references_stay_out_of_highlights() {
        let input = [
            "## 0.1.0",
            "",
            "### Highlights",
            "",
            "A lead.",
            "",
            "[0.1.0]: https://x/tree/v0.1.0",
            "",
        ]
        .join("\n");
        let releases = parse_changelog(&input);
        assert_eq!(releases[0].highlights.as_deref(), Some("A lead."));
    }

    #[test]
    fn references_remain_inside_fenced_content() {
        let input = [
            "## 0.1.0",
            "",
            "### Fixed",
            "",
            "- Document the reference syntax:",
            "",
            "```md",
            "[label]: https://example.com",
            "```",
            "",
        ]
        .join("\n");
        let releases = parse_changelog(&input);
        assert!(
            releases[0].sections[0].entries[0]
                .description
                .contains("[label]: https://example.com")
        );
    }

    // Direct translations of Hunk's parser robustness regressions: preserve
    // legitimate changeset content and reject malformed version headings.
    #[test]
    fn robustness_fenced_heading_preserves_following_section() {
        let releases = parse_changelog(
            &[
                "## 0.20.0",
                "",
                "### Minor Changes",
                "",
                "- [#1](https://x/pull/1) - Example output:",
                "",
                "```md",
                "## Not a heading",
                "- not a bullet",
                "```",
                "",
                "### Patch Changes",
                "",
                "- [#2](https://x/pull/2) - Second entry.",
                "",
            ]
            .join("\n"),
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
    }

    #[test]
    fn robustness_fence_closes_only_matching_delimiter() {
        let releases = parse_changelog(
            &[
                "## 0.20.0",
                "",
                "### Patch Changes",
                "",
                "- One:",
                "",
                "~~~",
                "```",
                "## x",
                "~~~",
                "",
            ]
            .join("\n"),
        );
        assert_eq!(releases[0].sections[0].entries.len(), 1);
    }

    #[test]
    fn robustness_rejects_partial_version_heading() {
        assert!(parse_changelog("## 1.2.3 (hotfix) <script>x</script>\n\n- a: b\n").is_empty());
        assert_eq!(
            parse_changelog("## 1.2.3\n\n### Fixed\n\n- Real.\n")
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            ["1.2.3"]
        );
    }

    #[test]
    fn robustness_accepts_prerelease_heading() {
        assert_eq!(
            parse_changelog("## 0.20.0-beta.10\n\n### Fixed\n\n- Real.\n")
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            ["0.20.0-beta.10"]
        );
    }

    #[test]
    fn robustness_preserves_nested_sublist() {
        let releases = parse_changelog(
            &[
                "## 0.20.0",
                "",
                "### Minor Changes",
                "",
                "- Adds:",
                "  - one",
                "  - two",
                "",
            ]
            .join("\n"),
        );
        let description = &releases[0].sections[0].entries[0].description;
        assert!(description.contains("\n  - one"));
        assert!(!description.contains("Adds: - one"));
    }

    #[test]
    fn entry_recovers_pull_request_from_changesets() {
        assert_eq!(serde_json::to_value(parse_entry("[#728](https://github.com/modem-dev/hunk/pull/728) [`bb6405e`](https://github.com/modem-dev/hunk/commit/bb6405e) - Highlight ranges.")).unwrap(), serde_json::json!({"description":"Highlight ranges.","pullRequest":728}));
    }

    #[test]
    fn entry_handles_commit_link_without_pull_request() {
        assert_eq!(
            serde_json::to_value(parse_entry(
                "[`f1bc9bf`](https://github.com/modem-dev/hunk/commit/f1bc9bf) - Move keys."
            ))
            .unwrap(),
            serde_json::json!({"description":"Move keys."})
        );
    }

    #[test]
    fn entry_strips_legacy_bare_sha() {
        assert_eq!(
            serde_json::to_value(parse_entry("59fcdbb: Require an explicit click.")).unwrap(),
            serde_json::json!({"description":"Require an explicit click."})
        );
    }

    #[test]
    fn entry_keeps_plain_prose() {
        assert_eq!(
            serde_json::to_value(parse_entry("Fixed Windows launches.")).unwrap(),
            serde_json::json!({"description":"Fixed Windows launches."})
        );
    }

    #[test]
    fn large_radix_dates_match_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-radix-date-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 8);
            for case in cases {
                assert_eq!(
                    format_release_date(case["input"].as_str().unwrap()),
                    case["expected"],
                    "{}",
                    case["input"]
                );
            }
        }
    }

    #[test]
    fn resolved_summaries_match_both_pins_with_explicit_publication_rules() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-resolved-summary-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for (index, result) in results.iter().enumerate() {
            assert_eq!(result["includesPrereleases"], index == 0);
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 16);
            for case in cases {
                let mut series =
                    group_into_series(parse_changelog(case["input"].as_str().unwrap()))
                        .pop()
                        .unwrap();
                let mut dates: std::collections::BTreeMap<String, String> =
                    serde_json::from_value(case["dates"].clone()).unwrap();
                // Stable's generator filters prereleases before grouping, and its
                // factual helper excludes them from both counts and date spans.
                if index == 1 {
                    for release in &series.releases {
                        if release.prerelease {
                            dates.remove(&release.version);
                        }
                    }
                    series.releases.retain(|release| !release.prerelease);
                }
                assert_eq!(
                    resolve_summary(&series, None, &dates, "Hunk"),
                    case["expected"]
                );
                assert_eq!(
                    resolve_summary(&series, Some("Editorial **unchanged**"), &dates, "Hunk"),
                    case["overlay"]
                );
            }
        }
    }

    #[test]
    fn date_format_matches_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-date-format-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 18);
            for case in cases {
                assert_eq!(
                    format_release_date(case["input"].as_str().unwrap()),
                    case["expected"],
                    "{}",
                    case["input"]
                );
            }
        }
    }

    #[test]
    fn all_pinned_release_bodies_match_rendered_page_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-release-body-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for (index, result) in results.iter().enumerate() {
            let baseline = result["baseline"].as_str().unwrap();
            assert_eq!(
                baseline,
                [
                    "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                    "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
                ][index]
            );
            let output = std::process::Command::new("git")
                .args(["show", &format!("{baseline}:CHANGELOG.md")])
                .output()
                .unwrap();
            assert!(output.status.success());
            let releases = parse_changelog(std::str::from_utf8(&output.stdout).unwrap());
            assert_eq!(releases.len(), [48, 47][index]);
            let dates = serde_json::from_value(result["dates"].clone()).unwrap();
            let actual = group_into_series(releases).iter().map(|series| serde_json::json!({
                "minor": series.minor, "markdown": render_release_body(series, &dates, "https://github.com/modem-dev/hunk")
            })).collect::<Vec<_>>();
            assert_eq!(serde_json::to_value(actual).unwrap(), result["expected"]);
        }
    }

    #[test]
    fn publication_matches_explicit_pin_semantics() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-publication-oracle.json"
        ))
        .unwrap();
        let releases = parse_changelog(fixture["input"].as_str().unwrap());
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for (index, result) in results.iter().enumerate() {
            assert_eq!(result["includesPrereleases"], index == 0);
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 4);
            for case in cases {
                let dates = serde_json::from_value(case["dates"].clone()).unwrap();
                let state = publication_state(&releases, &dates);
                if index == 0 {
                    assert_eq!(state["published"], case["published"]);
                    assert_eq!(state["stable"], case["stable"]);
                } else {
                    assert_eq!(state["stable"], case["published"]);
                    assert!(case.get("stable").is_none());
                }
                assert_eq!(
                    state["latestStable"],
                    state["stable"]
                        .as_array()
                        .unwrap()
                        .first()
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
                );
            }
        }
    }

    #[test]
    fn summary_helpers_match_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-summary-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 8);
            for case in cases {
                let input = case["input"].as_str().unwrap();
                assert_eq!(
                    serde_json::to_value(split_highlights(input)).unwrap(),
                    case["split"]
                );
                assert_eq!(to_plain_text(input), case["plain"]);
                let release = |version: &str, text: &str| ReleaseEntry {
                    version: version.into(),
                    prerelease: false,
                    heading_date: None,
                    highlights: Some(text.into()),
                    sections: vec![],
                };
                let series = ReleaseSeries {
                    minor: "1.2".into(),
                    releases: vec![release("1.2.1", input), release("1.2.0", "Older **lead**")],
                };
                let actual = [None, Some(""), Some("Editorial **unchanged**")]
                    .map(|overlay| series_summary(&series, overlay));
                assert_eq!(serde_json::to_value(actual).unwrap(), case["summaries"]);
            }
        }
    }

    #[test]
    fn body_whitespace_matches_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-body-whitespace-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 24);
            for case in cases {
                assert_eq!(
                    serde_json::to_value(parse_changelog(case["input"].as_str().unwrap())).unwrap(),
                    case["expected"],
                    "{}",
                    case["input"]
                );
            }
        }
    }

    #[test]
    fn heading_whitespace_matches_both_pinned_oracles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-whitespace-oracle.json"
        ))
        .unwrap();
        let results = fixture["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        for result in results {
            let cases = result["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 2);
            for case in cases {
                assert_eq!(
                    serde_json::to_value(parse_changelog(case["input"].as_str().unwrap())).unwrap(),
                    case["expected"]
                );
            }
        }
    }

    #[test]
    fn date_maps_and_lookup_calls_match_frozen_pins() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/website-changelog-dates-oracle.json"
        ))
        .unwrap();
        let releases = parse_changelog(fixture["input"].as_str().unwrap());
        let recorded = serde_json::from_value(fixture["recorded"].clone()).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 2);
        for case in cases {
            let mut calls = Vec::new();
            let dates = resolve_dates(&releases, &recorded, |version| {
                calls.push(version.to_owned());
                fixture["lookup"][version].as_str().map(str::to_owned)
            });
            assert_eq!(
                serde_json::to_value(dates.keys().collect::<Vec<_>>()).unwrap(),
                case["keys"]
            );
            assert_eq!(serde_json::to_value(dates).unwrap(), case["dates"]);
            assert_eq!(serde_json::to_value(calls).unwrap(), case["lookupCalls"]);
            for tag in case["tagDates"].as_array().unwrap() {
                assert_eq!(
                    serde_json::to_value(resolve_tag_date(
                        tag["tagger"].as_str().unwrap(),
                        tag["commit"].as_str().unwrap()
                    ))
                    .unwrap(),
                    tag["expected"]
                );
            }
        }
        assert_eq!(cases[0]["tagHelperExported"], true);
        assert_eq!(cases[1]["tagHelperExported"], false);
    }

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
