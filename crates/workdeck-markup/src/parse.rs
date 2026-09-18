//! Tolerant, terminal-safe parser translated from Hunk's MIT-licensed STML engine.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{is_raw_text_stml_tag, is_void_stml_tag};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StmlNode {
    Text {
        value: String,
    },
    Element {
        tag: String,
        attrs: BTreeMap<String, String>,
        children: Vec<StmlNode>,
    },
}

impl StmlNode {
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { value } => Some(value),
            Self::Element { .. } => None,
        }
    }

    #[must_use]
    pub fn element(&self) -> Option<StmlElementRef<'_>> {
        match self {
            Self::Element {
                tag,
                attrs,
                children,
            } => Some(StmlElementRef {
                tag,
                attrs,
                children,
            }),
            Self::Text { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StmlElementRef<'a> {
    pub tag: &'a str,
    pub attrs: &'a BTreeMap<String, String>,
    pub children: &'a [StmlNode],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StmlParseResult {
    pub nodes: Vec<StmlNode>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StmlParseOptions {
    pub max_input_bytes: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_errors: usize,
}

pub const DEFAULT_STML_PARSE_LIMITS: StmlParseOptions = StmlParseOptions {
    max_input_bytes: 64 * 1024,
    max_nodes: 2_000,
    max_depth: 32,
    max_errors: 20,
};

impl Default for StmlParseOptions {
    fn default() -> Self {
        DEFAULT_STML_PARSE_LIMITS
    }
}

#[derive(Debug)]
enum ArenaKind {
    Text(String),
    Element {
        tag: String,
        attrs: BTreeMap<String, String>,
        children: Vec<usize>,
    },
}

#[derive(Debug)]
struct ErrorCollector {
    messages: Vec<String>,
    max: usize,
    omitted: bool,
}

impl ErrorCollector {
    fn new(max: usize) -> Self {
        Self {
            messages: Vec::new(),
            max,
            omitted: false,
        }
    }

    fn add(&mut self, message: impl Into<String>) {
        if self.messages.len() < self.max {
            self.messages.push(message.into());
        } else if !self.omitted {
            self.omitted = true;
            if let Some(last) = self.messages.last_mut() {
                last.push_str(" (further parse errors omitted)");
            }
        }
    }
}

#[derive(Debug)]
struct OpenTag {
    tag: String,
    attrs: BTreeMap<String, String>,
    self_closing: bool,
    next: usize,
}

/// Decode STML's bounded entity vocabulary. Unknown entities remain literal.
#[must_use]
pub fn decode_stml_entities(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find('&') {
        let start = cursor + relative;
        output.push_str(&text[cursor..start]);
        let Some(relative_end) = text[start + 1..].find(';') else {
            output.push_str(&text[start..]);
            return output;
        };
        let end = start + 1 + relative_end;
        let body = &text[start + 1..end];
        if !valid_entity_body(body) {
            output.push('&');
            cursor = start + 1;
            continue;
        }
        if let Some(decoded) = decode_entity_body(body) {
            output.push_str(&decoded);
        } else {
            output.push_str(&text[start..=end]);
        }
        cursor = end + 1;
    }
    output.push_str(&text[cursor..]);
    output
}

fn valid_entity_body(body: &str) -> bool {
    if let Some(number) = body.strip_prefix('#') {
        if let Some(hex) = number.strip_prefix('x') {
            return !hex.is_empty() && hex.bytes().all(|byte| byte.is_ascii_hexdigit());
        }
        return !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_hexdigit());
    }
    let mut bytes = body.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric())
}

fn decode_entity_body(body: &str) -> Option<String> {
    if let Some(number) = body.strip_prefix('#') {
        let (radix, digits) = number.strip_prefix('x').map_or_else(
            || {
                (
                    10,
                    &number[..number.bytes().take_while(u8::is_ascii_digit).count()],
                )
            },
            |digits| (16, digits),
        );
        if digits.is_empty() {
            return None;
        }
        let value = u32::from_str_radix(digits, radix).ok()?;
        return char::from_u32(value).map(|character| character.to_string());
    }
    Some(
        match body.to_ascii_lowercase().as_str() {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => " ",
            "mdash" => "—",
            "ndash" => "–",
            "hellip" => "…",
            "bull" => "•",
            "middot" => "·",
            "rarr" => "→",
            "larr" => "←",
            "uarr" => "↑",
            "darr" => "↓",
            "check" => "✓",
            "cross" => "✗",
            "times" => "×",
            _ => return None,
        }
        .into(),
    )
}

