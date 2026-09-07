//! Native Ratatui port of Hunk's constrained JSX file-view gallery.

use regex::Regex;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::sync::OnceLock;
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionDiffFile, ExtensionDiffHunk, ExtensionFileChangeKind, ExtensionFileChangeRange,
    ExtensionFileSide, ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewRowComponent, ExtensionFileViewSourceRange, ExtensionFileViewSpan,
    ExtensionFileViewTone, ExtensionHostAction, ExtensionNotifyType, FileViewLayoutRequest,
    FileViewMatchRequest, HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    Registration, ViewNode, ViewStyle,
};

pub const CHANGE_ATLAS_VIEW_ID: &str = "change-atlas";
pub const PALETTE_DELTA_VIEW_ID: &str = "palette-delta";
pub const DEPENDENCY_DELTA_VIEW_ID: &str = "dependency-delta";
pub const COMMAND_ID: &str = "toggle-file-view-gallery";
const SOURCE_LIMIT: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependencyGroup {
    Dependencies,
    DevDependencies,
}

impl DependencyGroup {
    const fn label(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::DevDependencies => "devDependencies",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CssToken {
    name: String,
    value: String,
    line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DependencyToken {
    group: DependencyGroup,
    name: String,
    value: String,
    line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct HighlightedVersion {
    pub before: String,
    pub changed: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct VersionChangeHighlights {
    pub old: HighlightedVersion,
    pub new: HighlightedVersion,
}

fn text(value: impl Into<String>, foreground: Option<&str>) -> ViewNode {
    ViewNode::Text {
        text: value.into(),
        style: ViewStyle {
            foreground: foreground.map(Into::into),
            ..ViewStyle::default()
        },
    }
}

fn colored_text(value: impl Into<String>, foreground: &str, background: &str) -> ViewNode {
    ViewNode::Text {
        text: value.into(),
        style: ViewStyle {
            foreground: Some(foreground.into()),
            background: Some(background.into()),
            ..ViewStyle::default()
        },
    }
}

fn column(children: Vec<ViewNode>) -> ViewNode {
    ViewNode::Column { children, gap: 0 }
}

fn row(children: Vec<ViewNode>) -> ViewNode {
    ViewNode::Row { children, gap: 0 }
}

fn component(
    height: usize,
    content: ViewNode,
    selected_content: ViewNode,
) -> ExtensionFileViewRowComponent {
    ExtensionFileViewRowComponent {
        height,
        content,
        selected_content: Some(selected_content),
        expanded_content: None,
        selected_expanded_content: None,
        toggle_expanded_on_left_mouse_up: false,
        selection_prefix: None,
    }
}

fn change_size(change: &ExtensionFileChangeRange) -> usize {
    change.range[1].saturating_sub(change.range[0]) + 1
}

fn hunk_source_ranges(hunk: &ExtensionDiffHunk) -> Vec<ExtensionFileViewSourceRange> {
    [
        (ExtensionFileSide::Old, hunk.old_range),
        (ExtensionFileSide::New, hunk.new_range),
    ]
    .into_iter()
    .filter_map(|(side, range)| {
        let range = range?;
        (range[0] >= 1).then_some(ExtensionFileViewSourceRange {
            side,
            range: [range[0] as usize, range[1] as usize],
        })
    })
    .collect()
}

fn clip_label(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let code_units = value.encode_utf16().collect::<Vec<_>>();
    if code_units.len() <= width {
        return value.into();
    }
    if width == 1 {
        return String::from_utf16_lossy(&code_units[..1]);
    }
    format!("{}…", String::from_utf16_lossy(&code_units[..width - 1]))
}

fn javascript_string_length(value: &str) -> usize {
    value.encode_utf16().count()
}

/// Reproduce Hunk's bounded proportional impact meter.
#[must_use]
pub fn impact_meter(value: usize, total: usize, width: usize) -> String {
    let filled = if value == 0 || total == 0 {
        0
    } else {
        (((value as f64 / total as f64) * width as f64).round() as usize).max(1)
    }
    .min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn impact_content(
    selected: bool,
    position: usize,
    header: &str,
    added: usize,
    removed: usize,
    width: usize,
) -> ViewNode {
    let meter_width = ((width as isize - 20) / 2).clamp(4, 16) as usize;
    let total = (added + removed).max(1);
    column(vec![
        row(vec![
            text(
                format!(
                    "{} CHANGE {:02}",
                    if selected { '▶' } else { '◇' },
                    position + 1
                ),
                Some(if selected { "accent" } else { "accent-muted" }),
            ),
            text(format!("   +{added} / -{removed}"), Some("text")),
        ]),
        row(vec![
            text(
                format!("  + {}", impact_meter(added, total, meter_width)),
                Some("file-new"),
            ),
            text(
                format!("   - {}", impact_meter(removed, total, meter_width)),
                Some("file-deleted"),
            ),
        ]),
        text(
            format!("  {}", clip_label(header, width.saturating_sub(2).max(1))),
            Some("muted"),
        ),
    ])
}

/// Build one fixed three-row impact card for every real diff hunk.
#[must_use]
pub fn create_change_atlas_layout(
    request: &FileViewLayoutRequest,
) -> Option<ExtensionFileViewLayout> {
    if request.file.hunks.is_empty() {
        return None;
    }
    let rows = request
        .file
        .hunks
        .iter()
        .enumerate()
        .map(|(position, hunk)| {
            let changes = request
                .changes
                .iter()
                .filter(|change| change.hunk_index == position)
                .collect::<Vec<_>>();
            let added = changes
                .iter()
                .filter(|change| change.kind == ExtensionFileChangeKind::Added)
                .map(|change| change_size(change))
                .sum();
            let removed = changes
                .iter()
                .filter(|change| change.kind == ExtensionFileChangeKind::Removed)
                .map(|change| change_size(change))
                .sum();
            ExtensionFileViewRow {
                id: format!("impact:{position}"),
                spans: vec![ExtensionFileViewSpan {
                    text: format!(
                        "Change {}: +{added} -{removed} · {}",
                        position + 1,
                        hunk.header
                    ),
                    tone: Some(ExtensionFileViewTone::Accent),
                    attributes: Vec::new(),
                }],
                source_ranges: hunk_source_ranges(hunk),
                component: Some(component(
                    3,
                    impact_content(false, position, &hunk.header, added, removed, request.width),
                    impact_content(true, position, &hunk.header, added, removed, request.width),
                )),
            }
        })
        .collect::<Vec<_>>();
    Some(ExtensionFileViewLayout {
        hunk_rows: request
            .file
            .hunks
            .iter()
            .enumerate()
            .map(|(position, _)| ExtensionFileViewHunkRows {
                start_row: position,
                end_row: position,
            })
            .collect(),
        rows,
    })
}

fn summary_content(selected: bool, title: &str, detail: &str, width: usize) -> ViewNode {
    column(vec![
        text(
            format!(
                "{} {}",
                if selected { '▶' } else { '◇' },
                clip_label(title, width.saturating_sub(2).max(1))
            ),
            Some(if selected { "accent" } else { "accent-muted" }),
        ),
        text(
            format!("  {}", clip_label(detail, width.saturating_sub(2).max(1))),
            Some("muted"),
        ),
    ])
}

fn summary_component(title: &str, detail: &str, width: usize) -> ExtensionFileViewRowComponent {
    component(
        2,
        summary_content(false, title, detail, width),
        summary_content(true, title, detail, width),
    )
}

fn css_token_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^\s*(--[A-Za-z0-9_-]+)\s*:\s*(#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{3}))\s*;")
            .expect("CSS token pattern is valid")
    })
}

fn parse_css_tokens(source: &str) -> Vec<CssToken> {
    source
        .split('\n')
        .enumerate()
        .filter_map(|(index, line)| {
            let captures = css_token_pattern().captures(line)?;
            Some(CssToken {
                name: captures[1].into(),
                value: captures[2].into(),
                line: index + 1,
            })
        })
        .collect()
}

fn token_is_changed(
    line: usize,
    kind: ExtensionFileChangeKind,
    position: usize,
    changes: &[ExtensionFileChangeRange],
) -> bool {
    changes.iter().any(|change| {
        change.hunk_index == position
            && change.kind == kind
            && change.range[0] <= line
            && line <= change.range[1]
    })
}

fn swatch_foreground(color: &str) -> &'static str {
    let raw = color.strip_prefix('#').unwrap_or(color);
    let normalized = if raw.len() == 3 {
        raw.chars()
            .flat_map(|character| [character, character])
            .collect::<String>()
    } else {
        raw.chars().take(6).collect()
    };
    let Ok(value) = u32::from_str_radix(&normalized, 16) else {
        return "white";
    };
    let red = (value >> 16) & 255;
    let green = (value >> 8) & 255;
    let blue = value & 255;
    if red as f64 * 0.299 + green as f64 * 0.587 + blue as f64 * 0.114 > 150.0 {
        "black"
    } else {
        "white"
    }
}

fn padded_swatch(label: &str) -> String {
    let mut value = label.to_owned();
    value.extend(std::iter::repeat_n(
        ' ',
        18_usize.saturating_sub(value.chars().count()),
    ));
    value
}

fn palette_content(
    selected: bool,
    name: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
    width: usize,
) -> ViewNode {
    let old_color = old_value.unwrap_or("#303030");
    let new_color = new_value.unwrap_or("#303030");
    column(vec![
        text(
            format!(
                "{} {}",
                if selected { '▶' } else { ' ' },
                clip_label(name, width.saturating_sub(2).max(1))
            ),
            Some(if selected { "accent" } else { "accent-muted" }),
        ),
        row(vec![
            colored_text(
                padded_swatch(&format!(" OLD {}", old_value.unwrap_or("missing"))),
                swatch_foreground(old_color),
                old_color,
            ),
            text("  →  ", None),
            colored_text(
                padded_swatch(&format!(" NEW {}", new_value.unwrap_or("missing"))),
                swatch_foreground(new_color),
                new_color,
            ),
        ]),
    ])
}

fn document(request: &FileViewLayoutRequest, side: ExtensionFileSide) -> Option<&str> {
    request.documents.get(&side)?.as_deref()
}

/// Build semantic old/new swatches for changed three- or six-digit CSS variables.
#[must_use]
pub fn create_css_palette_layout(
    request: &FileViewLayoutRequest,
) -> Option<ExtensionFileViewLayout> {
    let old_source = document(request, ExtensionFileSide::Old)?;
    let new_source = document(request, ExtensionFileSide::New)?;
    if request.file.hunks.is_empty()
        || javascript_string_length(old_source) > SOURCE_LIMIT
        || javascript_string_length(new_source) > SOURCE_LIMIT
        || request.aborted
    {
        return None;
    }
    let old_tokens = parse_css_tokens(old_source);
    let new_tokens = parse_css_tokens(new_source);
    let mut rows = Vec::new();
    let mut hunk_rows = Vec::new();
    let mut semantic_rows = 0;
    for (position, hunk) in request.file.hunks.iter().enumerate() {
        let start_row = rows.len();
        let old_changed = old_tokens
            .iter()
            .filter(|token| {
                token_is_changed(
                    token.line,
                    ExtensionFileChangeKind::Removed,
                    position,
                    &request.changes,
                )
            })
            .collect::<Vec<_>>();
        let new_changed = new_tokens
            .iter()
            .filter(|token| {
                token_is_changed(
                    token.line,
                    ExtensionFileChangeKind::Added,
                    position,
                    &request.changes,
                )
            })
            .collect::<Vec<_>>();
        let mut names = Vec::<String>::new();
        for token in old_changed.iter().chain(&new_changed) {
            if !names.contains(&token.name) {
                names.push(token.name.clone());
            }
        }
        if names.iter().any(|name| {
            old_changed
                .iter()
                .filter(|token| token.name == *name)
                .count()
                > 1
                || new_changed
                    .iter()
                    .filter(|token| token.name == *name)
                    .count()
                    > 1
        }) {
            return None;
        }
        for (token_position, name) in names.into_iter().enumerate() {
            let old_token = old_changed.iter().find(|token| token.name == name).copied();
            let new_token = new_changed.iter().find(|token| token.name == name).copied();
            let old_value = old_token.map(|token| token.value.as_str());
            let new_value = new_token.map(|token| token.value.as_str());
            semantic_rows += 1;
            rows.push(ExtensionFileViewRow {
                id: format!("palette:{position}:{token_position}:{name}"),
                spans: vec![ExtensionFileViewSpan {
                    text: format!(
                        "{name}: {} → {}",
                        old_value.unwrap_or("missing"),
                        new_value.unwrap_or("missing")
                    ),
                    tone: Some(ExtensionFileViewTone::Syntax),
                    attributes: Vec::new(),
                }],
                source_ranges: old_token
                    .map(|token| ExtensionFileViewSourceRange {
                        side: ExtensionFileSide::Old,
                        range: [token.line, token.line],
                    })
                    .into_iter()
                    .chain(new_token.map(|token| ExtensionFileViewSourceRange {
                        side: ExtensionFileSide::New,
                        range: [token.line, token.line],
                    }))
                    .collect(),
                component: Some(component(
                    2,
                    palette_content(false, &name, old_value, new_value, request.width),
                    palette_content(true, &name, old_value, new_value, request.width),
                )),
            });
        }
        if rows.len() == start_row {
            let title = format!("PALETTE HUNK {}", position + 1);
            rows.push(ExtensionFileViewRow {
                id: format!("palette:{position}:summary"),
                spans: vec![ExtensionFileViewSpan {
                    text: format!("Hunk {}: no changed hexadecimal variables", position + 1),
                    tone: Some(ExtensionFileViewTone::Muted),
                    attributes: Vec::new(),
                }],
                source_ranges: hunk_source_ranges(hunk),
                component: Some(summary_component(&title, &hunk.header, request.width)),
            });
        }
        hunk_rows.push(ExtensionFileViewHunkRows {
            start_row,
            end_row: rows.len() - 1,
        });
    }
    (semantic_rows > 0).then_some(ExtensionFileViewLayout { rows, hunk_rows })
}

fn json_dependency_header() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"^(\s*)"(dependencies|devDependencies)"\s*:\s*\{\s*$"#)
            .expect("dependency section pattern is valid")
    })
}

