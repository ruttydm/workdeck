//! Terminal-safe STML parser, deterministic layout engine, and renderer.

use serde::{Deserialize, Serialize};

mod layout;
mod parse;
mod render;

pub use layout::*;
pub use parse::*;
pub use render::*;

pub const DEFAULT_WIDTH: usize = STML_REFERENCE_WIDTH;
pub const MAX_MARKUP_BYTES: usize = DEFAULT_STML_PARSE_LIMITS.max_input_bytes;

/// Render-time STML projection of the active Workdeck application theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StmlThemeColors {
    pub accent: String,
    pub accent_muted: String,
    pub added_sign_color: String,
    pub removed_sign_color: String,
    pub file_modified: String,
    pub muted: String,
    pub panel_alt: String,
    pub text: String,
    pub panel: String,
    pub note_border: String,
    pub background: String,
}

/// Resolve one symbolic, named, or explicit STML color at paint time.
///
/// Markup layout retains color tokens as strings so measurement is theme-free.
/// Unknown values return `None`, allowing callers to keep their default text color.
#[must_use]
pub fn resolve_stml_color(token: Option<&str>, theme: &StmlThemeColors) -> Option<String> {
    let value = token?.trim().to_ascii_lowercase();
    let semantic = match value.as_str() {
        "accent" => Some(&theme.accent),
        "info" => Some(&theme.accent_muted),
        "success" => Some(&theme.added_sign_color),
        "danger" | "error" => Some(&theme.removed_sign_color),
        "warning" => Some(&theme.file_modified),
        "muted" => Some(&theme.muted),
        "subtle" => Some(&theme.panel_alt),
        "heading" | "text" => Some(&theme.text),
        "panel" | "bg" => Some(&theme.panel),
        "note-border" => Some(&theme.note_border),
        "badge-text" => Some(&theme.background),
        _ => None,
    };
    if let Some(color) = semantic {
        return Some(color.clone());
    }
    if is_stml_hex_color(&value) {
        return Some(value);
    }
    Some(
        match value.as_str() {
            "black" => "#1c1c1c",
            "red" => "#e05252",
            "green" => "#4fb469",
            "yellow" => "#d9a331",
            "blue" => "#4f8fd9",
            "magenta" => "#b969d9",
            "cyan" => "#3fb5b5",
            "white" => "#e8e8e8",
            "gray" | "grey" => "#8a8a8a",
            "orange" => "#e0873d",
            "purple" => "#9a6fd0",
            "pink" => "#d9699a",
            _ => return None,
        }
        .into(),
    )
}

