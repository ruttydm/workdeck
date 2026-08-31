use crate::{
    ReviewLens,
    components::{
        Badge, CodeLanguageBadge, EmptyState, HighlightedCode, IconGlyph, PaneLayoutContext,
        PaneResizer, SafeMarkdown, WorkdeckIcon,
    },
};
use dioxus::prelude::*;
use std::time::Duration;
use workdeck_api::{
    DiffLine, DiffLineKind, RequestId, ReviewId, ReviewSet, ReviewUnit, ReviewUnitId,
    UnitTransition, WorkdeckClient, WorkdeckRequest, WorkdeckResponse,
};

const REVIEW_CACHE_MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReviewResizePane {
    Tree,
    Structure,
}

#[component]
pub fn ChangesSurface(client: WorkdeckClient, review_id: ReviewId) -> Element {
    let mut lens = use_signal(|| ReviewLens::Diff);
    let mut selected = use_signal(|| Option::<ReviewUnitId>::None);
    let mut generation = use_signal(|| 0_u64);
    let mut show_whitespace = use_signal(|| true);
    let mut show_tree = use_signal(|| true);
    let mut show_structure = use_signal(|| true);
    let mut resizing = use_signal(|| None::<(ReviewResizePane, f64, f64)>);
    let pane_layout = use_context::<PaneLayoutContext>();
    let review_client = client.clone();
    let review_key = review_id.clone();
    let cached_client = client.clone();
    let cached_review_key = review_id.clone();
    let cached_review = use_hook(move || {
        cached_client
            .cached_response(&WorkdeckRequest::LoadReview {
                request_id: RequestId::new(),
                review_id: cached_review_key,
            })
            .and_then(|response| match response.payload {
                WorkdeckResponse::Review(review) => Some(review),
                _ => None,
            })
    });
    let review = use_resource(move || {
        let client = review_client.clone();
        let review_id = review_key.clone();
        let _generation = generation();
        async move {
            client
                .request_cached(
                    WorkdeckRequest::LoadReview {
                        request_id: RequestId::new(),
                        review_id,
                    },
                    REVIEW_CACHE_MAX_AGE,
                )
                .await
        }
    });
    let live_snapshot = review
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|envelope| match &envelope.payload {
            WorkdeckResponse::Review(review) => Some(review.clone()),
            _ => None,
        });
    let snapshot = live_snapshot.or(cached_review);
    rsx! {
        section {
            class: "surface review-surface",
            style: "--review-tree-width: {pane_layout.current().review_tree}px; --review-structure-width: {pane_layout.current().review_structure}px",
            onpointermove: move |event| {
                if let Some((pane, start_x, start_width)) = resizing() {
                    let delta = event.client_coordinates().x - start_x;
                    match pane {
                        ReviewResizePane::Tree => pane_layout.update(|widths| widths.review_tree = (start_width + delta).clamp(220.0, 480.0)),
                        ReviewResizePane::Structure => pane_layout.update(|widths| widths.review_structure = (start_width - delta).clamp(220.0, 480.0)),
                    }
                }
            },
            onpointerup: move |_| {
                if resizing().is_some() {
                    resizing.set(None);
                    pane_layout.persist();
                }
            },
            if let Some(review) = snapshot {
                ChangesHeader { review: review.clone() }
                div { class: "review-toolbar",
                    div { class: "segmented-control", role: "tablist", aria_label: "Change view",
                        for candidate in [ReviewLens::Diff, ReviewLens::Split, ReviewLens::Source, ReviewLens::Markdown, ReviewLens::Calls, ReviewLens::Structure, ReviewLens::Ast] {
                            button { class: if lens() == candidate { "segment is-selected" } else { "segment" }, role: "tab", aria_selected: lens() == candidate, r#type: "button", onclick: move |_| lens.set(candidate), {candidate.label()} }
                        }
                    }
                    span { class: "review-toolbar__spacer" }
                    button {
                        class: if show_tree() { "icon-button is-selected review-pane-toggle review-pane-toggle--tree" } else { "icon-button review-pane-toggle review-pane-toggle--tree" },
                        r#type: "button",
                        aria_label: if show_tree() { "Hide changed files" } else { "Show changed files" },
                        aria_pressed: show_tree(),
                        title: if show_tree() { "Hide changed files" } else { "Show changed files" },
                        onclick: move |_| show_tree.toggle(),
                        WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 }
                    }
                    button {
                        class: if show_structure() { "icon-button is-selected review-pane-toggle review-pane-toggle--structure" } else { "icon-button review-pane-toggle review-pane-toggle--structure" },
                        r#type: "button",
                        aria_label: if show_structure() { "Hide canonical structure" } else { "Show canonical structure" },
                        aria_pressed: show_structure(),
                        title: if show_structure() { "Hide canonical structure" } else { "Show canonical structure" },
                        onclick: move |_| show_structure.toggle(),
                        WorkdeckIcon { glyph: IconGlyph::Inspector, size: 14 }
                    }
                    button { class: if show_whitespace() { "button is-selected" } else { "button" }, r#type: "button", aria_pressed: show_whitespace(), onclick: move |_| show_whitespace.toggle(), if show_whitespace() { "Whitespace shown" } else { "Whitespace hidden" } }
                }
                div { class: match (show_tree(), show_structure()) {
                        (true, true) => "review-workbench",
                        (false, true) => "review-workbench tree-collapsed",
                        (true, false) => "review-workbench structure-collapsed",
                        (false, false) => "review-workbench tree-collapsed structure-collapsed",
                    },
                    if show_tree() {
                    ChangesTree { review: review.clone(), selected: selected(), onselect: move |unit| selected.set(Some(unit)) }
                    PaneResizer {
                        label: "Resize changed files".to_owned(),
                        class_name: "review-tree-resizer".to_owned(),
                        value: pane_layout.current().review_tree,
                        min: 220.0,
                        max: 480.0,
                        default_value: 272.0,
                        onstart: move |event: PointerEvent| resizing.set(Some((ReviewResizePane::Tree, event.client_coordinates().x, pane_layout.current().review_tree))),
                        onchange: move |next| {
                            pane_layout.update(|widths| widths.review_tree = next);
                            pane_layout.persist();
                        },
                    }
                    }
                    ChangesContent { review: review.clone(), selected: selected(), lens: lens(), show_whitespace: show_whitespace() }
                    if show_structure() {
                    PaneResizer {
                        label: "Resize canonical structure".to_owned(),
                        class_name: "review-structure-resizer".to_owned(),
                        value: pane_layout.current().review_structure,
                        min: 220.0,
                        max: 480.0,
                        default_value: 280.0,
                        reverse: true,
                        onstart: move |event: PointerEvent| resizing.set(Some((ReviewResizePane::Structure, event.client_coordinates().x, pane_layout.current().review_structure))),
                        onchange: move |next| {
                            pane_layout.update(|widths| widths.review_structure = next);
                            pane_layout.persist();
                        },
                    }
                    CanonicalTree { review, selected: selected() }
                    }
                }
            } else if let Some(Err(error)) = review.read().as_ref() {
                div { class: "fatal-state", WorkdeckIcon { glyph: IconGlyph::Warning, size: 24 } h2 { "Changes unavailable" } p { "{error}" } button { class: "button button--primary", r#type: "button", onclick: move |_| generation += 1, "Retry" } }
            } else {
                div { class: "surface-loading", div { class: "loading-spinner" } "Preparing changes…" }
            }
        }
    }
}

#[component]
fn CanonicalTree(review: ReviewSet, selected: Option<ReviewUnitId>) -> Element {
    let unit = selected
        .as_ref()
        .and_then(|id| review.units.iter().find(|unit| &unit.id == id))
        .or_else(|| review.units.first())
        .cloned();
    rsx! {
        aside { class: "canonical-pane", aria_label: "Canonical tree",
            header { class: "canonical-pane__header",
                div { strong { "Canonical tree" } small { "Stable semantic structure" } }
                Badge { tone: "neutral", "AST" }
            }
            if let Some(unit) = unit {
                div { class: "canonical-pane__path", WorkdeckIcon { glyph: IconGlyph::File, size: 13 } span { "{unit.path}" } }
                div { class: "canonical-tree", role: "tree", aria_label: "Canonical nodes",
                    for node in unit.ast {
                        div {
                            class: "canonical-node",
                            role: "treeitem",
                            aria_level: node.depth + 1,
                            style: format!("padding-left: {}px", 10 + node.depth * 14),
                            WorkdeckIcon { glyph: IconGlyph::ChevronRight, size: 11 }
                            span { class: "canonical-node__body",
                                strong { "{node.label}" }
                                small { class: "canonical-node__kind", "{node.kind}" }
                            }
                            code { "L{node.line}" }
                        }
                    }
                }
            } else {
                div { class: "canonical-pane__empty", "Select a semantic unit" }
            }
        }
    }
}

#[component]
fn ChangesHeader(review: ReviewSet) -> Element {
    let additions = review
        .units
        .iter()
        .map(|unit| unit.additions)
        .sum::<usize>();
    let deletions = review
        .units
        .iter()
        .map(|unit| unit.deletions)
        .sum::<usize>();
    rsx! {
        header { class: "review-header",
            div { class: "review-header__title",
                p { class: "eyebrow", {format!("{} / {} · {}", review.summary.project, review.summary.repository, review.summary.branch)} }
                h1 { "{review.summary.title}" }
            }
            div { class: "review-header__actions",
                Badge { tone: "neutral", {format!("{} commits", review.commits.len())} }
                span { class: "review-header__diff", span { class: "text-success", "+{additions}" } span { class: "text-danger", "−{deletions}" } }
            }
        }
    }
}

#[component]
fn ChangesTree(
    review: ReviewSet,
    selected: Option<ReviewUnitId>,
    onselect: EventHandler<ReviewUnitId>,
) -> Element {
    rsx! {
        aside { class: "review-tree", aria_label: "Changed files and symbols",
            div { class: "review-tree__header", strong { "Changed files" } span { {review.units.len().to_string()} } }
            div { class: "review-tree__path", WorkdeckIcon { glyph: IconGlyph::ChevronDown, size: 12 } WorkdeckIcon { glyph: IconGlyph::Workspaces, size: 14 } "app" }
            for unit in review.units {
                button { class: if selected.as_ref().is_some_and(|id| id == &unit.id) { "review-unit is-selected" } else { "review-unit" }, r#type: "button", onclick: move |_| onselect.call(unit.id.clone()),
                    span { class: "review-unit__indent" }
                    span { class: "review-unit__state", WorkdeckIcon { glyph: IconGlyph::File, size: 14 } }
                    span { class: "review-unit__body", strong { "{unit.title}" } small { "{unit.path}" } }
                    span { class: "review-unit__diff", span { class: "text-success", "+{unit.additions}" } span { class: "text-danger", "−{unit.deletions}" } }
                    if unit.transition != UnitTransition::Unchanged { Badge { tone: transition_tone(unit.transition), {format!("{:?}", unit.transition)} } }
                }
            }
        }
    }
}

#[component]
fn ChangesContent(
    review: ReviewSet,
    selected: Option<ReviewUnitId>,
    lens: ReviewLens,
    show_whitespace: bool,
) -> Element {
    let unit = selected
        .as_ref()
        .and_then(|id| review.units.iter().find(|unit| &unit.id == id))
        .or_else(|| review.units.first())
        .cloned();
    let Some(unit) = unit else {
        return rsx!(EmptyState {
            glyph: IconGlyph::Check,
            title: "No changes".to_owned(),
            message: "No files or semantic units changed in this comparison.".to_owned()
        });
    };
    rsx! {
        article { class: "review-content",
            header { class: "file-header",
                div { WorkdeckIcon { glyph: IconGlyph::File, size: 16 } span { strong { "{unit.title}" } small { "{unit.path}" } } }
                div { CodeLanguageBadge { language: unit.language.clone() } Badge { tone: transition_tone(unit.transition), {format!("{:?}", unit.transition)} } }
            }
            match lens {
                ReviewLens::Diff => rsx!(UnifiedDiff { lines: visible_diff(unit.diff.clone(), show_whitespace) }),
                ReviewLens::Split => rsx!(SplitDiff { lines: visible_diff(unit.diff.clone(), show_whitespace) }),
                ReviewLens::Source => rsx!(SourceView { unit: unit.clone() }),
                ReviewLens::Markdown => rsx!(MarkdownView { review: review.clone() }),
                ReviewLens::Calls => rsx!(CallsView { unit: unit.clone() }),
                ReviewLens::Structure => rsx!(StructureView { unit: unit.clone() }),
                ReviewLens::Ast => rsx!(AstView { unit: unit.clone() }),
            }
        }
    }
}

#[component]
pub(super) fn UnifiedDiff(lines: Vec<DiffLine>) -> Element {
    if lines.is_empty() {
        return rsx!(EmptyState {
            glyph: IconGlyph::File,
            title: "No textual diff".to_owned(),
            message: "Choose Source or Structure for this semantic unit.".to_owned()
        });
    }
    rsx! { div { class: "diff-view", role: "table", aria_label: "Unified diff", tabindex: "0", for line in lines { div { class: format!("diff-line diff-line--{}", diff_kind(line.kind)), role: "row", span { class: "line-number line-number--old", role: "cell", aria_label: "Old line", {line.old_number.map_or_else(String::new, |number| number.to_string())} } span { class: "line-number line-number--new", role: "cell", aria_label: "New line", {line.new_number.map_or_else(String::new, |number| number.to_string())} } div { class: "diff-code-cell", role: "cell", HighlightedCode { text: line.text, spans: line.spans } } } } } }
}

#[component]
pub(super) fn SplitDiff(lines: Vec<DiffLine>) -> Element {
    let removed = lines
        .iter()
        .filter(|line| line.kind != DiffLineKind::Added)
        .cloned()
        .collect::<Vec<_>>();
    let added = lines
        .iter()
        .filter(|line| line.kind != DiffLineKind::Removed)
        .cloned()
        .collect::<Vec<_>>();
    rsx! { div { class: "split-diff", div { class: "split-diff__pane", role: "table", aria_label: "Before changes", header { "Before" } for line in removed { div { class: format!("diff-line diff-line--split diff-line--{}", diff_kind(line.kind)), role: "row", span { class: "line-number line-number--split", role: "cell", aria_label: "Old line", {line.old_number.map_or_else(String::new, |number| number.to_string())} } div { class: "diff-code-cell", role: "cell", HighlightedCode { text: line.text, spans: line.spans } } } } } div { class: "split-diff__pane", role: "table", aria_label: "After changes", header { "After" } for line in added { div { class: format!("diff-line diff-line--split diff-line--{}", diff_kind(line.kind)), role: "row", span { class: "line-number line-number--split", role: "cell", aria_label: "New line", {line.new_number.map_or_else(String::new, |number| number.to_string())} } div { class: "diff-code-cell", role: "cell", HighlightedCode { text: line.text, spans: line.spans } } } } } } }
}

#[component]
fn SourceView(unit: ReviewUnit) -> Element {
    rsx! { div { class: "source-view", role: "table", aria_label: "Source code", tabindex: "0", for line in unit.source { div { class: "source-line", role: "row", span { class: "line-number line-number--source", role: "cell", aria_label: "Line", "{line.number}" } div { class: "diff-code-cell", role: "cell", HighlightedCode { text: line.text, spans: line.spans } } } } } }
}

#[component]
fn MarkdownView(review: ReviewSet) -> Element {
    if let Some(document) = review.plan {
        rsx! { article { class: "markdown-view", p { class: "eyebrow", "{document.path}" } h1 { "{document.title}" } for section in document.sections { section { class: "markdown-section", h2 { "{section.heading}" } SafeMarkdown { source: section.body } } } } }
    } else {
        rsx!(EmptyState {
            glyph: IconGlyph::File,
            title: "No Markdown attached".to_owned(),
            message: "Attach a plan or Markdown document to inspect its sections.".to_owned()
        })
    }
}

#[component]
fn CallsView(unit: ReviewUnit) -> Element {
    rsx! { div { class: "analysis-view", header { h2 { "Call flow" } p { "Callers and callees for {unit.title}" } } div { class: "call-flow", for node in unit.calls { div { class: format!("call-node call-node--{:?}", node.direction), style: format!("margin-left: {}px", node.depth * 28), WorkdeckIcon { glyph: IconGlyph::ChevronRight, size: 13 } div { strong { "{node.label}" } small { "{node.detail}" } } } } } } }
}

#[component]
fn StructureView(unit: ReviewUnit) -> Element {
    rsx! { div { class: "analysis-view", header { h2 { "Canonical structure" } p { "Semantic nodes, stable across formatting and moves" } } div { class: "structure-cards", for node in unit.ast { div { class: "structure-card", style: format!("margin-left: {}px", node.depth * 20), Badge { "{node.kind}" } strong { "{node.label}" } small { "line {node.line}" } } } } } }
}

#[component]
fn AstView(unit: ReviewUnit) -> Element {
    rsx! { div { class: "analysis-view ast-view", header { h2 { "Abstract syntax tree" } p { "Parser structure for {unit.language}" } } for node in unit.ast { div { class: "ast-row", style: format!("padding-left: {}px", 16 + node.depth * 22), WorkdeckIcon { glyph: IconGlyph::ChevronRight, size: 12 } code { "{node.kind}" } strong { "{node.label}" } span { "L{node.line}" } } } } }
}

fn diff_kind(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Context => "context",
        DiffLineKind::Added => "added",
        DiffLineKind::Removed => "removed",
        DiffLineKind::Header => "header",
    }
}

pub(super) fn visible_diff(lines: Vec<DiffLine>, show_whitespace: bool) -> Vec<DiffLine> {
    if show_whitespace {
        lines
    } else {
        lines
            .into_iter()
            .filter(|line| !line.text.trim().is_empty())
            .collect()
    }
}
fn transition_tone(transition: UnitTransition) -> String {
    match transition {
        UnitTransition::Changed | UnitTransition::New => "incoming",
        UnitTransition::FormattingOnly | UnitTransition::Moved => "warning",
        UnitTransition::Removed => "danger",
        UnitTransition::Unchanged => "neutral",
    }
    .into()
}