fn json_dependency_entry() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"^\s*"([^"]+)"\s*:\s*"([^"]+)"\s*,?\s*$"#)
            .expect("dependency entry pattern is valid")
    })
}

fn parse_json_dependency_tokens(source: &str) -> Option<Vec<DependencyToken>> {
    serde_json::from_str::<Value>(source).ok()?;
    let mut tokens = Vec::new();
    let mut group = None;
    let mut section_indent = 0;
    for (index, line) in source.split('\n').enumerate() {
        if let Some(captures) = json_dependency_header().captures(line) {
            group = Some(if &captures[2] == "dependencies" {
                DependencyGroup::Dependencies
            } else {
                DependencyGroup::DevDependencies
            });
            section_indent = captures[1].len();
            continue;
        }
        if group.is_some()
            && line.trim_start().starts_with('}')
            && line.len().saturating_sub(line.trim_start().len()) <= section_indent
        {
            group = None;
            continue;
        }
        let Some(group) = group else {
            continue;
        };
        if let Some(captures) = json_dependency_entry().captures(line) {
            tokens.push(DependencyToken {
                group,
                name: captures[1].into(),
                value: captures[2].into(),
                line: index + 1,
            });
        }
    }
    Some(tokens)
}

fn cargo_dependency_header() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^\s*\[(dependencies|dev-dependencies)\]\s*$")
            .expect("Cargo dependency section pattern is valid")
    })
}

