use super::{CodeLanguageBadge, HighlightedCode};
use dioxus::prelude::*;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownInline {
    Text(String),
    Code(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownBlock {
    Heading {
        level: u8,
        content: Vec<MarkdownInline>,
    },
    Paragraph(Vec<MarkdownInline>),
    Quote(Vec<MarkdownInline>),
    Code {
        language: String,
        text: String,
    },
    List {
        ordered: bool,
        items: Vec<Vec<MarkdownInline>>,
    },
    Rule,
}

#[component]
pub fn SafeMarkdown(source: String) -> Element {
    let blocks = parse_markdown(&source);
    rsx! {
        div { class: "safe-markdown",
            for block in blocks {
                match block {
                    MarkdownBlock::Heading { level: 1, content } => rsx!(h2 { InlineMarkdown { content } }),
                    MarkdownBlock::Heading { level: 2, content } => rsx!(h3 { InlineMarkdown { content } }),
                    MarkdownBlock::Heading { content, .. } => rsx!(h4 { InlineMarkdown { content } }),
                    MarkdownBlock::Paragraph(content) => rsx!(p { InlineMarkdown { content } }),
                    MarkdownBlock::Quote(content) => rsx!(blockquote { InlineMarkdown { content } }),
                    MarkdownBlock::Code { language, text } => rsx!(figure { class: "markdown-code-block",
                        if !language.is_empty() { figcaption { CodeLanguageBadge { language: language.clone() } } }
                        pre { class: "markdown-code", "data-language": "{language}", HighlightedCode { text, spans: Vec::new() } }
                    }),
                    MarkdownBlock::List { ordered: true, items } => rsx!(ol { for item in items { li { InlineMarkdown { content: item } } } }),
                    MarkdownBlock::List { items, .. } => rsx!(ul { for item in items { li { InlineMarkdown { content: item } } } }),
                    MarkdownBlock::Rule => rsx!(hr {}),
                }
            }
        }
    }
}

#[component]
fn InlineMarkdown(content: Vec<MarkdownInline>) -> Element {
    rsx! {
        for fragment in content {
            match fragment {
                MarkdownInline::Text(text) => rsx! {
                    for (index, line) in text.lines().enumerate() {
                        if index > 0 { br {} }
                        "{line}"
                    }
                },
                MarkdownInline::Code(code) => rsx!(code { class: "markdown-inline-code", "{code}" }),
            }
        }
    }
}

fn parse_markdown(source: &str) -> Vec<MarkdownBlock> {
    let parser = Parser::new_ext(
        source,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    );
    let mut blocks = Vec::new();
    let mut content = Vec::new();
    let mut code_text = String::new();
    let mut heading = None;
    let mut quote = false;
    let mut code_language = None;
    let mut list = None::<(bool, Vec<Vec<MarkdownInline>>)>;
    let mut in_item = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                content.clear();
                heading = Some(heading_number(level));
            }
            Event::End(TagEnd::Heading(_)) => {
                blocks.push(MarkdownBlock::Heading {
                    level: heading.take().unwrap_or(2),
                    content: take_inline_trimmed(&mut content),
                });
            }
            Event::Start(Tag::Paragraph) => content.clear(),
            Event::End(TagEnd::Paragraph) if quote => {}
            Event::End(TagEnd::Paragraph) if in_item => {}
            Event::End(TagEnd::Paragraph) => {
                let value = take_inline_trimmed(&mut content);
                if !value.is_empty() {
                    blocks.push(MarkdownBlock::Paragraph(value));
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                content.clear();
                quote = true;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                quote = false;
                blocks.push(MarkdownBlock::Quote(take_inline_trimmed(&mut content)));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                code_text.clear();
                code_language = Some(match kind {
                    CodeBlockKind::Fenced(language) => language.into_string(),
                    CodeBlockKind::Indented => String::new(),
                });
            }
            Event::End(TagEnd::CodeBlock) => blocks.push(MarkdownBlock::Code {
                language: code_language.take().unwrap_or_default(),
                text: std::mem::take(&mut code_text),
            }),
            Event::Start(Tag::List(start)) => list = Some((start.is_some(), Vec::new())),
            Event::End(TagEnd::List(_)) => {
                if let Some((ordered, items)) = list.take() {
                    blocks.push(MarkdownBlock::List { ordered, items });
                }
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                content.clear();
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                if let Some((_, items)) = list.as_mut() {
                    items.push(take_inline_trimmed(&mut content));
                }
            }
            Event::Text(value) if code_language.is_some() => code_text.push_str(&value),
            Event::Text(value) => {
                push_inline(&mut content, MarkdownInline::Text(value.into_string()))
            }
            Event::Code(value) => {
                push_inline(&mut content, MarkdownInline::Code(value.into_string()))
            }
            Event::SoftBreak | Event::HardBreak => {
                push_inline(&mut content, MarkdownInline::Text("\n".into()));
            }
            Event::Rule => blocks.push(MarkdownBlock::Rule),
            // Raw HTML, inline HTML, footnote payloads, and metadata are never
            // injected into the DOM. Link text is retained through Text events.
            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => {}
        }
    }
    let trailing = take_inline_trimmed(&mut content);
    if !trailing.is_empty() {
        blocks.push(MarkdownBlock::Paragraph(trailing));
    }
    blocks
}

fn heading_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn push_inline(content: &mut Vec<MarkdownInline>, next: MarkdownInline) {
    match next {
        MarkdownInline::Text(next_text) => {
            if let Some(MarkdownInline::Text(previous)) = content.last_mut() {
                previous.push_str(&next_text);
            } else {
                content.push(MarkdownInline::Text(next_text));
            }
        }
        code @ MarkdownInline::Code(_) => content.push(code),
    }
}

fn take_inline_trimmed(content: &mut Vec<MarkdownInline>) -> Vec<MarkdownInline> {
    let mut content = std::mem::take(content);
    if let Some(MarkdownInline::Text(first)) = content.first_mut() {
        *first = first.trim_start().to_owned();
    }
    if let Some(MarkdownInline::Text(last)) = content.last_mut() {
        *last = last.trim_end().to_owned();
    }
    content.retain(|fragment| !matches!(fragment, MarkdownInline::Text(text) if text.is_empty()));
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_html_is_never_a_renderable_block() {
        let blocks = parse_markdown("# Safe\n<script>alert(1)</script>\n\n- one\n- two\n");
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block, MarkdownBlock::Heading { .. }))
        );
        assert!(
            blocks.iter().any(
                |block| matches!(block, MarkdownBlock::List { items, .. } if items.len() == 2)
            )
        );
        assert!(!format!("{blocks:?}").contains("script"));
    }

    #[test]
    fn inline_and_fenced_code_keep_their_semantic_shape() {
        let blocks = parse_markdown(
            "Use `cargo check` before merge.\n\n```rust\npub fn ready() -> bool { true }\n```\n",
        );
        assert!(blocks.iter().any(|block| {
            matches!(
                block,
                MarkdownBlock::Paragraph(content)
                    if content.iter().any(|part| matches!(part, MarkdownInline::Code(code) if code == "cargo check"))
            )
        }));
        assert!(blocks.iter().any(|block| {
            matches!(
                block,
                MarkdownBlock::Code { language, text }
                    if language == "rust" && text.contains("pub fn ready")
            )
        }));
    }
}