fn is_stml_hex_color(value: &str) -> bool {
    matches!(value.len(), 4 | 7)
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StmlTagRole {
    Strong,
    Emphasis,
    Underline,
    Strike,
    Muted,
    Key,
    Badge,
    Link,
    Styled,
    LineBreak,
    Container,
    Card,
    Row,
    Paragraph,
    Heading,
    Title,
    Divider,
    Spacer,
    List,
    OrderedList,
    ListItem,
    Code,
}

pub fn stml_tag_role(tag: &str) -> Option<StmlTagRole> {
    Some(match tag {
        "b" | "strong" => StmlTagRole::Strong,
        "i" | "em" => StmlTagRole::Emphasis,
        "u" => StmlTagRole::Underline,
        "s" | "strike" | "del" => StmlTagRole::Strike,
        "dim" | "muted" => StmlTagRole::Muted,
        "kbd" => StmlTagRole::Key,
        "badge" => StmlTagRole::Badge,
        "a" | "link" => StmlTagRole::Link,
        "c" | "color" | "span" => StmlTagRole::Styled,
        "br" => StmlTagRole::LineBreak,
        "box" | "col" | "column" | "stack" | "section" => StmlTagRole::Container,
        "card" => StmlTagRole::Card,
        "row" => StmlTagRole::Row,
        "text" | "p" => StmlTagRole::Paragraph,
        "h" | "h2" | "h3" | "heading" => StmlTagRole::Heading,
        "h1" | "title" => StmlTagRole::Title,
        "hr" | "rule" | "divider" => StmlTagRole::Divider,
        "spacer" | "space" => StmlTagRole::Spacer,
        "list" | "ul" => StmlTagRole::List,
        "ol" => StmlTagRole::OrderedList,
        "item" | "li" => StmlTagRole::ListItem,
        "code" | "pre" => StmlTagRole::Code,
        _ => return None,
    })
}

pub fn is_inline_stml_role(role: Option<StmlTagRole>) -> bool {
    matches!(
        role,
        Some(
            StmlTagRole::Strong
                | StmlTagRole::Emphasis
                | StmlTagRole::Underline
                | StmlTagRole::Strike
                | StmlTagRole::Muted
                | StmlTagRole::Key
                | StmlTagRole::Badge
                | StmlTagRole::Link
                | StmlTagRole::Styled
                | StmlTagRole::LineBreak
        )
    )
}

pub fn is_void_stml_tag(tag: &str) -> bool {
    matches!(
        stml_tag_role(tag),
        Some(StmlTagRole::LineBreak | StmlTagRole::Divider | StmlTagRole::Spacer)
    )
}

pub fn is_raw_text_stml_tag(tag: &str) -> bool {
    stml_tag_role(tag) == Some(StmlTagRole::Code)
}

pub const STML_REFERENCE_WIDTH: usize = 56;

/// Canonical, on-demand teaching artifact for agents authoring Workdeck notes.
pub const GUIDE: &str = r#"# STML — terminal markup for Workdeck agent notes

Experimental: the tag and color vocabulary may change between releases.
The review must be launched with `--experimental`; otherwise Workdeck uses the
required plain-text summary fallback and rejects live markup comments.

Small HTML-like markup rendered as real terminal UI inside agent notes:
boxes, rows, badges, gauges, lists, code blocks. Sources (--summary stays
as the plain-text fallback):

    workdeck session comment add ... --markup '<text>formatted note body</text>'
    comment apply items:    { "markup": "...", ... }
    agent-context sidecar:  annotations[].markup

Preview from a file or stdin:

    echo '<badge color="success">OK</badge> ready' | workdeck markup render -

## Ground rules

- Workdeck supplies the note's outer frame, author, and source location. STML is
  the note body; use borders for useful inner hierarchy rather than duplicating
  that frame around the whole body. Sibling and nested boxes are supported.
- Confirm `workdeck session context --json` lists `stml` in
  `experimentalFeatures` before authoring markup. Width follows the live
  session: stack ≈ full pane, split ≈ half. The context reports
  `noteMarkupWidth`; comment responses echo `markupWidth`. Preview with
  `workdeck markup render - --width <that>`. Unknown? Design for ~56 cols —
  it holds up wider, and users resize/switch layouts anytime.
- No chart tag: gauges are block chars (█ ░) in color spans (glyph-run example below).
- Bad markup degrades instead of crashing and produces render notes
  (in comment responses and on `markup render` stderr) — fix what they flag.
- Entities work: &rarr; → &check; ✓ &amp; &.

## Tags

Block: box card section col row · text p · h1 h2 h3 · list ul ol item ·
hr · spacer · code pre
Inline: b i u s dim · c/color · kbd · badge · a · br
box/card attrs: border, border-style (single|rounded|double|heavy),
border-color, title, title-color, bg, padding[-x|-y], width (cells or %).
row: gap. list: marker. spacer: size. code: title.
Colors: theme tokens accent success warning danger info muted subtle heading
(preferred — they follow the user's theme), ANSI names, or #hex.

## Syntax examples

These fragments demonstrate mechanics, not preferred layouts. STML is an
open composition grammar: combine, omit, repeat, and nest elements as the
explanation requires.

Inline styles:

```stml
<text><badge color="success">label</badge> <b>bold</b> <i>italic</i> <dim>dim</dim></text>
```

Bordered grouping:

```stml
<box border border-color="accent" padding-x="1" title="group">
  grouped detail
</box>
```

Responsive siblings:

```stml
<row gap="2">
  <box border title="left">first region</box>
  <box border title="right">second region</box>
</row>
```

Colored glyph runs:

```stml
<text><c fg="success">████████████</c><c fg="subtle">░░░░░░░░</c> 60%</text>
```

Rows with connectors (the <br/> vertically centers each arrow):

```stml
<row gap="1">
  <box border>first<br/><dim>detail</dim></box>
  <text width="3"><br/> &rarr;</text>
  <box border>second<br/><dim>detail</dim></box>
</row>
```

List structure:

```stml
<list>
  <item>first item</item>
  <item>second item</item>
</list>
```

Fixed-width columns:

```stml
<row gap="1">
  <box width="12"><dim>label</dim><br/><dim>status</dim></box>
  <box>value<br/>ready</box>
</row>
```

Verbatim block (clips, never wraps):

```stml
<code title="output">
const value = compute();
</code>
```

Keyboard token:

```stml
<text><kbd> key </kbd></text>
```

Use STML when its layout makes the note clearer than plain text, and add
structure where it helps communicate the note.
"#;

/// Extract fenced `stml` examples from a guide in source order.
#[must_use]
pub fn stml_guide_snippets(guide: &str) -> Vec<String> {
    guide
        .split("```stml\n")
        .skip(1)
        .filter_map(|tail| tail.split_once("```").map(|(snippet, _)| snippet))
        .map(|snippet| snippet.trim_end().to_owned())
        .collect()
}

/// Backward-compatible plain projection used by Workdeck note measurement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedMarkup {
    pub width: usize,
    pub lines: Vec<String>,
    pub notes: Vec<String>,
}

pub fn render(source: &str, width: usize) -> RenderedMarkup {
    let rendered = render_stml_to_text(source, width);
    RenderedMarkup {
        width,
        lines: rendered.lines,
        notes: rendered.errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn stml_theme() -> StmlThemeColors {
        StmlThemeColors {
            accent: "accent".into(),
            accent_muted: "accent-muted".into(),
            added_sign_color: "added".into(),
            removed_sign_color: "removed".into(),
            file_modified: "modified".into(),
            muted: "muted".into(),
            panel_alt: "panel-alt".into(),
            text: "text".into(),
            panel: "panel".into(),
            note_border: "note-border".into(),
            background: "background".into(),
        }
    }

    #[test]
    fn stml_colors_map_semantic_tokens_and_aliases_through_the_active_theme() {
        let theme = stml_theme();
        for (token, expected) in [
            ("accent", "accent"),
            ("info", "accent-muted"),
            ("success", "added"),
            ("danger", "removed"),
            ("error", "removed"),
            ("warning", "modified"),
            ("muted", "muted"),
            ("subtle", "panel-alt"),
            ("heading", "text"),
            ("text", "text"),
            ("panel", "panel"),
            ("bg", "panel"),
            ("note-border", "note-border"),
            ("badge-text", "background"),
        ] {
            assert_eq!(
                resolve_stml_color(Some(token), &theme).as_deref(),
                Some(expected)
            );
        }
    }

    #[test]
    fn stml_colors_accept_explicit_and_named_colors_and_reject_unknown_values() {
        let theme = stml_theme();
        assert_eq!(
            resolve_stml_color(Some(" #Aa11CC "), &theme).as_deref(),
            Some("#aa11cc")
        );
        assert_eq!(
            resolve_stml_color(Some("red"), &theme).as_deref(),
            Some("#e05252")
        );
        assert_eq!(resolve_stml_color(Some("unknown"), &theme), None);
        assert_eq!(resolve_stml_color(Some("__proto__"), &theme), None);
        assert_eq!(resolve_stml_color(Some("constructor"), &theme), None);
        assert_eq!(resolve_stml_color(None, &theme), None);
    }

    #[test]
    fn stml_guide_contains_copy_paste_snippets_for_every_core_mechanic() {
        let snippets = stml_guide_snippets(GUIDE);
        assert!(snippets.len() >= 8);
        for required in [
            "█",
            "&rarr;",
            "--width",
            "--experimental",
            "experimentalFeatures",
            &STML_REFERENCE_WIDTH.to_string(),
        ] {
            assert!(GUIDE.contains(required));
        }
    }

    #[test]
    fn stml_guide_teaches_composition_inside_workdecks_existing_note_frame() {
        assert!(GUIDE.contains("Workdeck supplies the note's outer frame"));
        assert!(GUIDE.contains("Sibling and nested boxes are supported"));
        assert!(GUIDE.contains("<box border"));
    }

    #[test]
    fn stml_guide_presents_snippets_as_mechanics_not_prescriptive_patterns() {
        assert!(GUIDE.contains("## Syntax examples"));
        assert!(GUIDE.contains("demonstrate mechanics, not preferred layouts"));
        assert!(GUIDE.contains("combine, omit, repeat, and nest"));
        assert!(!GUIDE.contains("## Patterns"));
    }

    #[test]
    fn every_stml_guide_snippet_renders_cleanly_at_the_reference_width() {
        for snippet in stml_guide_snippets(GUIDE) {
            let rendered = render(&snippet, STML_REFERENCE_WIDTH);
            assert!(
                rendered.notes.is_empty(),
                "{snippet:?}: {:?}",
                rendered.notes
            );
            assert!(!rendered.lines.is_empty());
        }
    }

    #[test]
    fn every_stml_guide_snippet_stays_within_the_reference_width() {
        for snippet in stml_guide_snippets(GUIDE) {
            let rendered = render(&snippet, STML_REFERENCE_WIDTH);
            assert!(
                rendered
                    .lines
                    .iter()
                    .all(|line| line.width() <= STML_REFERENCE_WIDTH),
                "{snippet:?}: {:?}",
                rendered.lines,
            );
        }
    }

    #[test]
    fn renders_boxes_lists_entities_and_width_bounds() {
        let rendered = render(
            "<box border title=\"flow\"><list><item>fetch &rarr; retry</item></list></box>",
            24,
        );
        assert!(rendered.lines.iter().any(|line| line.contains("flow")));
        assert!(
            rendered
                .lines
                .iter()
                .any(|line| line.contains("fetch → retry"))
        );
        assert!(rendered.lines.iter().all(|line| line.width() <= 24));
        assert!(rendered.notes.is_empty());
    }

    #[test]
    fn malformed_or_unknown_markup_degrades_with_notes() {
        let rendered = render("<future>text</future><box>", 20);
        assert!(rendered.lines.iter().any(|line| line.contains("text")));
        assert!(!rendered.notes.is_empty());
    }

    #[test]
    fn aliases_share_roles_and_classifications() {
        for tag in ["b", "strong"] {
            assert_eq!(stml_tag_role(tag), Some(StmlTagRole::Strong));
        }
        for tag in ["box", "col", "column", "stack", "section"] {
            assert_eq!(stml_tag_role(tag), Some(StmlTagRole::Container));
        }
        for tag in ["c", "color", "span", "br"] {
            assert!(is_inline_stml_role(stml_tag_role(tag)));
        }
        for tag in ["br", "hr", "rule", "divider", "spacer", "space"] {
            assert!(is_void_stml_tag(tag));
        }
        assert!(is_raw_text_stml_tag("code"));
        assert!(is_raw_text_stml_tag("pre"));
        assert!(stml_tag_role("marquee").is_none());
    }
}
