use dioxus::prelude::*;
use workdeck_api::SyntaxSpan;

/// Renderer-owned presentation for parser-produced semantic spans.
///
/// The text remains the source of truth: invalid, overlapping, or non-UTF-8
/// boundary spans are ignored rather than changing the displayed source.
#[component]
pub fn HighlightedCode(text: String, spans: Vec<SyntaxSpan>) -> Element {
    let parts = highlighted_parts(&text, &spans);
    rsx! {
        code { class: "syntax-code",
            for part in parts {
                if let Some(token) = part.token {
                    span { class: "syntax syntax--{token}", "{part.text}" }
                } else {
                    "{part.text}"
                }
            }
        }
    }
}

#[component]
pub fn CodeLanguageBadge(language: String) -> Element {
    rsx! { span { class: "code-language-badge", "{display_language(&language)}" } }
}

pub(crate) fn language_from_path(path: &str) -> &'static str {
    let filename = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if matches!(filename.as_str(), "dockerfile" | "containerfile") {
        return "Shell";
    }
    if matches!(filename.as_str(), "gemfile" | "rakefile") {
        return "Ruby";
    }
    let extension = filename.rsplit_once('.').map(|(_, extension)| extension);
    match extension {
        Some("rs") => "Rust",
        Some("swift") => "Swift",
        Some("js" | "mjs" | "cjs") => "JavaScript",
        Some("ts" | "mts" | "cts") => "TypeScript",
        Some("tsx") => "TSX",
        Some("jsx") => "JSX",
        Some("py" | "pyi") => "Python",
        Some("go") => "Go",
        Some("php") => "PHP",
        Some("sh" | "bash" | "zsh") => "Shell",
        Some("c" | "h") => "C",
        Some("cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx") => "C++",
        Some("cs") => "C#",
        Some("css") => "CSS",
        Some("html" | "htm") => "HTML",
        Some("vue") => "Vue",
        Some("java") => "Java",
        Some("json" | "jsonc") => "JSON",
        Some("rb") => "Ruby",
        Some("toml") => "TOML",
        Some("yaml" | "yml") => "YAML",
        Some("md" | "mdx") => "Markdown",
        _ => "Text",
    }
}

fn display_language(language: &str) -> &str {
    match language.to_ascii_lowercase().as_str() {
        "rust" | "rs" => "Rust",
        "swift" => "Swift",
        "javascript" | "js" | "jsx" => "JavaScript",
        "typescript" | "ts" => "TypeScript",
        "tsx" => "TSX",
        "python" | "py" => "Python",
        "go" => "Go",
        "php" => "PHP",
        "bash" | "shell" | "sh" | "zsh" => "Shell",
        "c" => "C",
        "cpp" | "c++" => "C++",
        "csharp" | "c#" => "C#",
        "css" => "CSS",
        "html" => "HTML",
        "java" => "Java",
        "json" => "JSON",
        "ruby" | "rb" => "Ruby",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "markdown" | "md" => "Markdown",
        _ => language,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct HighlightPart {
    text: String,
    token: Option<&'static str>,
}

fn highlighted_parts(text: &str, spans: &[SyntaxSpan]) -> Vec<HighlightPart> {
    let mut spans = spans.to_vec();
    spans.sort_by_key(|span| (span.start, span.end));
    let mut parts = Vec::new();
    let mut cursor = 0;
    for span in spans {
        if span.start < cursor
            || span.start >= span.end
            || span.end > text.len()
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            continue;
        }
        if cursor < span.start {
            parts.push(HighlightPart {
                text: text[cursor..span.start].to_owned(),
                token: None,
            });
        }
        parts.push(HighlightPart {
            text: text[span.start..span.end].to_owned(),
            token: Some(known_token(&span.token)),
        });
        cursor = span.end;
    }
    if cursor < text.len() || parts.is_empty() {
        parts.push(HighlightPart {
            text: text[cursor..].to_owned(),
            token: None,
        });
    }
    parts
}

fn known_token(token: &str) -> &'static str {
    match token {
        "keyword" => "keyword",
        "type" => "type",
        "string" => "string",
        "number" => "number",
        "comment" => "comment",
        "attribute" => "attribute",
        "function" => "function",
        "variable" => "variable",
        "parameter" => "parameter",
        "property" => "property",
        "constant" => "constant",
        "builtin" => "builtin",
        "macro" => "macro",
        "tag" => "tag",
        "namespace" => "namespace",
        "label" => "label",
        "escape" => "escape",
        "embedded" => "embedded",
        "operator" => "operator",
        "punctuation" => "punctuation",
        "heading" => "heading",
        _ => "identifier",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighted_parts_preserve_source_and_ignore_overlaps() {
        let text = "pub fn run";
        let parts = highlighted_parts(
            text,
            &[
                SyntaxSpan {
                    start: 0,
                    end: 3,
                    token: "keyword".into(),
                },
                SyntaxSpan {
                    start: 0,
                    end: 6,
                    token: "overlap".into(),
                },
                SyntaxSpan {
                    start: 4,
                    end: 6,
                    token: "keyword".into(),
                },
            ],
        );
        assert_eq!(
            parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>(),
            text
        );
        assert_eq!(parts.iter().filter(|part| part.token.is_some()).count(), 2);
    }

    #[test]
    fn unknown_tokens_cannot_create_arbitrary_css_classes() {
        assert_eq!(known_token("function.call"), "identifier");
        assert_eq!(known_token("punctuation"), "punctuation");
    }

    #[test]
    fn path_language_labels_cover_polyglot_sources() {
        assert_eq!(language_from_path("src/main.rs"), "Rust");
        assert_eq!(language_from_path("web/App.tsx"), "TSX");
        assert_eq!(language_from_path(".github/workflows/ci.yml"), "YAML");
        assert_eq!(language_from_path("Dockerfile"), "Shell");
        assert_eq!(language_from_path("Gemfile"), "Ruby");
    }
}