/// Parse STML into a best-effort tree. Malformed markup is reported, never thrown.
#[must_use]
pub fn parse_stml(input: &str, options: StmlParseOptions) -> StmlParseResult {
    let mut errors = ErrorCollector::new(options.max_errors);
    let source = if input.len() > options.max_input_bytes {
        errors.add(format!(
            "input truncated at {} byte(s)",
            options.max_input_bytes
        ));
        truncate_utf8(input, options.max_input_bytes)
    } else {
        input
    };

    let mut arena = Vec::<ArenaKind>::new();
    let mut root = Vec::<usize>::new();
    let mut stack = Vec::<usize>::new();
    let mut cursor = 0;
    let mut node_count = 0;
    let mut node_limit_reached = false;

    while cursor < source.len() && !node_limit_reached {
        let Some(relative_lt) = source[cursor..].find('<') else {
            push_text(
                &source[cursor..],
                &mut arena,
                &mut root,
                &stack,
                &mut node_count,
                options.max_nodes,
                &mut node_limit_reached,
                &mut errors,
            );
            break;
        };
        let lt = cursor + relative_lt;
        if lt > cursor {
            push_text(
                &source[cursor..lt],
                &mut arena,
                &mut root,
                &stack,
                &mut node_count,
                options.max_nodes,
                &mut node_limit_reached,
                &mut errors,
            );
        }
        if node_limit_reached {
            break;
        }
        cursor = lt;

        if source[cursor..].starts_with("<!--") {
            cursor = source[cursor + 4..]
                .find("-->")
                .map_or(source.len(), |end| cursor + 4 + end + 3);
            continue;
        }

        if source.as_bytes().get(cursor + 1) == Some(&b'/') {
            let mut end = cursor + 2;
            while source
                .as_bytes()
                .get(end)
                .is_some_and(|byte| is_name_byte(*byte))
            {
                end += 1;
            }
            let name = source[cursor + 2..end].to_ascii_lowercase();
            while source.as_bytes().get(end).is_some_and(|byte| *byte != b'>') {
                end += 1;
            }
            cursor = end.saturating_add(1).min(source.len());
            if let Some(index) = stack.iter().rposition(
                |node| matches!(&arena[*node], ArenaKind::Element { tag, .. } if tag == &name),
            ) {
                if index != stack.len() - 1 {
                    errors.add(format!(
                        "closing </{name}> implicitly closed {} tag(s)",
                        stack.len() - 1 - index
                    ));
                }
                stack.truncate(index);
            } else {
                errors.add(format!("stray closing tag </{name}>",));
            }
            continue;
        }

        let tag_start = source.as_bytes().get(cursor + 1).copied();
        if !tag_start.is_some_and(|byte| byte.is_ascii_alphabetic()) {
            push_text(
                "<",
                &mut arena,
                &mut root,
                &stack,
                &mut node_count,
                options.max_nodes,
                &mut node_limit_reached,
                &mut errors,
            );
            cursor += 1;
            continue;
        }

        let open = read_open_tag(source, cursor);
        cursor = open.next;
        if stack.len() >= options.max_depth {
            errors.add(format!(
                "depth limit reached at <{}> ({} level(s))",
                open.tag, options.max_depth
            ));
            continue;
        }
        if !claim_node(
            &mut node_count,
            options.max_nodes,
            &mut node_limit_reached,
            &mut errors,
        ) {
            break;
        }

        let tag = open.tag;
        let node_index = arena.len();
        arena.push(ArenaKind::Element {
            tag: tag.clone(),
            attrs: open.attrs,
            children: Vec::new(),
        });
        append_child(node_index, &mut arena, &mut root, &stack);

        if open.self_closing || is_void_stml_tag(&tag) {
            continue;
        }
        if is_raw_text_stml_tag(&tag) {
            let closer = format!("</{tag}");
            let end = find_ascii_case_insensitive(source, cursor, &closer);
            let raw_end = end.unwrap_or(source.len());
            if raw_end > cursor
                && claim_node(
                    &mut node_count,
                    options.max_nodes,
                    &mut node_limit_reached,
                    &mut errors,
                )
            {
                let child = arena.len();
                arena.push(ArenaKind::Text(sanitize_stml_text(
                    &source[cursor..raw_end],
                )));
                if let ArenaKind::Element { children, .. } = &mut arena[node_index] {
                    children.push(child);
                }
            }
            if let Some(end) = end {
                cursor = source[end..]
                    .find('>')
                    .map_or(source.len(), |close| end + close + 1);
            } else {
                errors.add(format!("unclosed <{tag}>",));
                cursor = source.len();
            }
            continue;
        }
        stack.push(node_index);
    }

    if !stack.is_empty() {
        let tags = stack
            .iter()
            .filter_map(|index| match &arena[*index] {
                ArenaKind::Element { tag, .. } => Some(format!("<{tag}>")),
                ArenaKind::Text(_) => None,
            })
            .collect::<Vec<_>>()
            .join(", ");
        errors.add(format!("unclosed tag(s): {tags}"));
    }

    StmlParseResult {
        nodes: root
            .into_iter()
            .map(|index| materialize(index, &arena))
            .collect(),
        errors: errors.messages,
    }
}

