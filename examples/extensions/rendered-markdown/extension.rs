//! Native, host-rendered port of Hunk's optional Marked-based Markdown file view.

use std::io::{self, BufRead, Write};
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag};
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionFileChangeKind, ExtensionFileChangeRange, ExtensionFileSide,
    ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewSourceRange, ExtensionFileViewSpan, ExtensionFileViewTone,
    ExtensionHostAction, ExtensionTextAttribute, FileViewLayoutRequest, FileViewMatchRequest,
    HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Registration,
};

pub const VIEW_ID: &str = "rendered-markdown";
pub const MAX_MARKDOWN_SOURCE_LENGTH: usize = 200_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedMarkdownRow {
    spans: Vec<ExtensionFileViewSpan>,
    source_range: [usize; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BlockKind {
    Heading,
    Paragraph,
    BlockQuote,
    List(Option<u64>),
    Table,
    Code { fenced: bool, language: String },
    Html,
    Rule,
    Definition,
    Space,
}

#[derive(Debug, Clone)]
struct ParsedBlock {
    kind: BlockKind,
    events: Range<usize>,
    byte_start: usize,
    byte_end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SpanPresentation {
    tone: Option<ExtensionFileViewTone>,
    attributes: Vec<ExtensionTextAttribute>,
}

impl SpanPresentation {
    fn with_attribute(&self, attribute: ExtensionTextAttribute) -> Self {
        let mut next = self.clone();
        next.attributes.push(attribute);
        next
    }

    fn with_tone(&self, tone: ExtensionFileViewTone) -> Self {
        let mut next = self.clone();
        next.tone = Some(tone);
        next
    }

    fn span(&self, text: impl Into<String>) -> ExtensionFileViewSpan {
        ExtensionFileViewSpan {
            text: text.into(),
            tone: self.tone,
            attributes: self.attributes.clone(),
        }
    }
}

#[derive(Debug)]
struct SourceIndex {
    length: usize,
    line_starts: Vec<usize>,
}

impl SourceIndex {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            length: source.len(),
            line_starts,
        }
    }

    fn line_at(&self, offset: usize) -> usize {
        let target = offset.min(self.length);
        self.line_starts
            .partition_point(|start| *start <= target)
            .max(1)
    }

    fn range(&self, offset: usize, length: usize) -> [usize; 2] {
        [
            self.line_at(offset),
            self.line_at(offset.saturating_add(length.saturating_sub(1))),
        ]
    }
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::Command(CommandRegistration {
            id: "toggle-rendered-markdown".into(),
            title: "Toggle rendered Markdown".into(),
            description: None,
            default_keys: vec!["f8".into()],
        }),
        Registration::FileView {
            id: VIEW_ID.into(),
            title: "Rendered Markdown".into(),
            priority: 0,
            interactive_mode: false,
        },
    ]
}

#[must_use]
pub fn required_capabilities() -> Vec<Capability> {
    vec![Capability::Commands, Capability::FileViews]
}

#[must_use]
pub fn matches_markdown_file(file: &workdeck_extension_api::ExtensionDiffFile) -> bool {
    let path = file.path.to_ascii_lowercase();
    (path.ends_with(".md") || path.ends_with(".mdown")) && !file.is_binary && !file.is_too_large
}

/// Reject ambiguous unterminated fenced blocks instead of previewing guessed structure.
#[must_use]
pub fn has_unterminated_fence(source: &str) -> bool {
    let mut open: Option<(u8, usize)> = None;
    for line in source.split('\n') {
        let trimmed = line.trim_start_matches(' ');
        if line.len().saturating_sub(trimmed.len()) > 3 {
            continue;
        }
        let Some(marker) = trimmed.as_bytes().first().copied() else {
            continue;
        };
        if !matches!(marker, b'`' | b'~') {
            continue;
        }
        let run = trimmed.bytes().take_while(|byte| *byte == marker).count();
        if run < 3 {
            continue;
        }
        match open {
            None => open = Some((marker, run)),
            Some((open_marker, open_length))
                if marker == open_marker
                    && run >= open_length
                    && trimmed[run..].trim().is_empty() =>
            {
                open = None;
            }
            Some(_) => {}
        }
    }
    open.is_some()
}

