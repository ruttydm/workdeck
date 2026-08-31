use super::{
    Badge, CodeLanguageBadge, EmptyState, HighlightedCode, IconButton, IconGlyph, ProgressBar,
    SafeMarkdown, StatusDot, WorkdeckIcon,
};
use crate::WORKDECK_CSS;
use dioxus::prelude::*;
use workdeck_api::SyntaxSpan;

/// Development-only visual inventory rendered by the fixture-backed web
/// target. It intentionally contains no application runtime or native I/O.
#[component]
pub fn ComponentGallery() -> Element {
    let mut selected = use_signal(|| false);
    rsx! {
        document::Style { "{WORKDECK_CSS}" }
        main { class: "component-gallery",
            header { h1 { "Workdeck component gallery" } p { "Source-owned Dioxus components · deterministic fixture state" } }
            GallerySection { title: "Buttons and status",
                div { class: "gallery-row",
                    button { class: "button button--primary", r#type: "button", "Primary action" }
                    button { class: "button", r#type: "button", "Secondary" }
                    button { class: "button", r#type: "button", disabled: true, "Disabled" }
                    IconButton { label: "Toggle inspector", glyph: IconGlyph::Inspector, selected: selected(), onclick: move |_| selected.toggle() }
                    StatusDot { tone: "ready" }
                    StatusDot { tone: "attention" }
                    StatusDot { tone: "danger" }
                }
            }
            GallerySection { title: "Badges and progress",
                div { class: "gallery-row", Badge { "Neutral" } Badge { tone: "success", "Passed" } Badge { tone: "warning", "Waiting" } Badge { tone: "danger", "Failed" } Badge { tone: "incoming", "Incoming" } }
                ProgressBar { value: 7, total: 12, label: "Gallery progress" }
            }
            GallerySection { title: "Fields and rows",
                label { class: "search-field", WorkdeckIcon { glyph: IconGlyph::Search, size: 14 } input { r#type: "search", value: "agent commit", aria_label: "Gallery search" } }
                button { class: "search-result", r#type: "button", span { class: "search-result__icon", WorkdeckIcon { glyph: IconGlyph::File, size: 16 } } span { class: "search-result__body", strong { "OpportunityFeed::rank" } small { "app/Services/OpportunityFeed.php" } } span { class: "search-result__meta", "Symbol" } WorkdeckIcon { glyph: IconGlyph::ChevronRight, size: 14 } }
            }
            GallerySection { title: "Code and syntax",
                div { class: "gallery-code-sample", role: "table", aria_label: "Rust syntax example",
                    div { class: "gallery-code-sample__header", span { "Semantic tree-sitter spans" } CodeLanguageBadge { language: "rust".to_owned() } }
                    GalleryCodeLine { number: 18, text: "pub async fn load_review(client: &WorkdeckClient) -> Result<Review, WorkdeckError> {".to_owned() }
                    GalleryCodeLine { number: 19, text: "    // Ignore obsolete revisions before they reach the viewport.".to_owned() }
                    GalleryCodeLine { number: 20, text: "    let review = client.request(WorkdeckRequest::Review).await?;".to_owned() }
                    GalleryCodeLine { number: 21, text: "    Ok(review)".to_owned() }
                    GalleryCodeLine { number: 22, text: "}".to_owned() }
                }
                SafeMarkdown { source: "Inline `OperationId` values retain their source revision.\n\n```rust\nclient.cancel(operation_id);\n```".to_owned() }
            }
            GallerySection { title: "Empty and error states",
                div { class: "gallery-grid", EmptyState { glyph: IconGlyph::Check, title: "You're caught up", message: "New commits and pull-request activity will appear here." } div { class: "inline-error surface-error", role: "alert", "Repository is currently unavailable; retained Git history is safe." } }
            }
        }
    }
}

#[component]
fn GalleryCodeLine(number: usize, text: String) -> Element {
    let spans = gallery_rust_spans(&text);
    rsx! {
        div { class: "source-line", role: "row",
            span { class: "line-number line-number--source", role: "cell", "{number}" }
            div { class: "diff-code-cell", role: "cell", HighlightedCode { text, spans } }
        }
    }
}

fn gallery_rust_spans(text: &str) -> Vec<SyntaxSpan> {
    const TOKENS: &[(&str, &str)] = &[
        (
            "// Ignore obsolete revisions before they reach the viewport.",
            "comment",
        ),
        ("pub", "keyword"),
        ("async", "keyword"),
        ("fn", "keyword"),
        ("let", "keyword"),
        ("await", "keyword"),
        ("load_review", "function"),
        ("request", "function"),
        ("Ok", "function"),
        ("WorkdeckClient", "type"),
        ("Result", "type"),
        ("Review", "type"),
        ("WorkdeckError", "type"),
        ("WorkdeckRequest", "type"),
        ("Review", "constant"),
        ("client", "parameter"),
        ("review", "variable"),
    ];
    let mut candidates = TOKENS
        .iter()
        .flat_map(|(needle, token)| {
            text.match_indices(needle)
                .map(move |(start, value)| SyntaxSpan {
                    start,
                    end: start + value.len(),
                    token: (*token).to_owned(),
                })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| right.end.cmp(&left.end))
    });
    let mut end = 0;
    candidates
        .into_iter()
        .filter(|span| {
            if span.start < end {
                false
            } else {
                end = span.end;
                true
            }
        })
        .collect()
}

#[component]
fn GallerySection(title: String, children: Element) -> Element {
    rsx! { section { class: "gallery-section", h2 { "{title}" } div { class: "gallery-section__content", {children} } } }
}