fn cargo_dependency_entry() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r#"^\s*([A-Za-z0-9_-]+)\s*=\s*"([^"]+)"\s*$"#)
            .expect("Cargo dependency entry pattern is valid")
    })
}

fn parse_cargo_dependency_tokens(source: &str) -> Option<Vec<DependencyToken>> {
    source.parse::<toml::Value>().ok()?;
    let mut tokens = Vec::new();
    let mut group = None;
    for (index, line) in source.split('\n').enumerate() {
        if let Some(captures) = cargo_dependency_header().captures(line) {
            group = Some(if &captures[1] == "dependencies" {
                DependencyGroup::Dependencies
            } else {
                DependencyGroup::DevDependencies
            });
            continue;
        }
        if line.trim_start().starts_with('[') {
            group = None;
            continue;
        }
        let Some(group) = group else {
            continue;
        };
        if let Some(captures) = cargo_dependency_entry().captures(line) {
            tokens.push(DependencyToken {
                group,
                name: captures[1].into(),
                value: captures[2].into(),
                line: index + 1,
            });
        }
    }
    Some(tokens)
}

fn parse_dependency_tokens(source: &str) -> Option<Vec<DependencyToken>> {
    parse_json_dependency_tokens(source).or_else(|| parse_cargo_dependency_tokens(source))
}