/// Parse and render Markdown while retaining source ranges for hunk geometry.
#[must_use]
pub fn render_markdown(source: &str, width: usize) -> Vec<ExtensionFileViewRow> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let events = markdown_events(source, options);
    let blocks = parsed_blocks(source, &events);
    let source_index = SourceIndex::new(source);
    let rendered = blocks
        .iter()
        .flat_map(|block| render_block(block, &events, &source_index, width))
        .collect::<Vec<_>>();
    bind_rows(rendered)
}

fn parsed_blocks(source: &str, events: &[(Event<'_>, Range<usize>)]) -> Vec<ParsedBlock> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < events.len() {
        match &events[index].0 {
            Event::Start(tag) if block_kind(tag).is_some() => {
                let kind = block_kind(tag).expect("checked block tag");
                let end = matching_end(events, index).unwrap_or(index);
                let byte_start = events[index].1.start;
                let mut byte_end = events[end].1.end.max(events[index].1.end);
                if !matches!(
                    kind,
                    BlockKind::Heading
                        | BlockKind::Table
                        | BlockKind::Html
                        | BlockKind::Code { fenced: false, .. }
                ) {
                    while byte_end > byte_start
                        && source.as_bytes()[byte_end - 1].is_ascii_whitespace()
                    {
                        byte_end -= 1;
                    }
                }
                blocks.push(ParsedBlock {
                    kind,
                    events: index..end.saturating_add(1),
                    byte_start,
                    byte_end,
                });
                index = end.saturating_add(1);
            }
            Event::Rule => {
                blocks.push(ParsedBlock {
                    kind: BlockKind::Rule,
                    events: index..index + 1,
                    byte_start: events[index].1.start,
                    byte_end: events[index].1.end,
                });
                index += 1;
            }
            _ => index += 1,
        }
    }

    for definition in markdown_definition_ranges(source) {
        if !blocks
            .iter()
            .any(|block| block.byte_start <= definition.start && definition.start < block.byte_end)
        {
            blocks.push(ParsedBlock {
                kind: BlockKind::Definition,
                events: 0..0,
                byte_start: definition.start,
                byte_end: definition.end,
            });
        }
    }
    blocks.sort_by_key(|block| block.byte_start);

    let mut with_spaces = Vec::new();
    let mut previous_end = 0;
    let mut previous_absorbs_gap = false;
    for block in blocks {
        if block.byte_start > previous_end {
            // pulldown-cmark includes the first separator newline in several
            // preceding block ranges, while Marked exposes the complete blank
            // separator as a `space` token. Include that boundary byte when
            // deciding whether the symbolic blank row exists.
            let gap_start = previous_end.saturating_sub(1);
            let gap = &source[gap_start..block.byte_start];
            if !previous_absorbs_gap && has_blank_separator(gap) {
                with_spaces.push(ParsedBlock {
                    kind: BlockKind::Space,
                    events: 0..0,
                    byte_start: previous_end,
                    byte_end: block.byte_start,
                });
            } else if previous_absorbs_gap && let Some(previous) = with_spaces.last_mut() {
                previous.byte_end = source[..block.byte_start]
                    .rfind('\n')
                    .map_or(block.byte_start, |line_break| line_break + 1);
            }
        }
        previous_end = block.byte_end;
        previous_absorbs_gap = matches!(
            block.kind,
            BlockKind::Heading
                | BlockKind::Table
                | BlockKind::Html
                | BlockKind::Code { fenced: false, .. }
        );
        with_spaces.push(block);
    }
    with_spaces
}

fn has_blank_separator(gap: &str) -> bool {
    gap.as_bytes().windows(2).any(|pair| pair == b"\n\n")
        || gap.as_bytes().windows(4).any(|pair| pair == b"\r\n\r\n")
}

fn markdown_definition_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let without_newline = line.strip_suffix('\n').unwrap_or(line);
        let trimmed = without_newline.trim_start_matches(' ');
        let indent = without_newline.len().saturating_sub(trimmed.len());
        let is_definition = indent <= 3
            && trimmed.starts_with('[')
            && trimmed
                .find("]: ")
                .or_else(|| trimmed.find("]:"))
                .is_some_and(|end| end > 1);
        if is_definition {
            ranges.push(offset..offset + without_newline.len());
        }
        offset += line.len();
    }
    ranges
}

