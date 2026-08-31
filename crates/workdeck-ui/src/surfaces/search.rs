use crate::{
    SearchActivation,
    components::{EmptyState, IconGlyph, WorkdeckIcon},
};
use dioxus::prelude::*;
use futures_timer::Delay;
use std::time::Duration;
use workdeck_api::{
    RequestId, SearchResult, SearchResultKind, UiPreferencePatch, WorkdeckClient, WorkdeckRequest,
    WorkdeckResponse,
};

const SEARCH_CACHE_MAX_AGE: Duration = Duration::from_secs(30);
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);

#[component]
pub fn SearchSurface(
    client: WorkdeckClient,
    saved_searches: Vec<String>,
    recent_searches: Vec<String>,
    onactivate: EventHandler<SearchActivation>,
) -> Element {
    let mut query = use_signal(String::new);
    let mut saved = use_signal(|| saved_searches);
    let mut recent = use_signal(|| recent_searches);
    let search_client = client.clone();
    let results = use_resource(move || {
        let client = search_client.clone();
        let value = query();
        async move {
            if value.trim().is_empty() {
                return Ok(None);
            }
            Delay::new(SEARCH_DEBOUNCE).await;
            client
                .request_cached(
                    WorkdeckRequest::Search {
                        request_id: RequestId::new(),
                        query: value,
                    },
                    SEARCH_CACHE_MAX_AGE,
                )
                .await
                .map(Some)
        }
    });
    let snapshot = results
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|envelope| envelope.as_ref())
        .and_then(|envelope| match &envelope.payload {
            WorkdeckResponse::Search(search) => Some(search.clone()),
            _ => None,
        });
    let save_client = client.clone();
    let recent_remove_client = client.clone();
    let saved_remove_client = client.clone();
    rsx! {
        section { class: "surface search-surface",
            div { class: "global-search",
                WorkdeckIcon { glyph: IconGlyph::Search, size: 20 }
                input { r#type: "search", autofocus: true, placeholder: "Search projects, worktrees, commits, files, PRs, CI…", value: "{query}", oninput: move |event| query.set(event.value()) }
                if !query().trim().is_empty() {
                    button { class: "text-button", r#type: "button", disabled: saved().iter().any(|value| value == query().trim()), onclick: move |_| {
                        let mut values = saved();
                        values.retain(|value| value != query().trim());
                        values.insert(0, query().trim().to_owned());
                        values.truncate(24);
                        saved.set(values.clone());
                        save_search_preferences(save_client.clone(), Some(values), None);
                    }, "Save" }
                }
                kbd { "ESC" }
            }
            if query().trim().is_empty() {
                div { class: "search-start",
                    h2 { "Search across every project" }
                    p { "Results are grouped and bounded so you can move without losing context." }
                    div { class: "search-hints", span { "Try" } button { r#type: "button", onclick: move |_| query.set("sampleapp".into()), "sampleapp" } button { r#type: "button", onclick: move |_| query.set("PR".into()), "PR" } button { r#type: "button", onclick: move |_| query.set("dashboard".into()), "dashboard" } }
                    if !recent().is_empty() { SearchHistory { title: "Recent", values: recent(), onrun: move |value| query.set(value), onremove: move |value| { let mut values = recent(); values.retain(|candidate| candidate != &value); recent.set(values.clone()); save_search_preferences(recent_remove_client.clone(), None, Some(values)); } } }
                    if !saved().is_empty() { SearchHistory { title: "Saved", values: saved(), onrun: move |value| query.set(value), onremove: move |value| { let mut values = saved(); values.retain(|candidate| candidate != &value); saved.set(values.clone()); save_search_preferences(saved_remove_client.clone(), Some(values), None); } } }
                }
            } else if let Some(search) = snapshot {
                if search.total == 0 {
                    EmptyState { glyph: IconGlyph::Search, title: "No results".to_owned(), message: format!("Nothing matched “{}”.", search.query) }
                } else {
                    div { class: "search-results",
                        div { class: "search-results__summary", {format!("{} results for “{}”", search.total, search.query)} }
                        for group in search.groups {
                            section { class: "search-group",
                                h2 { "{group.label}" }
                                for result in group.results {
                                    SearchResultRow { key: "{group.label}-{result.id}", client: client.clone(), query: search.query.clone(), kind: group.kind, result, recent, onactivate }
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "surface-loading", div { class: "loading-spinner" } "Searching…" }
            }
        }
    }
}

#[component]
fn SearchResultRow(
    client: WorkdeckClient,
    query: String,
    kind: SearchResultKind,
    result: SearchResult,
    mut recent: Signal<Vec<String>>,
    onactivate: EventHandler<SearchActivation>,
) -> Element {
    rsx! {
        button { class: "search-result", r#type: "button", onclick: move |_| {
            let mut values = recent();
            values.retain(|candidate| candidate != &query);
            values.insert(0, query.clone());
            values.truncate(12);
            recent.set(values.clone());
            save_search_preferences(client.clone(), None, Some(values));
            onactivate.call(SearchActivation { kind, id: result.id.clone(), target: result.target.clone() });
        },
            span { class: "search-result__icon", WorkdeckIcon { glyph: match kind { SearchResultKind::Project | SearchResultKind::Repository | SearchResultKind::Checkout | SearchResultKind::Worktree => IconGlyph::Workspaces, SearchResultKind::PullRequest => IconGlyph::PullRequest, SearchResultKind::Ci => IconGlyph::Ci, SearchResultKind::Artifact => IconGlyph::Artifacts, _ => IconGlyph::File }, size: 16 } }
            span { class: "search-result__body", strong { "{result.title}" } small { "{result.subtitle}" } }
            span { class: "search-result__meta", "{result.metadata}" }
            WorkdeckIcon { glyph: IconGlyph::ChevronRight, size: 14 }
        }
    }
}

#[component]
fn SearchHistory(
    title: String,
    values: Vec<String>,
    onrun: EventHandler<String>,
    onremove: EventHandler<String>,
) -> Element {
    rsx! {
        section { class: "search-history", h3 { "{title}" }
            for value in values {
                SearchHistoryRow { key: "{title}-{value}", value, onrun, onremove }
            }
        }
    }
}

#[component]
fn SearchHistoryRow(
    value: String,
    onrun: EventHandler<String>,
    onremove: EventHandler<String>,
) -> Element {
    let run_value = value.clone();
    let remove_value = value.clone();
    rsx! {
        div { class: "search-history__row",
            button { class: "search-history__query", r#type: "button", onclick: move |_| onrun.call(run_value.clone()), WorkdeckIcon { glyph: IconGlyph::Clock, size: 13 } "{value}" }
            button { class: "search-history__remove", r#type: "button", aria_label: "Remove {value}", onclick: move |_| onremove.call(remove_value.clone()), "×" }
        }
    }
}

fn save_search_preferences(
    client: WorkdeckClient,
    saved_searches: Option<Vec<String>>,
    recent_searches: Option<Vec<String>>,
) {
    spawn(async move {
        let _ = client
            .request(WorkdeckRequest::UpdatePreferences {
                request_id: RequestId::new(),
                patch: UiPreferencePatch {
                    saved_searches,
                    recent_searches,
                    ..Default::default()
                },
            })
            .await;
    });
}
