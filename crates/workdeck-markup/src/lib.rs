//! Small, bounded STML renderer for rich agent-note fallbacks.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use serde::{Deserialize, Serialize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const DEFAULT_WIDTH: usize = 56;
pub const MAX_MARKUP_BYTES: usize = 1024 * 1024;

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

pub const GUIDE: &str = r#"# STML - terminal markup for Workdeck agent notes

STML is small HTML-like markup rendered by Workdeck without a browser or JavaScript runtime.
Plain `summary` text remains the required fallback for agent annotations.

Block tags: box, card, section, col, row, text, p, h1, h2, h3, list, ul, ol,
item, hr, spacer, code, pre.
Inline tags: b, i, u, s, dim, c, color, kbd, badge, a, br.

Colors use symbolic theme tokens: accent, success, warning, danger, info, muted,
subtle, and heading. Unknown tags degrade to text and produce a render note.

Example:

    <h2>Retry flow</h2>
    <row gap="1"><box border>fetch</box><badge color="warning">retry</badge></row>
    <list><item>bounded attempts</item><item>exponential backoff</item></list>

Preview safely before attaching markup:

    workdeck markup render note.stml --width 56
    workdeck markup render - --json
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedMarkup {
    pub width: usize,
    pub lines: Vec<String>,
    pub notes: Vec<String>,
}

pub fn render(source: &str, width: usize) -> RenderedMarkup {
    let width = width.max(1);
    if source.len() > MAX_MARKUP_BYTES {
        return RenderedMarkup {
            width,
            lines: vec!["[markup omitted: size limit exceeded]".into()],
            notes: vec![format!("markup exceeded {MAX_MARKUP_BYTES} bytes")],
        };
    }
    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(false);
    let mut output = String::new();
    let mut notes = Vec::new();
    let mut list_depth = 0_usize;
    let mut box_depth = 0_usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(tag)) => {
                let name = String::from_utf8_lossy(tag.name().as_ref()).to_ascii_lowercase();
                match name.as_str() {
                    "h1" | "h2" | "h3" | "p" | "text" | "section" | "col" | "code" | "pre" => {
                        ensure_newline(&mut output)
                    }
                    "list" | "ul" | "ol" => {
                        ensure_newline(&mut output);
                        list_depth = list_depth.saturating_add(1);
                    }
                    "item" => {
                        ensure_newline(&mut output);
                        output.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                        output.push_str("- ");
                    }
                    "row" => ensure_newline(&mut output),
                    "box" | "card" => {
                        ensure_newline(&mut output);
                        let title = attribute(&tag, b"title");
                        output.push('┌');
                        if let Some(title) = title {
                            output.push(' ');
                            output.push_str(&title);
                            output.push(' ');
                        }
                        output.push_str(
                            &"─".repeat(width.saturating_sub(output_line_width(&output) + 1)),
                        );
                        output.push('┐');
                        output.push('\n');
                        box_depth = box_depth.saturating_add(1);
                    }
                    "b" | "i" | "u" | "s" | "dim" | "c" | "color" | "kbd" | "badge" | "a" => {}
                    other => notes.push(format!("unknown STML tag <{other}> was rendered as text")),
                }
            }
            Ok(Event::Empty(tag)) => {
                let name = String::from_utf8_lossy(tag.name().as_ref()).to_ascii_lowercase();
                match name.as_str() {
                    "br" => output.push('\n'),
                    "hr" => {
                        ensure_newline(&mut output);
                        output.push_str(&"─".repeat(width));
                        output.push('\n');
                    }
                    "spacer" => {
                        let size = attribute(&tag, b"size")
                            .and_then(|size| size.parse::<usize>().ok())
                            .unwrap_or(1)
                            .min(8);
                        output.extend(std::iter::repeat_n('\n', size));
                    }
                    other => notes.push(format!("unknown empty STML tag <{other}/> was ignored")),
                }
            }
            Ok(Event::End(tag)) => {
                let name = String::from_utf8_lossy(tag.name().as_ref()).to_ascii_lowercase();
                match name.as_str() {
                    "h1" | "h2" | "h3" | "p" | "text" | "section" | "col" | "row" | "item"
                    | "code" | "pre" => ensure_newline(&mut output),
                    "list" | "ul" | "ol" => {
                        list_depth = list_depth.saturating_sub(1);
                        ensure_newline(&mut output);
                    }
                    "box" | "card" => {
                        ensure_newline(&mut output);
                        output.push('└');
                        output.push_str(&"─".repeat(width.saturating_sub(2)));
                        output.push('┘');
                        output.push('\n');
                        box_depth = box_depth.saturating_sub(1);
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(text)) => match text.decode() {
                Ok(text) => output.push_str(&text),
                Err(error) => notes.push(format!("invalid STML text: {error}")),
            },
            Ok(Event::GeneralRef(reference)) => match reference.decode() {
                Ok(reference) => output.push_str(&decode_entity(&reference)),
                Err(error) => notes.push(format!("invalid STML entity: {error}")),
            },
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => {
                notes.push(format!("malformed STML degraded to parsed text: {error}"));
                break;
            }
        }
    }
    if box_depth > 0 {
        notes.push("unclosed STML box".into());
    }
    let mut lines = Vec::new();
    for line in output.lines() {
        wrap_line(line.trim_end(), width, &mut lines);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    RenderedMarkup {
        width,
        lines,
        notes,
    }
}

fn attribute(tag: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    tag.attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == name)
        .and_then(|attribute| {
            attribute
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
        })
        .map(|value| value.into_owned())
}

fn decode_entity(entity: &str) -> String {
    match entity {
        "rarr" => "→".into(),
        "check" => "✓".into(),
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        other => format!("&{other};"),
    }
}

fn ensure_newline(output: &mut String) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
}

fn output_line_width(output: &str) -> usize {
    output.rsplit('\n').next().unwrap_or_default().width()
}

fn wrap_line(line: &str, width: usize, lines: &mut Vec<String>) {
    if line.is_empty() {
        lines.push(String::new());
        return;
    }
    let mut current = String::new();
    let mut used = 0;
    for character in line.chars() {
        let cell_width = character.width().unwrap_or(0);
        if used > 0 && used + cell_width > width {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(character);
        used += cell_width;
    }
    lines.push(current);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_boxes_lists_entities_and_width_bounds() {
        let rendered = render(
            "<box title=\"flow\"><list><item>fetch &rarr; retry</item></list></box>",
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