fn whole(value: Option<&str>) -> HighlightedVersion {
    HighlightedVersion {
        before: String::new(),
        changed: value.unwrap_or_default().into(),
        after: String::new(),
    }
}

struct ParsedVersion<'a> {
    major: &'a str,
    minor: &'a str,
    patch: &'a str,
    prefix: &'a str,
    suffix: &'a str,
    minor_start: usize,
    patch_start: usize,
}

fn version_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^([^0-9]*)([0-9]+)\.([0-9]+)\.([0-9]+)(.*)$")
            .expect("version pattern is valid")
    })
}

fn parse_version(value: &str) -> Option<ParsedVersion<'_>> {
    let captures = version_pattern().captures(value)?;
    let prefix = captures.get(1)?;
    let major = captures.get(2)?;
    let minor = captures.get(3)?;
    let patch = captures.get(4)?;
    let suffix = captures.get(5)?;
    Some(ParsedVersion {
        major: major.as_str(),
        minor: minor.as_str(),
        patch: patch.as_str(),
        prefix: prefix.as_str(),
        suffix: suffix.as_str(),
        minor_start: minor.start(),
        patch_start: patch.start(),
    })
}

fn split_highlight(value: &str, start: usize, length: usize) -> HighlightedVersion {
    HighlightedVersion {
        before: value[..start].into(),
        changed: value[start..start + length].into(),
        after: value[start + length..].into(),
    }
}

