use crate::{
    components::{IconGlyph, PaneLayoutContext, PaneResizer, WorkdeckIcon},
    surfaces::{TaskChangesSurface, TaskDiffContext},
};
use dioxus::prelude::*;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use workdeck_api::{
    GitGraph, GitGraphRow, GitReference, GitReferenceKind, RequestId, ReviewSet, WorkdeckClient,
    WorkdeckRequest, WorkdeckResponse,
};

const ROW_HEIGHT: f64 = 48.0;
const DEFAULT_VIEWPORT_HEIGHT: f64 = 720.0;
const OVERSCAN_VIEWPORTS: usize = 4;
const LANE_GAP: usize = 14;
const GRAPH_PADDING: usize = 14;
const GRAPH_CACHE_MAX_AGE: Duration = Duration::from_secs(120);
const REVIEW_CACHE_MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitRange {
    base: String,
    head: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GraphMetrics {
    width: usize,
    lane_gap: usize,
}

#[component]
pub fn GitSurface(
    client: WorkdeckClient,
    worktree_id: Option<workdeck_api::WorktreeId>,
) -> Element {
    let mut selected = use_signal(|| Option::<String>::None);
    let mut query = use_signal(String::new);
    let mut generation = use_signal(|| 0_u64);
    let mut range_anchor = use_signal(|| None::<String>);
    let mut extra_rows = use_signal(Vec::<GitGraphRow>::new);
    let mut next_cursor = use_signal(|| None::<String>);
    let mut more_available = use_signal(|| None::<bool>);
    let mut loading_more = use_signal(|| false);
    let preparing_changes = use_signal(|| false);
    let mut action_error = use_signal(|| None::<String>);
    let mut list_visible = use_signal(|| true);
    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut viewport_height = use_signal(|| DEFAULT_VIEWPORT_HEIGHT);
    let mut opened_changes = use_signal(|| None::<Arc<ReviewSet>>);
    let changes_generation = use_signal(|| 0_u64);
    let mut resizing = use_signal(|| None::<(f64, f64)>);
    let pane_layout = use_context::<PaneLayoutContext>();
    let git_client = client.clone();
    let more_client = client.clone();
    let refresh_client = client.clone();
    let resource_worktree = worktree_id.clone();
    let more_worktree = worktree_id.clone();
    let refresh_worktree = worktree_id.clone();
    let cached_client = client.clone();
    let cached_worktree = worktree_id.clone();
    let cached_graph = use_hook(move || {
        cached_client
            .cached_response(&WorkdeckRequest::LoadGitGraph {
                request_id: RequestId::new(),
                worktree_id: cached_worktree,
                cursor: None,
            })
            .and_then(|envelope| match envelope.payload {
                WorkdeckResponse::GitGraph(graph) => Some(graph),
                _ => None,
            })
    });
    let graph = use_resource(move || {
        let client = git_client.clone();
        let worktree_id = resource_worktree.clone();
        let _generation = generation();
        async move {
            client
                .request_cached(
                    WorkdeckRequest::LoadGitGraph {
                        request_id: RequestId::new(),
                        worktree_id,
                        cursor: None,
                    },
                    GRAPH_CACHE_MAX_AGE,
                )
                .await
        }
    });
    let live_snapshot = graph
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|envelope| match &envelope.payload {
            WorkdeckResponse::GitGraph(graph) => Some(graph.clone()),
            _ => None,
        });
    let snapshot = live_snapshot.or(cached_graph);
    let refresh_error = graph
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().err())
        .map(ToString::to_string);
    let range_worktree = worktree_id.clone().or_else(|| {
        snapshot
            .as_ref()
            .and_then(|graph| graph.worktree_id.clone())
    });
    let needle = query().trim().to_ascii_lowercase();
    let list_width = pane_layout.current().master_list;
    if selected().is_none()
        && !preparing_changes()
        && opened_changes().is_none()
        && action_error().is_none()
        && let Some(row) = snapshot
            .as_ref()
            .and_then(|graph| graph.rows.first())
            .cloned()
    {
        selected.set(Some(row.oid.clone()));
        prepare_commit_changes(
            client.clone(),
            range_worktree.clone(),
            row,
            opened_changes,
            preparing_changes,
            action_error,
            changes_generation,
        );
    }

    rsx! {
        section {
            class: "surface git-surface activity-review-surface",
            style: "--master-list-width: {list_width}px",
            onpointermove: move |event| {
                if let Some((start_x, start_width)) = resizing() {
                    let next = (start_width + event.client_coordinates().x - start_x).clamp(300.0, 640.0);
                    pane_layout.update(|widths| widths.master_list = next);
                }
            },
            onpointerup: move |_| {
                if resizing().is_some() {
                    resizing.set(None);
                    pane_layout.persist();
                }
            },
            if let Some(graph) = snapshot {
                {
                    let all_rows = combined_rows(&graph, &extra_rows());
                    let filtered_rows = filter_rows(&all_rows, &needle);
                    let graph_metrics = graph_metrics(&all_rows);
                    let (window_start, window_end) = virtual_window(
                        scroll_top(),
                        viewport_height(),
                        filtered_rows.len(),
                    );
                    let range = normalized_range(&all_rows, range_anchor().as_deref(), selected().as_deref());
                    let selected_row = selected()
                        .as_deref()
                        .and_then(|oid| all_rows.iter().find(|row| row.oid == oid))
                        .cloned();
                    rsx! {
                        div { class: if list_visible() { "activity-review-grid git-activity-grid" } else { "activity-review-grid git-activity-grid list-collapsed" },
                            if list_visible() {
                                aside { class: "activity-master git-commit-master", aria_label: "Commit list",
                                    header { class: "activity-master__toolbar",
                                        button { class: "icon-button is-selected", r#type: "button", aria_label: "Hide commit list", title: "Hide commit list", onclick: move |_| list_visible.set(false), WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 } }
                                        label { class: "search-field search-field--compact",
                                            WorkdeckIcon { glyph: IconGlyph::Search, size: 14 }
                                            input { r#type: "search", aria_label: "Filter commit history", placeholder: "Search commits…", value: "{query}", oninput: move |event| { query.set(event.value()); scroll_top.set(0.0); } }
                                        }
                                        details { class: "git-reference-menu",
                                            summary { class: "icon-button", aria_label: "Browse Git references", title: "Branches, tags, remotes, and stashes", WorkdeckIcon { glyph: IconGlyph::Git, size: 14 } }
                                            div { class: "git-reference-popover",
                                                GitReferences { graph: graph.clone(), onselect: {
                                                    let reference_rows = all_rows.clone();
                                                    let reference_client = client.clone();
                                                    let reference_worktree = range_worktree.clone();
                                                    move |oid: String| {
                                                        query.set(String::new());
                                                        selected.set(Some(oid.clone()));
                                                        if let Some(row) = reference_rows.iter().find(|row| row.oid == oid).cloned() {
                                                            prepare_commit_changes(reference_client.clone(), reference_worktree.clone(), row, opened_changes, preparing_changes, action_error, changes_generation);
                                                        } else {
                                                            opened_changes.set(None);
                                                            action_error.set(Some("That reference is outside the loaded history. Load older commits to open it.".into()));
                                                        }
                                                    }
                                                } }
                                            }
                                        }
                                        button { class: "icon-button", r#type: "button", aria_label: "Refresh Git graph", title: "Refresh Git graph", onclick: move |_| {
                                            refresh_client.invalidate_cached(&WorkdeckRequest::LoadGitGraph {
                                                request_id: RequestId::new(),
                                                worktree_id: refresh_worktree.clone(),
                                                cursor: None,
                                            });
                                            extra_rows.set(Vec::new());
                                            next_cursor.set(None);
                                            more_available.set(None);
                                            action_error.set(None);
                                            generation += 1;
                                        }, WorkdeckIcon { glyph: IconGlyph::Refresh, size: 14 } }
                                    }
                                    div { class: "activity-master__scope",
                                        span { "{visible_row_count(&graph, &extra_rows(), &needle)} commits" }
                                        if let Some(message) = refresh_error.as_ref() {
                                            span { class: "text-danger", title: "{message}", "Refresh failed" }
                                        } else {
                                            span { "Read-only" }
                                        }
                                    }
                                    {render_range_controls(
                                        Some(&graph),
                                        &extra_rows(),
                                        selected(),
                                        range_anchor(),
                                        preparing_changes(),
                                        Callback::new({
                                            let compare_client = client.clone();
                                            let compare_worktree = range_worktree.clone();
                                            move |intent| match intent {
                                                RangeIntent::Clear => range_anchor.set(None),
                                                RangeIntent::Compare(range) => prepare_commit_range(
                                                    compare_client.clone(), compare_worktree.clone(), range,
                                                    opened_changes, preparing_changes, action_error, changes_generation,
                                                ),
                                            }
                                        }),
                                    )}
                                    div { class: "git-history git-history--compact", style: "--git-graph-width: {graph_metrics.width}px", tabindex: "0", role: "region", aria_label: "Git commit history",
                                        onscroll: move |event| { scroll_top.set(event.data().scroll_top().max(0.0)); viewport_height.set((event.data().client_height() as f64).max(ROW_HEIGHT)); },
                                        onkeydown: {
                                            let rows = filtered_rows.clone();
                                            let key_client = client.clone();
                                            let key_worktree = range_worktree.clone();
                                            move |event: KeyboardEvent| {
                                                let Some(next) = keyboard_selection(&rows, selected().as_deref(), event.key()) else { return; };
                                                event.prevent_default();
                                                if event.modifiers().contains(Modifiers::SHIFT) && range_anchor().is_none() && let Some(current) = selected().filter(|oid| !is_wip_oid(oid)) { range_anchor.set(Some(current)); }
                                                selected.set(Some(next.clone()));
                                                if let Some(row) = rows.iter().find(|row| row.oid == next).cloned() {
                                                    prepare_commit_changes(key_client.clone(), key_worktree.clone(), row, opened_changes, preparing_changes, action_error, changes_generation);
                                                }
                                            }
                                        },
                                        if filtered_rows.is_empty() {
                                            div { class: "git-filter-empty", role: "status", strong { "No matching commits" } span { "Try a subject, author, hash, or reference." } }
                                        } else {
                                            div { class: "git-rows", role: "listbox", aria_label: "Commits",
                                                div { class: "git-virtual-spacer", aria_hidden: "true", style: "height: {window_start as f64 * ROW_HEIGHT}px" }
                                                for (offset, row) in filtered_rows[window_start..window_end].iter().enumerate() {
                                                    {
                                                        let position = window_start + offset;
                                                        let in_range = range.as_ref().is_some_and(|range| row_in_range(&all_rows, row.oid.as_str(), range));
                                                        let selected_row_client = client.clone();
                                                        let selected_row_worktree = range_worktree.clone();
                                                        let selected_row_value = row.clone();
                                                        rsx! { GitRow {
                                                            key: "{row.oid}", row: row.clone(), selected: selected().as_ref() == Some(&row.oid), range_anchor: range_anchor().as_ref() == Some(&row.oid), in_range,
                                                            position, set_size: filtered_rows.len(), graph_width: graph_metrics.width, lane_gap: graph_metrics.lane_gap,
                                                            onselect: move |(oid, extend): (String, bool)| {
                                                                if extend && range_anchor().is_none() && let Some(current) = selected().filter(|value| !is_wip_oid(value)) { range_anchor.set(Some(current)); }
                                                                selected.set(Some(oid));
                                                                prepare_commit_changes(selected_row_client.clone(), selected_row_worktree.clone(), selected_row_value.clone(), opened_changes, preparing_changes, action_error, changes_generation);
                                                            }
                                                        } }
                                                    }
                                                }
                                                div { class: "git-virtual-spacer", aria_hidden: "true", style: "height: {(filtered_rows.len() - window_end) as f64 * ROW_HEIGHT}px" }
                                            }
                                            if more_available().unwrap_or(graph.has_more) {
                                                button { class: "load-more", r#type: "button", disabled: loading_more(), onclick: move |_| {
                                                    let client = more_client.clone(); let worktree_id = more_worktree.clone(); let cursor = next_cursor().or_else(|| graph.next_cursor.clone());
                                                    loading_more.set(true); action_error.set(None);
                                                    spawn(async move { match client.request_cached(WorkdeckRequest::LoadGitGraph { request_id: RequestId::new(), worktree_id, cursor }, GRAPH_CACHE_MAX_AGE).await {
                                                        Ok(response) => if let WorkdeckResponse::GitGraph(page) = response.payload { extra_rows.set(append_unique_rows(extra_rows(), page.rows)); next_cursor.set(page.next_cursor); more_available.set(Some(page.has_more)); },
                                                        Err(error) => action_error.set(Some(error.to_string())),
                                                    } loading_more.set(false); });
                                                }, if loading_more() { "Loading older history…" } else { "Load older commits" } }
                                            }
                                        }
                                    }
                                }
                                PaneResizer { label: "Resize commit list".to_owned(), class_name: "activity-list-resizer".to_owned(), value: list_width, min: 300.0, max: 640.0, default_value: 420.0,
                                    onstart: move |event: PointerEvent| resizing.set(Some((event.client_coordinates().x, list_width))),
                                    onchange: move |next| { pane_layout.update(|widths| widths.master_list = next); pane_layout.persist(); },
                                }
                            } else {
                                button { class: "icon-button activity-list-reveal", r#type: "button", aria_label: "Show commit list", title: "Show commit list", onclick: move |_| list_visible.set(true), WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 } }
                            }
                            if let (Some(review), Some(row)) = (opened_changes(), selected_row.clone()) {
                                TaskChangesSurface { key: "commit-changes-{row.oid}", review,
                                    context: TaskDiffContext {
                                        eyebrow: if row.wip { "Working tree".into() } else { format!("{} · {} · {}", row.short_oid, row.author, row.timestamp.format("%b %-d")) },
                                        title: row.subject.clone(),
                                        status: row.references.first().cloned().or_else(|| row.head.then_some("HEAD".into())),
                                        external_url: None,
                                    }
                                }
                            } else if preparing_changes() {
                                div { class: "task-selection-state", role: "status", div { class: "loading-spinner" } strong { "Preparing commit diff…" } if let Some(row) = selected_row.as_ref() { p { "{row.short_oid} · {row.subject}" } } }
                            } else if let Some(message) = action_error() {
                                div { class: "task-selection-state", role: "alert", WorkdeckIcon { glyph: IconGlyph::Warning, size: 20 } strong { "Could not prepare this diff" } p { "{message}" }
                                    if let Some(row) = selected_row { button { class: "button button--primary", r#type: "button", onclick: move |_| prepare_commit_changes(client.clone(), range_worktree.clone(), row.clone(), opened_changes, preparing_changes, action_error, changes_generation), "Try again" } }
                                }
                            } else {
                                div { class: "task-selection-state", WorkdeckIcon { glyph: IconGlyph::Git, size: 22 } strong { "Select a commit" } p { "Its diff opens here immediately; changed files stay on the right." } }
                            }
                        }
                    }
                }
            } else if let Some(Err(error)) = graph.read().as_ref() {
                div { class: "empty-state",
                    div { class: "empty-state__icon", WorkdeckIcon { glyph: IconGlyph::Warning, size: 22 } }
                    h2 { "Git history unavailable" }
                    p { "{error}" }
                    button { class: "button button--primary", r#type: "button", onclick: move |_| generation += 1, "Retry" }
                }
            } else {
                GitLoadingSkeleton {}
            }
        }
    }
}

#[component]
fn GitLoadingSkeleton() -> Element {
    rsx! {
        div {
            class: "git-loading-shell",
            role: "status",
            aria_live: "polite",
            aria_label: "Opening commit history",
            aside { class: "git-loading-shell__master",
                div { class: "git-loading-shell__toolbar",
                    span { class: "skeleton skeleton--icon" }
                    span { class: "skeleton skeleton--field" }
                    span { class: "skeleton skeleton--icon" }
                }
                for index in 0..7 {
                    div { key: "git-loading-row-{index}", class: "git-loading-shell__row",
                        span { class: "skeleton skeleton--node" }
                        span { class: "git-loading-shell__copy",
                            span { class: "skeleton skeleton--line" }
                            span { class: "skeleton skeleton--line skeleton--line-short" }
                        }
                    }
                }
            }
            div { class: "git-loading-shell__content",
                span { class: "skeleton skeleton--eyebrow" }
                span { class: "skeleton skeleton--title" }
                span { class: "skeleton skeleton--rule" }
            }
        }
    }
}

fn prepare_commit_changes(
    client: WorkdeckClient,
    worktree_id: Option<workdeck_api::WorktreeId>,
    row: GitGraphRow,
    mut opened_changes: Signal<Option<Arc<ReviewSet>>>,
    mut preparing: Signal<bool>,
    mut error: Signal<Option<String>>,
    mut generation: Signal<u64>,
) {
    let Some(worktree_id) = worktree_id else {
        opened_changes.set(None);
        error.set(Some(
            "Select an available worktree before opening commit changes.".into(),
        ));
        return;
    };
    let request = if row.wip {
        WorkdeckRequest::PrepareWorktreeReview {
            request_id: RequestId::new(),
            worktree_id,
        }
    } else if let Some(parent) = row.edges.first() {
        WorkdeckRequest::PrepareCommitRangeReview {
            request_id: RequestId::new(),
            worktree_id,
            base: parent.parent_oid.clone(),
            head: row.oid,
        }
    } else {
        opened_changes.set(None);
        error.set(Some(
            "This root commit has no parent comparison available.".into(),
        ));
        return;
    };
    generation += 1;
    let request_generation = generation();
    if let Some(review) = cached_review(&client, &request) {
        opened_changes.set(Some(review));
        preparing.set(false);
        error.set(None);
        return;
    }
    opened_changes.set(None);
    preparing.set(true);
    error.set(None);
    spawn(async move {
        let result = client.request_cached(request, REVIEW_CACHE_MAX_AGE).await;
        if generation() != request_generation {
            return;
        }
        match result {
            Ok(response) => {
                if let WorkdeckResponse::Review(review) = response.payload {
                    opened_changes.set(Some(Arc::new(review)));
                }
            }
            Err(value) => error.set(Some(value.to_string())),
        }
        preparing.set(false);
    });
}

fn prepare_commit_range(
    client: WorkdeckClient,
    worktree_id: Option<workdeck_api::WorktreeId>,
    range: CommitRange,
    mut opened_changes: Signal<Option<Arc<ReviewSet>>>,
    mut preparing: Signal<bool>,
    mut error: Signal<Option<String>>,
    mut generation: Signal<u64>,
) {
    let Some(worktree_id) = worktree_id else {
        error.set(Some(
            "Select an available worktree before comparing commits.".into(),
        ));
        return;
    };
    generation += 1;
    let request_generation = generation();
    let request = WorkdeckRequest::PrepareCommitRangeReview {
        request_id: RequestId::new(),
        worktree_id,
        base: range.base,
        head: range.head,
    };
    if let Some(review) = cached_review(&client, &request) {
        opened_changes.set(Some(review));
        preparing.set(false);
        error.set(None);
        return;
    }
    opened_changes.set(None);
    preparing.set(true);
    error.set(None);
    spawn(async move {
        let result = client.request_cached(request, REVIEW_CACHE_MAX_AGE).await;
        if generation() != request_generation {
            return;
        }
        match result {
            Ok(response) => {
                if let WorkdeckResponse::Review(review) = response.payload {
                    opened_changes.set(Some(Arc::new(review)));
                }
            }
            Err(value) => error.set(Some(value.to_string())),
        }
        preparing.set(false);
    });
}

fn cached_review(client: &WorkdeckClient, request: &WorkdeckRequest) -> Option<Arc<ReviewSet>> {
    client
        .cached_response(request)
        .and_then(|response| match response.payload {
            WorkdeckResponse::Review(review) => Some(Arc::new(review)),
            _ => None,
        })
}

#[derive(Debug, Clone)]
enum RangeIntent {
    Clear,
    Compare(CommitRange),
}

fn render_range_controls(
    graph: Option<&GitGraph>,
    extra_rows: &[GitGraphRow],
    selected: Option<String>,
    anchor: Option<String>,
    preparing: bool,
    onintent: EventHandler<RangeIntent>,
) -> Element {
    let Some(anchor) = anchor else {
        return rsx! {};
    };
    let Some(graph) = graph else {
        return rsx! {};
    };
    let rows = combined_rows(graph, extra_rows);
    let range = normalized_range(&rows, Some(&anchor), selected.as_deref());

    rsx! {
        div { class: "git-range-controls", aria_label: "Commit range comparison controls",
            span { class: "git-range-anchor", "Anchor " code { "{short_oid(&anchor)}" } }
            if let Some(range) = range {
                button {
                    class: "button button--primary",
                    r#type: "button",
                    disabled: preparing,
                    onclick: move |_| onintent.call(RangeIntent::Compare(range.clone())),
                    if preparing { "Opening…" } else { "Compare range" }
                }
            } else {
                span { class: "git-range-hint", "Select another commit" }
            }
            button {
                class: "button button--quiet",
                r#type: "button",
                aria_label: "Cancel commit range",
                onclick: move |_| onintent.call(RangeIntent::Clear),
                "Cancel"
            }
        }
    }
}

#[component]
fn GitReferences(graph: GitGraph, onselect: EventHandler<String>) -> Element {
    let mut groups = BTreeMap::<u8, (String, Vec<GitReference>)>::new();
    for reference in graph.references {
        let (order, label) = reference_group(reference.kind);
        groups
            .entry(order)
            .or_insert_with(|| (label.into(), Vec::new()))
            .1
            .push(reference);
    }
    rsx! {
        aside { class: "git-references", aria_label: "Git references",
            div { class: "git-references__heading",
                strong { "References" }
                span { "{groups.values().map(|(_, values)| values.len()).sum::<usize>()}" }
            }
            for (_, (label, references)) in groups {
                section { class: "reference-group",
                    h3 { "{label}" }
                    for reference in references {
                        button {
                            class: if reference.current { "reference-row is-current" } else { "reference-row" },
                            r#type: "button",
                            title: "{reference.name} · {short_oid(&reference.target)}",
                            aria_label: "Open {reference.name} at {short_oid(&reference.target)}",
                            onclick: move |_| onselect.call(reference.target.clone()),
                            WorkdeckIcon { glyph: IconGlyph::Git, size: 13 }
                            span { "{reference.name}" }
                            if reference.current { span { class: "git-ref git-ref--head", "HEAD" } }
                            code { "{short_oid(&reference.target)}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn GitRow(
    row: GitGraphRow,
    selected: bool,
    range_anchor: bool,
    in_range: bool,
    position: usize,
    set_size: usize,
    graph_width: usize,
    lane_gap: usize,
    onselect: EventHandler<(String, bool)>,
) -> Element {
    let when = if row.wip {
        "Now".into()
    } else {
        row.timestamp.format("%b %-d · %H:%M").to_string()
    };
    let oid = row.oid.clone();
    let row_class = [
        "git-row",
        if selected { "is-selected" } else { "" },
        if range_anchor { "is-range-anchor" } else { "" },
        if in_range { "is-in-range" } else { "" },
        if row.wip { "is-wip" } else { "" },
    ]
    .join(" ");
    let aria_label = format!(
        "{}{}, {}, {}, {}",
        if row.wip {
            "Working tree changes, "
        } else {
            ""
        },
        row.subject,
        row.short_oid,
        row.author,
        when
    );
    rsx! {
        button {
            class: row_class,
            id: "commit-{row.short_oid}",
            "data-oid": "{row.oid}",
            role: "option",
            aria_label,
            aria_selected: selected,
            aria_posinset: position + 1,
            aria_setsize: set_size,
            r#type: "button",
            onclick: move |event| onselect.call((
                oid.clone(),
                event.modifiers().contains(Modifiers::SHIFT),
            )),
            GraphCell { row: row.clone(), graph_width, lane_gap }
            span { class: "git-row__commit",
                span { class: "git-row__subject", "{row.subject}" }
                span { class: "git-row__meta",
                    code { "{row.short_oid}" }
                    if row.wip { span { class: "git-ref git-ref--wip", "WIP" } }
                    for reference in row.references.iter() {
                        span { class: if row.head { "git-ref git-ref--head" } else { "git-ref" }, "{reference}" }
                    }
                    if row.edges.len() > 1 { span { class: "git-merge-label", "{row.edges.len()} parents" } }
                    span { class: "git-row__compact-meta",
                        span { "{when}" }
                        if row.wip {
                            span { "{row.files_changed} files" }
                        } else if row.additions > 0 || row.deletions > 0 {
                            span { class: "text-success", "+{row.additions}" }
                            span { class: "text-danger", "−{row.deletions}" }
                        }
                    }
                }
            }
            span { class: "git-row__author", "{row.author}" }
            span { class: "git-row__when", "{when}" }
            span { class: "git-row__changes",
                if row.wip {
                    span { "{row.files_changed} files" }
                } else if row.additions == 0 && row.deletions == 0 {
                    span { class: "muted", "—" }
                } else {
                    span { class: "text-success", "+{row.additions}" }
                    span { class: "text-danger", "−{row.deletions}" }
                }
            }
        }
    }
}

#[component]
fn GraphCell(row: GitGraphRow, graph_width: usize, lane_gap: usize) -> Element {
    let lane_x = |lane: usize| GRAPH_PADDING + lane * lane_gap;
    let height = ROW_HEIGHT as usize;
    let middle = height / 2;
    let edge_destinations = row
        .edges
        .iter()
        .map(|edge| edge.to_lane)
        .collect::<Vec<_>>();
    let node_x = lane_x(row.lane);
    rsx! {
        span { class: "git-row__graph",
            svg {
                width: "{graph_width}",
                height: "{height}",
                view_box: "0 0 {graph_width} {height}",
                preserve_aspect_ratio: "xMinYMid meet",
                "aria-hidden": "true",
                for lane in row.lanes_before.iter().copied() {
                    line {
                        x1: "{lane_x(lane)}",
                        y1: "0",
                        x2: "{lane_x(lane)}",
                        y2: "{middle}",
                        class: "graph-segment graph-lane--{lane % 8}"
                    }
                }
                for lane in row.lanes_after.iter().copied().filter(|lane| !edge_destinations.contains(lane)) {
                    line {
                        x1: "{lane_x(lane)}",
                        y1: "{middle}",
                        x2: "{lane_x(lane)}",
                        y2: "{height}",
                        class: "graph-segment graph-lane--{lane % 8}"
                    }
                }
                for edge in row.edges.iter() {
                    {
                        let from_x = lane_x(edge.from_lane);
                        let to_x = lane_x(edge.to_lane);
                        let path = if from_x == to_x {
                            format!("M {from_x} {middle} L {to_x} {height}")
                        } else {
                            format!(
                                "M {from_x} {middle} C {from_x} {}, {to_x} {}, {to_x} {height}",
                                middle + 9,
                                height - 9,
                            )
                        };
                        rsx! {
                            path {
                                d: path,
                                "data-parent-oid": "{edge.parent_oid}",
                                class: "graph-edge graph-lane--{edge.to_lane % 8}"
                            }
                        }
                    }
                }
                if row.wip {
                    path {
                        d: "M {node_x} {middle - 6} L {node_x + 6} {middle} L {node_x} {middle + 6} L {node_x - 6} {middle} Z",
                        class: "graph-node graph-node--wip graph-lane--{row.lane % 8}"
                    }
                } else {
                    circle {
                        cx: "{node_x}",
                        cy: "{middle}",
                        r: if row.head { "5" } else if row.edges.len() > 1 { "4.5" } else { "4" },
                        class: if row.head { "graph-node graph-node--head graph-lane--{row.lane % 8}" } else if row.edges.len() > 1 { "graph-node graph-node--merge graph-lane--{row.lane % 8}" } else { "graph-node graph-lane--{row.lane % 8}" }
                    }
                    if row.edges.len() > 1 {
                        circle {
                            cx: "{node_x}",
                            cy: "{middle}",
                            r: "1.5",
                            class: "graph-node__center graph-lane--{row.lane % 8}"
                        }
                    }
                }
            }
        }
    }
}

fn combined_rows(graph: &GitGraph, extra_rows: &[GitGraphRow]) -> Vec<GitGraphRow> {
    let mut rows = graph.rows.clone();
    rows = append_unique_rows(rows, extra_rows.to_vec());
    rows
}

fn append_unique_rows(
    mut existing: Vec<GitGraphRow>,
    incoming: Vec<GitGraphRow>,
) -> Vec<GitGraphRow> {
    for row in incoming {
        if !existing
            .iter()
            .any(|existing_row| existing_row.oid == row.oid)
        {
            existing.push(row);
        }
    }
    existing
}

fn filter_rows(rows: &[GitGraphRow], needle: &str) -> Vec<GitGraphRow> {
    rows.iter()
        .filter(|row| {
            needle.is_empty()
                || format!(
                    "{} {} {} {} {}",
                    row.subject,
                    row.author,
                    row.oid,
                    row.short_oid,
                    row.references.join(" ")
                )
                .to_ascii_lowercase()
                .contains(needle)
        })
        .cloned()
        .collect()
}

fn visible_row_count(graph: &GitGraph, extra_rows: &[GitGraphRow], needle: &str) -> usize {
    filter_rows(&combined_rows(graph, extra_rows), needle).len()
}

fn virtual_window(scroll_top: f64, viewport_height: f64, row_count: usize) -> (usize, usize) {
    if row_count == 0 {
        return (0, 0);
    }
    let visible = (viewport_height.max(ROW_HEIGHT) / ROW_HEIGHT).ceil() as usize + 1;
    let overscan_each_side = visible * (OVERSCAN_VIEWPORTS / 2);
    let first_visible = (scroll_top.max(0.0) / ROW_HEIGHT).floor() as usize;
    let start = first_visible
        .saturating_sub(overscan_each_side)
        .min(row_count);
    let end = first_visible
        .saturating_add(visible)
        .saturating_add(overscan_each_side)
        .min(row_count);
    (start.min(end), end)
}

fn graph_metrics(rows: &[GitGraphRow]) -> GraphMetrics {
    let max_lane = rows
        .iter()
        .flat_map(|row| {
            row.lanes_before
                .iter()
                .chain(row.lanes_after.iter())
                .chain(
                    row.edges
                        .iter()
                        .flat_map(|edge| [&edge.from_lane, &edge.to_lane]),
                )
                .chain(std::iter::once(&row.lane))
        })
        .copied()
        .max()
        .unwrap_or_default();
    graph_metrics_for_lane(max_lane)
}

fn graph_metrics_for_lane(max_lane: usize) -> GraphMetrics {
    let width = (GRAPH_PADDING * 2 + (max_lane + 1) * LANE_GAP).clamp(48, 240);
    let lane_gap = (width - GRAPH_PADDING * 2)
        .checked_div(max_lane)
        .unwrap_or(LANE_GAP)
        .clamp(2, LANE_GAP);
    GraphMetrics { width, lane_gap }
}

fn normalized_range(
    rows: &[GitGraphRow],
    anchor: Option<&str>,
    selected: Option<&str>,
) -> Option<CommitRange> {
    let anchor = anchor.filter(|oid| !is_wip_oid(oid))?;
    let selected = selected.filter(|oid| !is_wip_oid(oid))?;
    let anchor_index = rows.iter().position(|row| row.oid == anchor)?;
    let selected_index = rows.iter().position(|row| row.oid == selected)?;
    if anchor_index == selected_index {
        return None;
    }
    let (head_index, base_index) = if anchor_index < selected_index {
        (anchor_index, selected_index)
    } else {
        (selected_index, anchor_index)
    };
    Some(CommitRange {
        base: rows[base_index].oid.clone(),
        head: rows[head_index].oid.clone(),
    })
}

fn row_in_range(rows: &[GitGraphRow], oid: &str, range: &CommitRange) -> bool {
    let Some(index) = rows.iter().position(|row| row.oid == oid) else {
        return false;
    };
    let Some(head) = rows.iter().position(|row| row.oid == range.head) else {
        return false;
    };
    let Some(base) = rows.iter().position(|row| row.oid == range.base) else {
        return false;
    };
    index >= head && index <= base
}

fn keyboard_selection(rows: &[GitGraphRow], selected: Option<&str>, key: Key) -> Option<String> {
    let current = selected.and_then(|oid| rows.iter().position(|row| row.oid == oid));
    let index = match key {
        Key::ArrowDown => current.map_or(0, |index| (index + 1).min(rows.len().saturating_sub(1))),
        Key::ArrowUp => current.map_or(0, |index| index.saturating_sub(1)),
        Key::Home => 0,
        Key::End => rows.len().checked_sub(1)?,
        _ => return None,
    };
    rows.get(index).map(|row| row.oid.clone())
}

fn reference_group(kind: GitReferenceKind) -> (u8, &'static str) {
    match kind {
        GitReferenceKind::LocalBranch => (0, "Local branches"),
        GitReferenceKind::RemoteBranch => (1, "Remotes"),
        GitReferenceKind::Tag => (2, "Tags"),
        GitReferenceKind::Stash => (3, "Stashes"),
    }
}

fn short_oid(oid: &str) -> String {
    oid.chars().take(7).collect()
}

fn is_wip_oid(oid: &str) -> bool {
    oid.starts_with("wip")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn row(oid: &str, lane: usize, wip: bool) -> GitGraphRow {
        GitGraphRow {
            oid: oid.into(),
            short_oid: short_oid(oid),
            subject: oid.into(),
            author: "Workdeck".into(),
            timestamp: Utc::now(),
            lane,
            lanes_before: vec![lane],
            lanes_after: vec![lane],
            edges: Vec::new(),
            references: Vec::new(),
            head: false,
            wip,
            additions: 0,
            deletions: 0,
            files_changed: 0,
        }
    }

    #[test]
    fn range_normalization_always_uses_older_commit_as_base() {
        let rows = vec![
            row("newest", 0, false),
            row("middle", 0, false),
            row("oldest", 0, false),
        ];
        let forward = normalized_range(&rows, Some("newest"), Some("oldest")).unwrap();
        let reverse = normalized_range(&rows, Some("oldest"), Some("newest")).unwrap();
        assert_eq!(forward, reverse);
        assert_eq!(forward.base, "oldest");
        assert_eq!(forward.head, "newest");
        assert!(normalized_range(&rows, Some("newest"), Some("newest")).is_none());
    }

    #[test]
    fn working_tree_cannot_enter_a_commit_range() {
        let rows = vec![
            row("wip:test", 0, true),
            row("head", 0, false),
            row("base", 0, false),
        ];
        assert!(normalized_range(&rows, Some("wip:test"), Some("base")).is_none());
        assert!(normalized_range(&rows, Some("head"), Some("wip:test")).is_none());
    }

    #[test]
    fn virtual_window_is_bounded_to_visible_rows_plus_four_viewports() {
        let (start, end) = virtual_window(24_000.0, 600.0, 2_000);
        let visible = (600.0 / ROW_HEIGHT).ceil() as usize + 1;
        assert!(start > 0);
        assert!(end < 2_000);
        assert!(end - start <= visible * (OVERSCAN_VIEWPORTS + 1));

        assert_eq!(virtual_window(0.0, 600.0, 10), (0, 10));
        assert_eq!(virtual_window(0.0, 600.0, 0), (0, 0));
    }

    #[test]
    fn pagination_deduplicates_boundary_rows_without_reordering() {
        let rows = append_unique_rows(
            vec![row("one", 0, false), row("two", 0, false)],
            vec![row("two", 0, false), row("three", 1, false)],
        );
        assert_eq!(
            rows.iter().map(|row| row.oid.as_str()).collect::<Vec<_>>(),
            vec!["one", "two", "three"]
        );
    }

    #[test]
    fn graph_width_accounts_for_every_edge_lane_and_stays_bounded() {
        let mut value = row("merge", 0, false);
        value.edges = vec![workdeck_api::GitGraphEdge {
            from_lane: 0,
            to_lane: 6,
            parent_oid: "parent".into(),
        }];
        assert_eq!(graph_metrics(&[value]), graph_metrics_for_lane(6));
        let stressed = graph_metrics_for_lane(100);
        assert_eq!(stressed.width, 240);
        assert_eq!(stressed.lane_gap, 2);
        assert!(GRAPH_PADDING + 100 * stressed.lane_gap < stressed.width);
    }

    #[test]
    fn keyboard_navigation_is_stable_at_history_boundaries() {
        let rows = vec![
            row("one", 0, false),
            row("two", 0, false),
            row("three", 0, false),
        ];
        assert_eq!(
            keyboard_selection(&rows, None, Key::ArrowDown).as_deref(),
            Some("one")
        );
        assert_eq!(
            keyboard_selection(&rows, Some("three"), Key::ArrowDown).as_deref(),
            Some("three")
        );
        assert_eq!(
            keyboard_selection(&rows, Some("one"), Key::ArrowUp).as_deref(),
            Some("one")
        );
        assert_eq!(
            keyboard_selection(&rows, Some("one"), Key::End).as_deref(),
            Some("three")
        );
    }
}