fn markdown_events(source: &str, options: Options) -> Vec<(Event<'_>, Range<usize>)> {
    Parser::new_ext(source, options)
        .into_offset_iter()
        .map(|(event, range)| {
            let event = match event {
                Event::Text(_text) if source[range.clone()].contains('&') => Event::Text(
                    CowStr::from(decode_markdown_entities(&source[range.clone()])),
                ),
                event => event,
            };
            (event, range)
        })
        .collect()
}

fn decode_markdown_entities(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find('&') {
        decoded.push_str(&remaining[..start]);
        let entity_start = &remaining[start..];
        let Some(relative_end) = entity_start.find(';') else {
            decoded.push_str(entity_start);
            return decoded;
        };
        let entity = &entity_start[..=relative_end];
        let body = &entity[1..entity.len() - 1];
        let replacement = body
            .strip_prefix("#x")
            .or_else(|| body.strip_prefix("#X"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32)
            .or_else(|| {
                body.strip_prefix('#')
                    .and_then(|decimal| decimal.parse::<u32>().ok())
                    .and_then(char::from_u32)
            })
            .or_else(|| match body.to_ascii_lowercase().as_str() {
                "amp" => Some('&'),
                "apos" => Some('\''),
                "gt" => Some('>'),
                "lt" => Some('<'),
                "quot" => Some('"'),
                _ => None,
            });
        if let Some(replacement) = replacement {
            decoded.push(replacement);
        } else {
            decoded.push_str(entity);
        }
        remaining = &entity_start[relative_end + 1..];
    }
    decoded.push_str(remaining);
    decoded
}

fn block_kind(tag: &Tag<'_>) -> Option<BlockKind> {
    match tag {
        Tag::Heading { .. } => Some(BlockKind::Heading),
        Tag::Paragraph => Some(BlockKind::Paragraph),
        Tag::BlockQuote(_) => Some(BlockKind::BlockQuote),
        Tag::List(start) => Some(BlockKind::List(*start)),
        Tag::Table(_) => Some(BlockKind::Table),
        Tag::CodeBlock(kind) => Some(match kind {
            CodeBlockKind::Indented => BlockKind::Code {
                fenced: false,
                language: String::new(),
            },
            CodeBlockKind::Fenced(language) => BlockKind::Code {
                fenced: true,
                language: language.trim().to_owned(),
            },
        }),
        Tag::HtmlBlock => Some(BlockKind::Html),
        Tag::Item
        | Tag::TableHead
        | Tag::TableRow
        | Tag::TableCell
        | Tag::Emphasis
        | Tag::Strong
        | Tag::Strikethrough
        | Tag::Link { .. }
        | Tag::Image { .. }
        | Tag::Superscript
        | Tag::Subscript
        | Tag::FootnoteDefinition(_)
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::MetadataBlock(_) => None,
    }
}

fn matching_end(events: &[(Event<'_>, Range<usize>)], start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, (event, _)) in events.iter().enumerate().skip(start) {
        match event {
            Event::Start(_) => depth = depth.saturating_add(1),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn render_block(
    block: &ParsedBlock,
    events: &[(Event<'_>, Range<usize>)],
    source_index: &SourceIndex,
    width: usize,
) -> Vec<RenderedMarkdownRow> {
    let source_range = source_index.range(
        block.byte_start,
        block.byte_end.saturating_sub(block.byte_start),
    );
    let inner = block_inner(block, events);
    let rows = |lines: Vec<Vec<ExtensionFileViewSpan>>, fallback: SpanPresentation| {
        lines
            .into_iter()
            .map(|spans| RenderedMarkdownRow {
                spans: non_empty_spans(spans, &fallback),
                source_range,
            })
            .collect::<Vec<_>>()
    };
    match &block.kind {
        BlockKind::Space => rows(vec![Vec::new()], SpanPresentation::default()),
        BlockKind::Heading => {
            let heading = SpanPresentation {
                tone: Some(ExtensionFileViewTone::Accent),
                attributes: vec![ExtensionTextAttribute::Bold],
            };
            rows(render_inline(inner, &heading), heading)
        }
        BlockKind::Paragraph => rows(
            render_inline(inner, &SpanPresentation::default()),
            SpanPresentation::default(),
        ),
        BlockKind::BlockQuote => {
            let quote = SpanPresentation {
                tone: Some(ExtensionFileViewTone::AccentMuted),
                attributes: Vec::new(),
            };
            rows(render_inline(inner, &quote), quote.clone())
                .into_iter()
                .map(|mut row| {
                    row.spans.insert(0, quote.span("│ "));
                    row
                })
                .collect()
        }
        BlockKind::List(start) => render_list(inner, source_range, *start),
        BlockKind::Table => render_table(inner, source_range),
        BlockKind::Code { fenced, language } => render_code(inner, source_range, *fenced, language),
        BlockKind::Html => {
            let visible = strip_html(&event_text(inner)).trim().to_owned();
            rows(
                vec![vec![SpanPresentation::default().span(
                    if visible.is_empty() {
                        " ".to_owned()
                    } else {
                        visible
                    },
                )]],
                SpanPresentation::default(),
            )
        }
        BlockKind::Definition => rows(vec![Vec::new()], SpanPresentation::default()),
        BlockKind::Rule => rows(
            vec![vec![
                SpanPresentation {
                    tone: Some(ExtensionFileViewTone::Muted),
                    attributes: Vec::new(),
                }
                .span("─".repeat(width.max(1))),
            ]],
            SpanPresentation::default(),
        ),
    }
}

fn block_inner<'a>(
    block: &ParsedBlock,
    events: &'a [(Event<'a>, Range<usize>)],
) -> &'a [(Event<'a>, Range<usize>)] {
    if block.events.is_empty() {
        return &[];
    }
    let slice = &events[block.events.clone()];
    if matches!(slice.first().map(|item| &item.0), Some(Event::Start(_)))
        && matches!(slice.last().map(|item| &item.0), Some(Event::End(_)))
    {
        &slice[1..slice.len().saturating_sub(1)]
    } else {
        slice
    }
}

fn render_inline(
    events: &[(Event<'_>, Range<usize>)],
    presentation: &SpanPresentation,
) -> Vec<Vec<ExtensionFileViewSpan>> {
    let mut lines = vec![Vec::new()];
    let mut index = 0;
    while index < events.len() {
        match &events[index].0 {
            Event::Text(text) => append_inline_text(&mut lines, text, presentation),
            Event::Code(text) => append_inline_text(
                &mut lines,
                text,
                &SpanPresentation {
                    tone: Some(ExtensionFileViewTone::Syntax),
                    attributes: Vec::new(),
                },
            ),
            Event::SoftBreak | Event::HardBreak => lines.push(Vec::new()),
            Event::TaskListMarker(checked) => append_inline_text(
                &mut lines,
                if *checked { "[x] " } else { "[ ] " },
                presentation,
            ),
            Event::Html(text) | Event::InlineHtml(text) => {
                let visible = strip_html(text);
                if !visible.is_empty() {
                    append_inline_text(&mut lines, &visible, presentation);
                }
            }
            Event::Start(tag) => {
                let Some(end) = matching_end(events, index) else {
                    index += 1;
                    continue;
                };
                let nested_events = &events[index + 1..end];
                let nested = match tag {
                    Tag::Strong => render_inline(
                        nested_events,
                        &presentation.with_attribute(ExtensionTextAttribute::Bold),
                    ),
                    Tag::Emphasis => render_inline(
                        nested_events,
                        &presentation.with_attribute(ExtensionTextAttribute::Italic),
                    ),
                    Tag::Strikethrough => render_inline(
                        nested_events,
                        &presentation
                            .with_tone(ExtensionFileViewTone::Muted)
                            .with_attribute(ExtensionTextAttribute::Strikethrough),
                    ),
                    Tag::Link { dest_url, .. } => {
                        let link = presentation
                            .with_tone(ExtensionFileViewTone::Accent)
                            .with_attribute(ExtensionTextAttribute::Underline);
                        let mut nested = render_inline(nested_events, &link);
                        let text = nested
                            .iter()
                            .flatten()
                            .map(|span| span.text.as_str())
                            .collect::<String>();
                        if !dest_url.is_empty() && dest_url.as_ref() != text {
                            append_inline_text(
                                &mut nested,
                                &format!(" <{dest_url}>"),
                                &SpanPresentation {
                                    tone: Some(ExtensionFileViewTone::Muted),
                                    attributes: Vec::new(),
                                },
                            );
                        }
                        nested
                    }
                    Tag::Image { dest_url, .. } => {
                        let alt = event_text(nested_events);
                        vec![vec![
                            SpanPresentation {
                                tone: Some(ExtensionFileViewTone::Accent),
                                attributes: vec![ExtensionTextAttribute::Underline],
                            }
                            .span(format!(
                                "▣ {}",
                                if alt.is_empty() {
                                    dest_url.as_ref()
                                } else {
                                    &alt
                                }
                            )),
                        ]]
                    }
                    _ => render_inline(nested_events, presentation),
                };
                append_inline_lines(&mut lines, nested);
                index = end;
            }
            Event::End(_)
            | Event::Rule
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_) => {}
        }
        index += 1;
    }
    lines
}

fn append_inline_text(
    lines: &mut Vec<Vec<ExtensionFileViewSpan>>,
    text: &str,
    presentation: &SpanPresentation,
) {
    for (index, part) in text.split('\n').enumerate() {
        if index > 0 {
            lines.push(Vec::new());
        }
        if !part.is_empty() {
            let line = lines.last_mut().expect("inline output owns one line");
            if let Some(previous) = line.last_mut()
                && previous.tone == presentation.tone
                && previous.attributes == presentation.attributes
                && !matches!(previous.text.as_str(), "[x] " | "[ ] ")
            {
                previous.text.push_str(part);
            } else {
                line.push(presentation.span(part));
            }
        }
    }
}

fn append_inline_lines(
    target: &mut Vec<Vec<ExtensionFileViewSpan>>,
    nested: Vec<Vec<ExtensionFileViewSpan>>,
) {
    let mut nested = nested.into_iter();
    if let Some(first) = nested.next() {
        target
            .last_mut()
            .expect("inline output owns one line")
            .extend(first);
    }
    target.extend(nested);
}

fn non_empty_spans(
    spans: Vec<ExtensionFileViewSpan>,
    presentation: &SpanPresentation,
) -> Vec<ExtensionFileViewSpan> {
    if spans.is_empty() {
        vec![presentation.span(" ")]
    } else {
        spans
    }
}

fn render_list(
    events: &[(Event<'_>, Range<usize>)],
    source_range: [usize; 2],
    start: Option<u64>,
) -> Vec<RenderedMarkdownRow> {
    let item_ranges = direct_child_ranges(events, |tag| matches!(tag, Tag::Item));
    let mut output = Vec::new();
    for (item_index, item_range) in item_ranges.into_iter().enumerate() {
        let item_events = &events[item_range];
        let task = item_events.iter().find_map(|(event, _)| match event {
            Event::TaskListMarker(checked) => Some(*checked),
            _ => None,
        });
        let marker = match task {
            Some(true) => "[x] ".to_owned(),
            Some(false) => "[ ] ".to_owned(),
            None => start.map_or_else(
                || "• ".to_owned(),
                |start| format!("{}. ", start.saturating_add(item_index as u64)),
            ),
        };
        let item_lines = render_inline(item_events, &SpanPresentation::default());
        for (line_index, spans) in item_lines.into_iter().enumerate() {
            let prefix = if line_index == 0 {
                marker.clone()
            } else {
                " ".repeat(marker.chars().count())
            };
            let mut row_spans = vec![
                SpanPresentation {
                    tone: Some(ExtensionFileViewTone::Muted),
                    attributes: Vec::new(),
                }
                .span(prefix),
            ];
            row_spans.extend(non_empty_spans(spans, &SpanPresentation::default()));
            output.push(RenderedMarkdownRow {
                spans: row_spans,
                source_range,
            });
        }
    }
    output
}

fn direct_child_ranges(
    events: &[(Event<'_>, Range<usize>)],
    matches_tag: impl Fn(&Tag<'_>) -> bool,
) -> Vec<Range<usize>> {
    let mut output = Vec::new();
    let mut index = 0;
    while index < events.len() {
        if let Event::Start(tag) = &events[index].0 {
            let end = matching_end(events, index).unwrap_or(index);
            if matches_tag(tag) {
                output.push(index + 1..end);
            }
            index = end.saturating_add(1);
        } else {
            index += 1;
        }
    }
    output
}

fn render_table(
    events: &[(Event<'_>, Range<usize>)],
    source_range: [usize; 2],
) -> Vec<RenderedMarkdownRow> {
    let mut table_rows = Vec::<Vec<String>>::new();
    for row_range in
        direct_child_ranges(events, |tag| matches!(tag, Tag::TableHead | Tag::TableRow))
    {
        let row_events = &events[row_range];
        let cells = direct_child_ranges(row_events, |tag| matches!(tag, Tag::TableCell))
            .into_iter()
            .map(|cell_range| {
                render_inline(&row_events[cell_range], &SpanPresentation::default())
                    .into_iter()
                    .flatten()
                    .map(|span| span.text)
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        table_rows.push(cells);
    }
    let Some(header) = table_rows.first() else {
        return Vec::new();
    };
    let mut output = vec![RenderedMarkdownRow {
        spans: vec![SpanPresentation::default().span(header.join(" │ "))],
        source_range: [source_range[0], source_range[0]],
    }];
    let divider_line = (source_range[0] + 1).min(source_range[1]);
    output.push(RenderedMarkdownRow {
        spans: vec![
            SpanPresentation {
                tone: Some(ExtensionFileViewTone::Muted),
                attributes: Vec::new(),
            }
            .span(
                header
                    .iter()
                    .map(|cell| "─".repeat(cell.chars().count().max(1)))
                    .collect::<Vec<_>>()
                    .join("─┼─"),
            ),
        ],
        source_range: [divider_line, divider_line],
    });
    for (index, cells) in table_rows.into_iter().skip(1).enumerate() {
        let source_line = (source_range[0] + index + 2).min(source_range[1]);
        output.push(RenderedMarkdownRow {
            spans: vec![SpanPresentation::default().span(cells.join(" │ "))],
            source_range: [source_line, source_line],
        });
    }
    output
}

fn render_code(
    events: &[(Event<'_>, Range<usize>)],
    source_range: [usize; 2],
    fenced: bool,
    language: &str,
) -> Vec<RenderedMarkdownRow> {
    let mut code = event_text(events);
    if code.ends_with('\n') {
        code.pop();
    }
    let muted = SpanPresentation {
        tone: Some(ExtensionFileViewTone::Muted),
        attributes: Vec::new(),
    };
    let syntax = SpanPresentation {
        tone: Some(ExtensionFileViewTone::Syntax),
        attributes: Vec::new(),
    };
    let mut output = Vec::new();
    if fenced {
        output.push(RenderedMarkdownRow {
            spans: vec![muted.span(if language.is_empty() {
                "┌─".to_owned()
            } else {
                format!("┌─ {language}")
            })],
            source_range: [source_range[0], source_range[0]],
        });
    }
    for (index, line) in code.split('\n').enumerate() {
        let source_line = (source_range[0] + index + usize::from(fenced)).min(source_range[1]);
        output.push(RenderedMarkdownRow {
            spans: vec![syntax.span(format!(
                "{}{}",
                if fenced { "│ " } else { "  " },
                if line.is_empty() { " " } else { line }
            ))],
            source_range: [source_line, source_line],
        });
    }
    if fenced {
        output.push(RenderedMarkdownRow {
            spans: vec![muted.span("└─")],
            source_range: [source_range[1], source_range[1]],
        });
    }
    output
}

fn event_text(events: &[(Event<'_>, Range<usize>)]) -> String {
    events
        .iter()
        .filter_map(|(event, _)| match event {
            Event::Text(text)
            | Event::Code(text)
            | Event::Html(text)
            | Event::InlineHtml(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text) => Some(text.as_ref()),
            _ => None,
        })
        .collect()
}

fn strip_html(source: &str) -> String {
    let mut visible = String::new();
    let mut in_tag = false;
    for character in source.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => visible.push(character),
            _ => {}
        }
    }
    visible
}

fn bind_rows(rows: Vec<RenderedMarkdownRow>) -> Vec<ExtensionFileViewRow> {
    let mut last_bound_line = 0;
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let source_ranges = if row.source_range[0] > last_bound_line {
                last_bound_line = row.source_range[1];
                vec![ExtensionFileViewSourceRange {
                    side: ExtensionFileSide::New,
                    range: row.source_range,
                }]
            } else {
                Vec::new()
            };
            ExtensionFileViewRow {
                id: format!("rendered:{index}"),
                spans: row.spans,
                source_ranges,
                component: None,
            }
        })
        .collect()
}

fn row_was_added(row: &RenderedMarkdownRow, changes: &[ExtensionFileChangeRange]) -> bool {
    changes.iter().any(|change| {
        change.kind == ExtensionFileChangeKind::Added
            && change.range[0] <= row.source_range[1]
            && change.range[1] >= row.source_range[0]
    })
}

fn rendered_bounds(rows: &[RenderedMarkdownRow], range: [usize; 2]) -> [usize; 2] {
    let overlapping = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            (row.source_range[0] <= range[1] && row.source_range[1] >= range[0]).then_some(index)
        })
        .collect::<Vec<_>>();
    if let (Some(first), Some(last)) = (overlapping.first(), overlapping.last()) {
        return [*first, *last];
    }
    let nearest = rows
        .iter()
        .enumerate()
        .min_by_key(|(_, row)| {
            if range[0] < row.source_range[0] {
                row.source_range[0] - range[0]
            } else {
                range[0].saturating_sub(row.source_range[1])
            }
        })
        .map_or(0, |(index, _)| index);
    [nearest, nearest]
}

/// Build a parsed, host-rendered Markdown preview from an immutable request.
#[must_use]
pub fn create_rendered_markdown_layout(
    request: &FileViewLayoutRequest,
) -> Option<ExtensionFileViewLayout> {
    let source = request
        .documents
        .get(&ExtensionFileSide::New)
        .and_then(Option::as_deref)?;
    if source.is_empty()
        || source.encode_utf16().count() > MAX_MARKDOWN_SOURCE_LENGTH
        || request.aborted
        || has_unterminated_fence(source)
    {
        return None;
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let events = markdown_events(source, options);
    let blocks = parsed_blocks(source, &events);
    let source_index = SourceIndex::new(source);
    let mut rendered_rows = blocks
        .iter()
        .flat_map(|block| render_block(block, &events, &source_index, request.width))
        .collect::<Vec<_>>();
    if rendered_rows.is_empty() {
        return None;
    }
    for row in &mut rendered_rows {
        if row_was_added(row, &request.changes) {
            for span in &mut row.spans {
                span.tone = Some(ExtensionFileViewTone::Added);
            }
        }
    }
    let mut rows = bind_rows(rendered_rows.clone());
    let hunk_rows = request
        .file
        .hunks
        .iter()
        .map(|hunk| {
            let changed = request
                .changes
                .iter()
                .filter(|change| {
                    change.hunk_index == hunk.index && change.kind == ExtensionFileChangeKind::Added
                })
                .collect::<Vec<_>>();
            let source_range = if changed.is_empty() {
                hunk.new_range
                    .map_or([1, 1], |range| [range[0] as usize, range[1] as usize])
            } else {
                [
                    changed
                        .iter()
                        .map(|change| change.range[0])
                        .min()
                        .expect("non-empty changes"),
                    changed
                        .iter()
                        .map(|change| change.range[1])
                        .max()
                        .expect("non-empty changes"),
                ]
            };
            let [start_row, end_row] = rendered_bounds(&rendered_rows, source_range);
            ExtensionFileViewHunkRows { start_row, end_row }
        })
        .collect::<Vec<_>>();

    for (row_index, row) in rows.iter_mut().enumerate() {
        let owner_count = hunk_rows
            .iter()
            .filter(|bounds| row_index >= bounds.start_row && row_index <= bounds.end_row)
            .count();
        if owner_count != 1 {
            row.source_ranges.clear();
        }
    }
    Some(ExtensionFileViewLayout { rows, hunk_rows })
}

fn invoke_command(invocation: &CommandInvocation) -> Result<CommandExecution, String> {
    if invocation.command_id != "toggle-rendered-markdown" {
        return Err(format!("Unknown command: {}", invocation.command_id));
    }
    Ok(CommandExecution {
        actions: vec![ExtensionHostAction::ToggleFileView { id: VIEW_ID.into() }],
    })
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
                if request.view_id != VIEW_ID {
                    Err(io::Error::other(format!(
                        "Unknown file view: {}",
                        request.view_id
                    )))
                } else {
                    serde_json::to_value(matches_markdown_file(&request.file))
                        .map_err(io::Error::other)
                }
            }
            "workdeck/file-view/layout" => {
                let request: FileViewLayoutRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                if request.view_id != VIEW_ID {
                    Err(io::Error::other(format!(
                        "Unknown file view: {}",
                        request.view_id
                    )))
                } else {
                    serde_json::to_value(create_rendered_markdown_layout(&request))
                        .map_err(io::Error::other)
                }
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