/// Highlight the same meaningful semantic-version segment as the Hunk gallery.
#[must_use]
pub fn version_change_highlights(
    old_value: Option<&str>,
    new_value: Option<&str>,
) -> VersionChangeHighlights {
    let Some(old_value) = old_value else {
        return VersionChangeHighlights {
            old: HighlightedVersion {
                before: "∅".into(),
                changed: String::new(),
                after: String::new(),
            },
            new: whole(new_value),
        };
    };
    let Some(new_value) = new_value else {
        return VersionChangeHighlights {
            old: whole(Some(old_value)),
            new: HighlightedVersion {
                before: "∅".into(),
                changed: String::new(),
                after: String::new(),
            },
        };
    };
    let (Some(old), Some(new)) = (parse_version(old_value), parse_version(new_value)) else {
        return VersionChangeHighlights {
            old: whole(Some(old_value)),
            new: whole(Some(new_value)),
        };
    };
    if old.major != new.major || old.prefix != new.prefix || old.suffix != new.suffix {
        return VersionChangeHighlights {
            old: whole(Some(old_value)),
            new: whole(Some(new_value)),
        };
    }
    if old.minor != new.minor {
        return VersionChangeHighlights {
            old: split_highlight(old_value, old.minor_start, old.minor.len()),
            new: split_highlight(new_value, new.minor_start, new.minor.len()),
        };
    }
    if old.patch != new.patch {
        return VersionChangeHighlights {
            old: split_highlight(old_value, old.patch_start, old.patch.len()),
            new: split_highlight(new_value, new.patch_start, new.patch.len()),
        };
    }
    VersionChangeHighlights {
        old: HighlightedVersion {
            before: old_value.into(),
            changed: String::new(),
            after: String::new(),
        },
        new: HighlightedVersion {
            before: new_value.into(),
            changed: String::new(),
            after: String::new(),
        },
    }
}