#[must_use]
pub fn parse_stml_default(input: &str) -> StmlParseResult {
    parse_stml(input, StmlParseOptions::default())
}

fn claim_node(
    count: &mut usize,
    max: usize,
    reached: &mut bool,
    errors: &mut ErrorCollector,
) -> bool {
    if *count < max {
        *count += 1;
        return true;
    }
    if !*reached {
        *reached = true;
        errors.add(format!(
            "node limit reached at {max} node(s); remaining markup ignored"
        ));
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn push_text(
    value: &str,
    arena: &mut Vec<ArenaKind>,
    root: &mut Vec<usize>,
    stack: &[usize],
    node_count: &mut usize,
    max_nodes: usize,
    node_limit_reached: &mut bool,
    errors: &mut ErrorCollector,
) {
    if value.is_empty() || *node_limit_reached {
        return;
    }
    let safe = sanitize_stml_text(value);
    if safe.is_empty() {
        return;
    }
    let last = stack.last().map_or_else(
        || root.last().copied(),
        |parent| match &arena[*parent] {
            ArenaKind::Element { children, .. } => children.last().copied(),
            ArenaKind::Text(_) => None,
        },
    );
    if let Some(index) = last
        && let ArenaKind::Text(existing) = &mut arena[index]
    {
        existing.push_str(&safe);
        return;
    }
    if !claim_node(node_count, max_nodes, node_limit_reached, errors) {
        return;
    }
    let index = arena.len();
    arena.push(ArenaKind::Text(safe));
    append_child(index, arena, root, stack);
}

fn append_child(child: usize, arena: &mut [ArenaKind], root: &mut Vec<usize>, stack: &[usize]) {
    if let Some(parent) = stack.last() {
        if let ArenaKind::Element { children, .. } = &mut arena[*parent] {
            children.push(child);
        }
    } else {
        root.push(child);
    }
}

fn materialize(index: usize, arena: &[ArenaKind]) -> StmlNode {
    match &arena[index] {
        ArenaKind::Text(value) => StmlNode::Text {
            value: value.clone(),
        },
        ArenaKind::Element {
            tag,
            attrs,
            children,
        } => StmlNode::Element {
            tag: tag.clone(),
            attrs: attrs.clone(),
            children: children
                .iter()
                .map(|child| materialize(*child, arena))
                .collect(),
        },
    }
}

fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

fn is_space_byte(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

fn read_open_tag(input: &str, start: usize) -> OpenTag {
    let bytes = input.as_bytes();
    let mut cursor = start + 1;
    while bytes.get(cursor).is_some_and(|byte| is_name_byte(*byte)) {
        cursor += 1;
    }
    let tag = input[start + 1..cursor].to_ascii_lowercase();
    let mut attrs = BTreeMap::new();
    while cursor < bytes.len() {
        while bytes.get(cursor).is_some_and(|byte| is_space_byte(*byte)) {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        if bytes[cursor] == b'>' {
            return OpenTag {
                tag,
                attrs,
                self_closing: false,
                next: cursor + 1,
            };
        }
        if bytes[cursor] == b'/' && bytes.get(cursor + 1) == Some(&b'>') {
            return OpenTag {
                tag,
                attrs,
                self_closing: true,
                next: cursor + 2,
            };
        }
        let name_start = cursor;
        while bytes.get(cursor).is_some_and(|byte| is_name_byte(*byte)) {
            cursor += 1;
        }
        if cursor == name_start {
            cursor += input[cursor..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        let name = input[name_start..cursor].to_ascii_lowercase();
        while bytes.get(cursor).is_some_and(|byte| is_space_byte(*byte)) {
            cursor += 1;
        }
        let value = if bytes.get(cursor) == Some(&b'=') {
            cursor += 1;
            while bytes.get(cursor).is_some_and(|byte| is_space_byte(*byte)) {
                cursor += 1;
            }
            if matches!(bytes.get(cursor), Some(b'\'' | b'\"')) {
                let quote = bytes[cursor];
                cursor += 1;
                let value_start = cursor;
                while bytes.get(cursor).is_some_and(|byte| *byte != quote) {
                    cursor += 1;
                }
                let value = &input[value_start..cursor];
                cursor = cursor.saturating_add(1).min(bytes.len());
                value
            } else {
                let value_start = cursor;
                while bytes.get(cursor).is_some_and(|byte| {
                    !is_space_byte(*byte)
                        && *byte != b'>'
                        && !(*byte == b'/' && bytes.get(cursor + 1) == Some(&b'>'))
                }) {
                    cursor += 1;
                }
                &input[value_start..cursor]
            }
        } else {
            ""
        };
        attrs.insert(name, sanitize_stml_text(&decode_stml_entities(value)));
    }
    OpenTag {
        tag,
        attrs,
        self_closing: false,
        next: input.len(),
    }
}

fn find_ascii_case_insensitive(input: &str, from: usize, needle: &str) -> Option<usize> {
    let needle = needle.as_bytes();
    input.as_bytes()[from..]
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|relative| from + relative)
}

fn sanitize_stml_text(text: &str) -> String {
    let characters = text.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        match character {
            '\u{1b}' => {
                let Some(introducer) = characters.get(index + 1).copied() else {
                    index += 1;
                    continue;
                };
                index = match introducer {
                    '[' => consume_csi(&characters, index + 2),
                    ']' => consume_control_string(&characters, index + 2, true),
                    'P' | 'X' | '^' | '_' => consume_control_string(&characters, index + 2, false),
                    _ => None,
                }
                .unwrap_or(index + 1);
            }
            '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => {
                index = consume_control_string(&characters, index + 1, true).unwrap_or(index + 1);
            }
            '\u{9d}' => {
                index = consume_control_string(&characters, index + 1, true).unwrap_or(index + 1);
            }
            '\u{9b}' => index = consume_csi(&characters, index + 1).unwrap_or(index + 1),
            '\n' => {
                output.push(character);
                index += 1;
            }
            '\t' => index += 1,
            value if value.is_control() => index += 1,
            value => {
                output.push(value);
                index += 1;
            }
        }
    }
    output
}

fn consume_csi(characters: &[char], mut index: usize) -> Option<usize> {
    while characters
        .get(index)
        .is_some_and(|character| ('0'..='?').contains(character))
    {
        index += 1;
    }
    while characters
        .get(index)
        .is_some_and(|character| (' '..='/').contains(character))
    {
        index += 1;
    }
    characters
        .get(index)
        .is_some_and(|character| ('@'..='~').contains(character))
        .then_some(index + 1)
}

fn consume_control_string(
    characters: &[char],
    mut index: usize,
    bell_terminates: bool,
) -> Option<usize> {
    while index < characters.len() {
        if characters[index] == '\u{9c}' {
            return Some(index + 1);
        }
        if bell_terminates && characters[index] == '\u{7}' {
            return Some(index + 1);
        }
        if characters[index] == '\u{1b}' && characters.get(index + 1) == Some(&'\\') {
            return Some(index + 2);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_element(markup: &str) -> StmlElementRef<'_> {
        let parsed = Box::leak(Box::new(parse_stml_default(markup)));
        parsed
            .nodes
            .iter()
            .find_map(StmlNode::element)
            .expect("expected an element")
    }

    #[test]
    fn parses_nested_elements_with_attributes() {
        let element =
            first_element("<box border-style=\"rounded\" title=\"Auth\"><text>hi</text></box>");
        assert_eq!(element.tag, "box");
        assert_eq!(element.attrs["border-style"], "rounded");
        assert_eq!(element.attrs["title"], "Auth");
        assert_eq!(element.children.len(), 1);
        assert_eq!(element.children[0].element().unwrap().tag, "text");
    }

    #[test]
    fn keeps_bare_text_and_lone_angle_brackets_as_text() {
        let parsed = parse_stml_default("a < b and 3<4");
        assert!(parsed.errors.is_empty());
        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].text(), Some("a < b and 3<4"));
    }

    #[test]
    fn tolerates_stray_closing_tags_with_an_error_note() {
        let parsed = parse_stml_default("hello</box>");
        assert_eq!(parsed.nodes[0].text(), Some("hello"));
        assert!(parsed.errors[0].contains("stray closing tag"));
    }

    #[test]
    fn implicitly_closes_unbalanced_tags() {
        let parsed = parse_stml_default("<box><text>hi</box>");
        assert!(
            parsed
                .errors
                .iter()
                .any(|error| error.contains("implicitly closed"))
        );
        assert_eq!(parsed.nodes[0].element().unwrap().tag, "box");
    }

    #[test]
    fn reports_unclosed_tags() {
        let parsed = parse_stml_default("<box><text>hi</text>");
        assert!(
            parsed
                .errors
                .iter()
                .any(|error| error.contains("unclosed tag(s)"))
        );
    }

    #[test]
    fn treats_void_tags_as_childless() {
        let element = first_element("<text>line one<br>line two</text>");
        assert!(
            element
                .children
                .iter()
                .any(|child| { child.element().is_some_and(|element| element.tag == "br") })
        );
    }

    #[test]
    fn takes_code_content_verbatim_without_nested_parsing() {
        let element = first_element("<code>const a = <b>1</b>;</code>");
        assert_eq!(element.children.len(), 1);
        assert_eq!(element.children[0].text(), Some("const a = <b>1</b>;"));
    }

    #[test]
    fn strips_terminal_control_sequences_from_text_and_attributes() {
        let parsed = parse_stml_default("<text fg=\"\u{1b}[31mred\">danger\u{1b}[2Jzone</text>");
        let element = parsed.nodes[0].element().unwrap();
        assert_eq!(element.attrs["fg"], "red");
        assert_eq!(element.children[0].text(), Some("dangerzone"));
    }

    #[test]
    fn ignores_comments() {
        let parsed = parse_stml_default("<!-- hidden -->visible");
        assert_eq!(parsed.nodes[0].text(), Some("visible"));
    }

    #[test]
    fn enforces_the_node_limit_without_throwing() {
        let parsed = parse_stml(
            &"<b>x</b>".repeat(50),
            StmlParseOptions {
                max_nodes: 10,
                ..StmlParseOptions::default()
            },
        );
        assert!(
            parsed
                .errors
                .iter()
                .any(|error| error.contains("node limit"))
        );
    }

    #[test]
    fn enforces_the_depth_limit_without_throwing() {
        let markup = format!("{}hi{}", "<box>".repeat(40), "</box>".repeat(40));
        let parsed = parse_stml(
            &markup,
            StmlParseOptions {
                max_depth: 5,
                ..StmlParseOptions::default()
            },
        );
        assert!(
            parsed
                .errors
                .iter()
                .any(|error| error.contains("depth limit"))
        );
    }

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(
            decode_stml_entities("&lt;a&gt; &amp; &#65;&#x42;"),
            "<a> & AB"
        );
    }

    #[test]
    fn keeps_unknown_and_out_of_range_entities_literal() {
        assert_eq!(
            decode_stml_entities("&unknown; &#x110000;"),
            "&unknown; &#x110000;"
        );
    }

    #[test]
    fn numeric_entities_match_javascript_parse_int_edges() {
        assert_eq!(decode_stml_entities("&#12A;"), "\u{c}");
        assert_eq!(decode_stml_entities("&#A;"), "&#A;");
        assert_eq!(decode_stml_entities("&#X42;"), "&#X42;");
    }

    #[test]
    fn unterminated_or_malformed_control_sequences_keep_non_control_payload() {
        let parsed =
            parse_stml_default("<text>a\u{1b}]unterminated b\u{1b}[éJ c\u{90}payload</text>");
        assert_eq!(
            parsed.nodes[0].element().unwrap().children[0].text(),
            Some("a]unterminated b[éJ cpayload")
        );
    }
}