fn highlighted_version(
    value: &HighlightedVersion,
    changed_color: &str,
    muted_color: &str,
) -> Vec<ViewNode> {
    let mut nodes = vec![text(value.before.clone(), Some(muted_color))];
    if !value.changed.is_empty() {
        nodes.push(colored_text(
            value.changed.clone(),
            changed_color,
            "panel-alt",
        ));
    }
    nodes.push(text(value.after.clone(), Some(muted_color)));
    nodes
}

fn dependency_content(
    selected: bool,
    name: &str,
    group: DependencyGroup,
    old_value: Option<&str>,
    new_value: Option<&str>,
    width: usize,
) -> ViewNode {
    let highlights = version_change_highlights(old_value, new_value);
    let mut version_row = vec![text(" ", Some("muted"))];
    version_row.extend(highlighted_version(
        &highlights.old,
        "file-deleted",
        "muted",
    ));
    version_row.push(text(" → ", Some("muted")));
    version_row.extend(highlighted_version(&highlights.new, "file-new", "muted"));
    column(vec![
        text(
            format!(
                "{} {}  {}",
                if selected { '▶' } else { ' ' },
                clip_label(name, width.saturating_sub(group.label().len() + 5).max(1)),
                group.label()
            ),
            Some(if selected { "accent" } else { "accent-muted" }),
        ),
        row(version_row),
    ])
}

fn duplicate_dependency_identity(tokens: &[DependencyToken]) -> bool {
    let mut identities = Vec::new();
    for token in tokens {
        let identity = (token.group, token.name.as_str());
        if identities.contains(&identity) {
            return true;
        }
        identities.push(identity);
    }
    false
}

/// Build dependency cards while preserving a positional row range for every parsed hunk.
#[must_use]
pub fn create_dependency_layout(
    request: &FileViewLayoutRequest,
) -> Option<ExtensionFileViewLayout> {
    let old_source = document(request, ExtensionFileSide::Old)?;
    let new_source = document(request, ExtensionFileSide::New)?;
    if request.file.hunks.is_empty()
        || old_source.is_empty()
        || new_source.is_empty()
        || javascript_string_length(old_source) > SOURCE_LIMIT
        || javascript_string_length(new_source) > SOURCE_LIMIT
        || request.aborted
    {
        return None;
    }
    let old_tokens = parse_dependency_tokens(old_source)?;
    let new_tokens = parse_dependency_tokens(new_source)?;
    if duplicate_dependency_identity(&old_tokens) || duplicate_dependency_identity(&new_tokens) {
        return None;
    }
    let mut rows = Vec::new();
    let mut hunk_rows = Vec::new();
    let mut semantic_rows = 0;
    for (position, hunk) in request.file.hunks.iter().enumerate() {
        let start_row = rows.len();
        let old_changed = old_tokens
            .iter()
            .filter(|token| {
                token_is_changed(
                    token.line,
                    ExtensionFileChangeKind::Removed,
                    position,
                    &request.changes,
                )
            })
            .collect::<Vec<_>>();
        let new_changed = new_tokens
            .iter()
            .filter(|token| {
                token_is_changed(
                    token.line,
                    ExtensionFileChangeKind::Added,
                    position,
                    &request.changes,
                )
            })
            .collect::<Vec<_>>();
        let mut identities = Vec::<(DependencyGroup, String)>::new();
        for token in old_changed.iter().chain(&new_changed) {
            let identity = (token.group, token.name.clone());
            if !identities.contains(&identity) {
                identities.push(identity);
            }
        }
        for (token_position, (group, name)) in identities.into_iter().enumerate() {
            let old_token = old_changed
                .iter()
                .find(|token| token.group == group && token.name == name)
                .copied();
            let new_token = new_changed
                .iter()
                .find(|token| token.group == group && token.name == name)
                .copied();
            let old_value = old_token.map(|token| token.value.as_str());
            let new_value = new_token.map(|token| token.value.as_str());
            semantic_rows += 1;
            rows.push(ExtensionFileViewRow {
                id: format!(
                    "dependency:{position}:{token_position}:{}:{name}",
                    group.label()
                ),
                spans: vec![ExtensionFileViewSpan {
                    text: format!(
                        "{name}: {} → {} ({})",
                        old_value.unwrap_or("missing"),
                        new_value.unwrap_or("missing"),
                        group.label()
                    ),
                    tone: Some(if new_value.is_none() {
                        ExtensionFileViewTone::Removed
                    } else if old_value.is_none() {
                        ExtensionFileViewTone::Added
                    } else {
                        ExtensionFileViewTone::Accent
                    }),
                    attributes: Vec::new(),
                }],
                source_ranges: old_token
                    .map(|token| ExtensionFileViewSourceRange {
                        side: ExtensionFileSide::Old,
                        range: [token.line, token.line],
                    })
                    .into_iter()
                    .chain(new_token.map(|token| ExtensionFileViewSourceRange {
                        side: ExtensionFileSide::New,
                        range: [token.line, token.line],
                    }))
                    .collect(),
                component: Some(component(
                    2,
                    dependency_content(false, &name, group, old_value, new_value, request.width),
                    dependency_content(true, &name, group, old_value, new_value, request.width),
                )),
            });
        }
        if rows.len() == start_row {
            let title = format!("Package metadata hunk {}", position + 1);
            rows.push(ExtensionFileViewRow {
                id: format!("dependency:{position}:summary"),
                spans: vec![ExtensionFileViewSpan {
                    text: format!("{title}: {}", hunk.header),
                    tone: Some(ExtensionFileViewTone::Muted),
                    attributes: Vec::new(),
                }],
                source_ranges: hunk_source_ranges(hunk),
                component: Some(summary_component(&title, &hunk.header, request.width)),
            });
        }
        hunk_rows.push(ExtensionFileViewHunkRows {
            start_row,
            end_row: rows.len() - 1,
        });
    }
    (semantic_rows > 0).then_some(ExtensionFileViewLayout { rows, hunk_rows })
}

fn path_basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn path_extension(path: &str) -> Option<&str> {
    path_basename(path)
        .rsplit_once('.')
        .map(|(_, extension)| extension)
}

fn gallery_view_for_parts<'a>(
    path: &'a str,
    previous_path: Option<&'a str>,
    language: Option<&str>,
) -> Option<&'static str> {
    let mut paths = std::iter::once(path).chain(previous_path);
    for candidate in paths.clone() {
        let basename = path_basename(candidate);
        if basename.eq_ignore_ascii_case("package.json")
            || basename.eq_ignore_ascii_case("cargo.toml")
        {
            return Some(DEPENDENCY_DELTA_VIEW_ID);
        }
    }
    if language.is_some_and(|language| language.eq_ignore_ascii_case("css"))
        || paths
            .clone()
            .any(|path| path_extension(path).is_some_and(|value| value.eq_ignore_ascii_case("css")))
    {
        return Some(PALETTE_DELTA_VIEW_ID);
    }
    if language.is_some_and(|language| {
        matches!(
            language.to_ascii_lowercase().as_str(),
            "typescript" | "javascript" | "rust"
        )
    }) || paths.any(|path| {
        path_extension(path).is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "js" | "jsx"
                    | "mjs"
                    | "mjsx"
                    | "cjs"
                    | "cjsx"
                    | "ts"
                    | "tsx"
                    | "mts"
                    | "mtsx"
                    | "cts"
                    | "ctsx"
                    | "rs"
            )
        })
    }) {
        return Some(CHANGE_ATLAS_VIEW_ID);
    }
    None
}

#[must_use]
pub fn gallery_view_for_file(file: &ExtensionDiffFile) -> Option<&'static str> {
    gallery_view_for_parts(
        &file.path,
        file.previous_path.as_deref(),
        file.language.as_deref(),
    )
}

#[must_use]
pub fn file_view_matches(view_id: &str, file: &ExtensionDiffFile) -> bool {
    gallery_view_for_file(file) == Some(view_id)
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::FileView {
            id: CHANGE_ATLAS_VIEW_ID.into(),
            title: "JSX demo: Change atlas".into(),
            priority: 0,
            interactive_mode: false,
        },
        Registration::FileView {
            id: PALETTE_DELTA_VIEW_ID.into(),
            title: "JSX demo: CSS palette delta".into(),
            priority: 0,
            interactive_mode: false,
        },
        Registration::FileView {
            id: DEPENDENCY_DELTA_VIEW_ID.into(),
            title: "JSX demo: Dependency delta".into(),
            priority: 0,
            interactive_mode: false,
        },
        Registration::Command(CommandRegistration {
            id: COMMAND_ID.into(),
            title: "Toggle native demo for current file".into(),
            description: None,
            default_keys: vec!["f8".into()],
        }),
    ]
}

#[must_use]
pub fn required_capabilities() -> Vec<Capability> {
    vec![
        Capability::Commands,
        Capability::FileViews,
        Capability::Notifications,
    ]
}

pub fn invoke_command(invocation: &CommandInvocation) -> Result<CommandExecution, String> {
    if invocation.command_id != COMMAND_ID {
        return Err(format!("Unknown command: {}", invocation.command_id));
    }
    let view_id = invocation
        .snapshot
        .changeset
        .files
        .get(invocation.snapshot.selection.file_index)
        .and_then(|file| {
            gallery_view_for_parts(
                &file.path,
                file.previous_path.as_deref(),
                file.language.as_deref(),
            )
        });
    Ok(CommandExecution {
        actions: if let Some(view_id) = view_id {
            vec![ExtensionHostAction::ToggleFileView { id: view_id.into() }]
        } else {
            vec![ExtensionHostAction::Notify {
                message: "The native gallery has no demo for this file type.".into(),
                notification_type: ExtensionNotifyType::Info,
            }]
        },
    })
}

fn layout_for_request(
    request: &FileViewLayoutRequest,
) -> Result<Option<ExtensionFileViewLayout>, String> {
    if !file_view_matches(&request.view_id, &request.file) {
        return Ok(None);
    }
    match request.view_id.as_str() {
        CHANGE_ATLAS_VIEW_ID => Ok(create_change_atlas_layout(request)),
        PALETTE_DELTA_VIEW_ID => Ok(create_css_palette_layout(request)),
        DEPENDENCY_DELTA_VIEW_ID => Ok(create_dependency_layout(request)),
        _ => Err(format!("Unknown file view: {}", request.view_id)),
    }
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => serde_json::to_value(HandshakeResponse {
                extension_api_version: API_VERSION,
                extension_version: env!("CARGO_PKG_VERSION").into(),
                registrations: registrations(),
            })
            .map_err(io::Error::other),
            "workdeck/command/invoke" => {
                let invocation: CommandInvocation =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                invoke_command(&invocation)
                    .and_then(|execution| {
                        serde_json::to_value(execution).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/file-view/matches" => {
                let request: FileViewMatchRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                serde_json::to_value(file_view_matches(&request.view_id, &request.file))
                    .map_err(io::Error::other)
            }
            "workdeck/file-view/layout" => {
                let request: FileViewLayoutRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                layout_for_request(&request)
                    .and_then(|layout| {
                        serde_json::to_value(layout).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        let response = match result {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: error.to_string(),
                    data: None,
                }),
            },
        };
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}
