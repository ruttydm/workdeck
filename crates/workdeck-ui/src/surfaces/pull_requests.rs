use crate::{
    components::{EmptyState, IconGlyph, PaneLayoutContext, PaneResizer, StatusDot, WorkdeckIcon},
    surfaces::{TaskChangesSurface, TaskDiffContext},
};
use dioxus::prelude::*;
use futures::future::join_all;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use workdeck_api::{
    ActivityTarget, PullRequest, PullRequestState, RequestId, ReviewSet, WorkdeckClient,
    WorkdeckRequest, WorkdeckResponse,
};

const MAX_PORTFOLIO_PULL_REPOSITORIES: usize = 12;
const PULL_CACHE_MAX_AGE: Duration = Duration::from_secs(60);
const REVIEW_CACHE_MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Clone, Default, PartialEq)]
struct PullLoad {
    pulls: Vec<PullRequest>,
    failed_repositories: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PullRequestFilter {
    Unread,
    #[default]
    Open,
    Merged,
    Closed,
    All,
}

impl PullRequestFilter {
    const ALL: [(Self, &'static str); 5] = [
        (Self::Unread, "Unread"),
        (Self::Open, "Open"),
        (Self::Merged, "Merged"),
        (Self::Closed, "Closed"),
        (Self::All, "All"),
    ];

    const fn id(self) -> &'static str {
        match self {
            Self::Unread => "unread",
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Unread => "Unread",
            Self::Open => "Open",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::All => "All",
        }
    }

    fn from_id(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find_map(|(filter, _)| (filter.id() == value).then_some(filter))
            .unwrap_or_default()
    }

    fn includes(self, state: PullRequestState, unread: bool) -> bool {
        match self {
            Self::Unread => unread,
            Self::Open => matches!(state, PullRequestState::Open | PullRequestState::Draft),
            Self::Merged => state == PullRequestState::Merged,
            Self::Closed => state == PullRequestState::Closed,
            Self::All => true,
        }
    }
}

#[component]
pub fn PullRequestsSurface(
    client: WorkdeckClient,
    repositories: Vec<String>,
    repository: Option<String>,
    onrepositorychange: EventHandler<Option<String>>,
    initial_pulls: Vec<PullRequest>,
    onchanged: EventHandler<()>,
) -> Element {
    let mut generation = use_signal(|| 0_u64);
    let mut query = use_signal(String::new);
    let mut filter = use_signal(PullRequestFilter::default);
    let mut selected = use_signal(|| None::<String>);
    let mut locally_read = use_signal(BTreeSet::<String>::new);
    let mut opened_changes = use_signal(|| None::<Arc<ReviewSet>>);
    let mut preparing_changes = use_signal(|| false);
    let mut changes_error = use_signal(|| None::<String>);
    let mut changes_generation = use_signal(|| 0_u64);
    let mut filter_open = use_signal(|| false);
    let mut master_visible = use_signal(|| true);
    let mut resizing = use_signal(|| None::<(f64, f64)>);
    let mut retained_load = use_signal(|| None::<PullLoad>);
    let pane_layout = use_context::<PaneLayoutContext>();
    let request_client = client.clone();
    let refresh_client = client.clone();
    let refresh_repository = repository.clone();
    let refresh_repositories = repositories.clone();
    let requested_repository = repository.clone();
    let requested_repositories = repositories.clone();
    let remote = use_resource(move || {
        let client = request_client.clone();
        let repository = requested_repository.clone();
        let repositories = requested_repositories.clone();
        let _generation = generation();
        async move { load_pull_requests(client, repository, repositories).await }
    });
    let remote_read = remote.read();
    let remote_load = remote_read
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned();
    if remote_load.is_some() && retained_load() != remote_load {
        retained_load.set(remote_load.clone());
    }
    let effective_load = match remote_load {
        Some(load) => Some(load),
        None => retained_load(),
    };
    let loading = remote_read.is_none() && initial_pulls.is_empty();
    let load_error = remote_read
        .as_ref()
        .and_then(|result| result.as_ref().err())
        .map(ToString::to_string);
    let partial_failures = effective_load
        .as_ref()
        .map_or(0, |load| load.failed_repositories);
    let all_pulls = effective_load
        .as_ref()
        .map(|load| load.pulls.clone())
        .unwrap_or(initial_pulls);
    let retained_selection = selected();
    let needle = query().trim().to_ascii_lowercase();
    let pulls = all_pulls
        .iter()
        .filter(|pull| {
            pull_matches_view(
                filter(),
                pull.state,
                &pull_key(pull),
                effective_unread(pull, &locally_read()),
                retained_selection.as_deref(),
            )
        })
        .filter(|pull| {
            needle.is_empty()
                || format!(
                    "{} {} {} {} #{}",
                    pull.title, pull.repository, pull.author, pull.branch, pull.number
                )
                .to_ascii_lowercase()
                .contains(&needle)
        })
        .cloned()
        .collect::<Vec<_>>();
    let selected_key = selected().filter(|key| pulls.iter().any(|pull| pull_key(pull) == *key));
    if selected().is_some() && selected_key.is_none() {
        changes_generation += 1;
        selected.set(None);
        opened_changes.set(None);
        preparing_changes.set(false);
        changes_error.set(None);
    }
    let selected_pull = pulls
        .iter()
        .find(|pull| Some(pull_key(pull)) == selected_key)
        .cloned();
    if selected_key.is_none()
        && selected().is_none()
        && !loading
        && load_error.is_none()
        && !preparing_changes()
        && opened_changes().is_none()
        && changes_error().is_none()
        && let Some(pull) = pulls.first().cloned()
    {
        selected.set(Some(pull_key(&pull)));
        prepare_pull_changes(
            client.clone(),
            pull,
            opened_changes,
            preparing_changes,
            changes_error,
            changes_generation,
        );
    }
    let groups = group_pull_requests(&pulls, &locally_read(), selected_key.as_deref());
    let scope_context = repository.clone().unwrap_or_else(|| {
        let count = repositories.len().min(MAX_PORTFOLIO_PULL_REPOSITORIES);
        format!("{count} active repositories")
    });
    let list_width = pane_layout.current().master_list;
    let layout_class = if master_visible() {
        "master-detail pr-master-detail"
    } else {
        "master-detail pr-master-detail master-collapsed"
    };

    rsx! {
        section {
            class: "surface pr-surface",
            style: "--master-list-width: {list_width}px",
            onkeydown: move |event: KeyboardEvent| if event.key() == Key::Escape { filter_open.set(false) },
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
            div { class: layout_class,
                if master_visible() {
                aside { class: "master-list pr-master", aria_label: "Pull request browser",
                    div { class: "master-list__toolbar pr-list-toolbar",
                        button {
                            class: "icon-button is-selected git-master-toggle",
                            r#type: "button",
                            aria_label: "Hide pull request list",
                            aria_pressed: "true",
                            title: "Hide pull request list",
                            onclick: move |_| master_visible.set(false),
                            WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 }
                        }
                        label { class: "search-field search-field--compact",
                            WorkdeckIcon { glyph: IconGlyph::Search, size: 14 }
                            input {
                                r#type: "search",
                                placeholder: "Search pull requests, or label:bug",
                                aria_label: "Search pull requests",
                                value: "{query}",
                                oninput: move |event| query.set(event.value())
                            }
                        }
                        div { class: "pr-filter-menu",
                            button {
                                class: "icon-button",
                                r#type: "button",
                                aria_label: "Filter pull requests",
                                aria_expanded: filter_open(),
                                title: "Filter pull requests · {filter().label()}",
                                onclick: move |_| filter_open.toggle(),
                                WorkdeckIcon { glyph: IconGlyph::Filter, size: 15 }
                            }
                            if filter_open() { div { class: "pr-filter-popover",
                                label { "Repository"
                                    select {
                                        class: "provider-select",
                                        aria_label: "GitHub repository",
                                        value: repository.clone().unwrap_or_default(),
                                        onchange: move |event| {
                                            opened_changes.set(None);
                                            preparing_changes.set(false);
                                            changes_error.set(None);
                                            selected.set(None);
                                            filter_open.set(false);
                                            let value = event.value();
                                            onrepositorychange.call((!value.is_empty()).then_some(value));
                                        },
                                        option { value: "", "Active repositories" }
                                        if repositories.is_empty() {
                                            option { value: "", "No GitHub repositories" }
                                        }
                                        for provider in repositories.clone() {
                                            option { value: "{provider}", "{provider}" }
                                        }
                                    }
                                }
                                label { "State"
                                    select {
                                        aria_label: "Pull request state",
                                        value: "{filter().id()}",
                                        onchange: move |event| {
                                            opened_changes.set(None);
                                            preparing_changes.set(false);
                                            changes_error.set(None);
                                            filter.set(PullRequestFilter::from_id(&event.value()));
                                            filter_open.set(false);
                                        },
                                        for (value, label) in PullRequestFilter::ALL {
                                            option { value: "{value.id()}", "{label}" }
                                        }
                                    }
                                }
                            } }
                        }
                        button {
                            class: "icon-button",
                            r#type: "button",
                            aria_label: "Refresh pull requests",
                            onclick: move |_| {
                                invalidate_pull_request_cache(
                                    &refresh_client,
                                    refresh_repository.as_ref(),
                                    &refresh_repositories,
                                );
                                generation += 1;
                            },
                            WorkdeckIcon { glyph: IconGlyph::Refresh, size: 15 }
                        }
                    }
                    div { class: "pr-list-scope",
                        span {
                            "{filter().label()} · {scope_context}"
                            if partial_failures > 0 { " · {partial_failures} unavailable" }
                        }
                        span { "{pulls.len()}" }
                    }
                    if loading {
                        div { class: "surface-loading", div { class: "loading-spinner" } "Loading pull requests…" }
                    } else if let Some(message) = load_error {
                        div { class: "provider-error-state", role: "alert",
                            WorkdeckIcon { glyph: IconGlyph::Warning, size: 20 }
                            strong { "Pull requests unavailable" }
                            p { "{message}" }
                            button { class: "button", r#type: "button", onclick: move |_| generation += 1, "Try again" }
                        }
                    } else if pulls.is_empty() {
                        EmptyState {
                            glyph: IconGlyph::PullRequest,
                            title: "No matching pull requests".to_owned(),
                            message: repository.as_ref().map_or_else(
                                || "No GitHub repositories are available in this portfolio.".to_owned(),
                                |repository| format!("No {} pull requests match in {repository}.", filter().label().to_ascii_lowercase()),
                            )
                        }
                    } else {
                        div {
                            class: "pr-list",
                            role: "listbox",
                            aria_label: "Pull requests",
                            onkeydown: {
                                    let keyboard_pulls = pulls.clone();
                                    let keyboard_client = client.clone();
                                    move |event: KeyboardEvent| {
                                    let key = event.key().to_string();
                                    let current = selected_key.as_ref()
                                        .and_then(|key| keyboard_pulls.iter().position(|pull| pull_key(pull) == *key))
                                        .unwrap_or(0);
                                    let next = match key.as_str() {
                                        "ArrowDown" => Some((current + 1).min(keyboard_pulls.len() - 1)),
                                        "ArrowUp" => Some(current.saturating_sub(1)),
                                        "Home" => Some(0),
                                        "End" => Some(keyboard_pulls.len() - 1),
                                        _ => None,
                                    };
                                    if let Some(next) = next {
                                        let pull = keyboard_pulls[next].clone();
                                        selected.set(Some(pull_key(&pull)));
                                        prepare_pull_changes(
                                            keyboard_client.clone(),
                                            pull,
                                            opened_changes,
                                            preparing_changes,
                                            changes_error,
                                            changes_generation,
                                        );
                                        event.prevent_default();
                                    }
                                }
                            },
                            for (group_label, group_pulls) in groups {
                                div { class: "pr-list__group", role: "presentation",
                                    span { "{group_label}" }
                                    span { "{group_pulls.len()}" }
                                }
                                for pull in group_pulls {
                                    PullRequestRow {
                                        key: "pr-{pull.repository}-{pull.number}",
                                        pull: pull.clone(),
                                        selected: Some(pull_key(&pull)) == selected_key,
                                        unread: effective_unread(&pull, &locally_read()),
                                        onselect: {
                                            let client = client.clone();
                                            let pull = pull.clone();
                                            move |_| {
                                                selected.set(Some(pull_key(&pull)));
                                                prepare_pull_changes(
                                                    client.clone(),
                                                    pull.clone(),
                                                    opened_changes,
                                                    preparing_changes,
                                                    changes_error,
                                                    changes_generation,
                                                );
                                                if effective_unread(&pull, &locally_read()) {
                                                    let client = client.clone();
                                                    let pull = pull.clone();
                                                    spawn(async move {
                                                        if mark_pull_read(&client, &pull).await.is_ok() {
                                                            locally_read.write().insert(pull.activity_revision.clone());
                                                            onchanged.call(());
                                                        }
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                PaneResizer {
                    label: "Resize pull request list".to_owned(),
                    value: list_width,
                    min: 300.0,
                    max: 640.0,
                    default_value: 420.0,
                    onstart: move |event: PointerEvent| resizing.set(Some((event.client_coordinates().x, list_width))),
                    onchange: move |next| {
                        pane_layout.update(|widths| widths.master_list = next);
                        pane_layout.persist();
                    },
                }
                }
                if !master_visible() {
                    button {
                        class: "icon-button pr-master-reveal",
                        r#type: "button",
                        aria_label: "Show pull request list",
                        aria_pressed: "false",
                        title: "Show pull request list",
                        onclick: move |_| master_visible.set(true),
                        WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 }
                    }
                }
                if let (Some(review), Some(pull)) = (opened_changes(), selected_pull.clone()) {
                    TaskChangesSurface {
                        key: "changes-{pull.repository}-{pull.number}",
                        review,
                        context: TaskDiffContext {
                            eyebrow: format!("{} #{} · {} ← {}", pull.repository, pull.number, pull.base, pull.branch),
                            title: pull.title.clone(),
                            status: Some(pull_check_label(&pull)),
                            external_url: Some(pull.url.clone()),
                        },
                    }
                } else if preparing_changes() {
                    div { class: "task-selection-state", role: "status",
                        div { class: "loading-spinner" }
                        strong { "Preparing pull request diff…" }
                        if let Some(pull) = selected_pull.as_ref() { p { "#{pull.number} · {pull.title}" } }
                    }
                } else if let Some(message) = changes_error() {
                    div { class: "task-selection-state", role: "alert",
                        WorkdeckIcon { glyph: IconGlyph::Warning, size: 20 }
                        strong { "Could not prepare this diff" }
                        p { "{message}" }
                        if let Some(pull) = selected_pull {
                            button {
                                class: "button button--primary",
                                r#type: "button",
                                onclick: move |_| prepare_pull_changes(
                                    client.clone(),
                                    pull.clone(),
                                    opened_changes,
                                    preparing_changes,
                                    changes_error,
                                    changes_generation,
                                ),
                                "Try again"
                            }
                        }
                    }
                } else {
                    div { class: "task-selection-state",
                        WorkdeckIcon { glyph: IconGlyph::PullRequest, size: 22 }
                        strong { "Select a pull request" }
                        p { "Its diff opens here immediately; changed files stay on the right." }
                    }
                }
            }
        }
    }
}

async fn load_pull_requests(
    client: WorkdeckClient,
    repository: Option<String>,
    repositories: Vec<String>,
) -> Result<PullLoad, workdeck_api::WorkdeckError> {
    if let Some(repository) = repository {
        return load_pull_request_repository(client, Some(repository))
            .await
            .map(|pulls| PullLoad {
                pulls,
                failed_repositories: 0,
            });
    }
    if repositories.is_empty() {
        return load_pull_request_repository(client, None)
            .await
            .map(|pulls| PullLoad {
                pulls,
                failed_repositories: 0,
            });
    }

    let requests = repositories
        .into_iter()
        .take(MAX_PORTFOLIO_PULL_REPOSITORIES)
        .map(|repository| load_pull_request_repository(client.clone(), Some(repository)));
    let results = join_all(requests).await;
    let mut first_error = None;
    let mut pulls = Vec::new();
    let mut failed_repositories = 0;
    for result in results {
        match result {
            Ok(mut repository_pulls) => pulls.append(&mut repository_pulls),
            Err(error) => {
                failed_repositories += 1;
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    pulls.sort_by_key(|pull| std::cmp::Reverse(pull.updated_at));
    pulls.dedup_by(|left, right| pull_key(left) == pull_key(right));
    if pulls.is_empty()
        && let Some(error) = first_error
    {
        return Err(error);
    }
    Ok(PullLoad {
        pulls,
        failed_repositories,
    })
}

fn invalidate_pull_request_cache(
    client: &WorkdeckClient,
    repository: Option<&String>,
    repositories: &[String],
) {
    let repositories = repository
        .cloned()
        .map(|repository| vec![Some(repository)])
        .unwrap_or_else(|| {
            if repositories.is_empty() {
                vec![None]
            } else {
                repositories
                    .iter()
                    .take(MAX_PORTFOLIO_PULL_REPOSITORIES)
                    .cloned()
                    .map(Some)
                    .collect()
            }
        });
    for repository in repositories {
        client.invalidate_cached(&WorkdeckRequest::LoadPullRequests {
            request_id: RequestId::new(),
            repository,
        });
    }
}

async fn load_pull_request_repository(
    client: WorkdeckClient,
    repository: Option<String>,
) -> Result<Vec<PullRequest>, workdeck_api::WorkdeckError> {
    let response = client
        .request_cached(
            WorkdeckRequest::LoadPullRequests {
                request_id: RequestId::new(),
                repository,
            },
            PULL_CACHE_MAX_AGE,
        )
        .await?;
    match response.payload {
        WorkdeckResponse::PullRequests(pulls) => Ok(pulls),
        _ => Err(workdeck_api::WorkdeckError::Internal(
            "Workdeck returned an unexpected pull-request response".into(),
        )),
    }
}

#[component]
fn PullRequestRow(
    pull: PullRequest,
    selected: bool,
    unread: bool,
    onselect: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            class: match (selected, unread) {
                (true, true) => "pr-row is-selected is-unread",
                (true, false) => "pr-row is-selected",
                (false, true) => "pr-row is-unread",
                (false, false) => "pr-row",
            },
            role: "option",
            aria_selected: selected,
            aria_label: "Open pull request {pull.repository} number {pull.number}: {pull.title}",
            r#type: "button",
            onclick: move |_| onselect.call(()),
            span { class: "pr-row__state",
                if unread { span { class: "unread-dot unread-dot--row", aria_label: "Unread" } }
                StatusDot { tone: pull_tone(&pull) }
            }
            span { class: "pr-row__body",
                strong { "{pull.title}" }
                small { "#{pull.number} · {pull.repository} · {pull.author}" }
            }
            span { class: "pr-row__meta",
                time {
                    datetime: pull.updated_at.to_rfc3339(),
                    {pull.updated_at.format("%b %-d").to_string()}
                }
                span { class: "pr-row__diff",
                    span { class: "text-success", "+{pull.additions}" }
                    span { class: "text-danger", "−{pull.deletions}" }
                }
            }
        }
    }
}

fn pull_key(pull: &PullRequest) -> String {
    format!("{}#{}", pull.repository, pull.number)
}

fn prepare_pull_changes(
    client: WorkdeckClient,
    pull: PullRequest,
    mut opened_changes: Signal<Option<Arc<ReviewSet>>>,
    mut preparing: Signal<bool>,
    mut error: Signal<Option<String>>,
    mut generation: Signal<u64>,
) {
    generation += 1;
    let request_generation = generation();
    let request = WorkdeckRequest::PreparePullRequestReview {
        request_id: RequestId::new(),
        repository: pull.repository,
        number: pull.number,
        title: pull.title,
    };
    if let Some(review) =
        client
            .cached_response(&request)
            .and_then(|response| match response.payload {
                WorkdeckResponse::Review(review) => Some(Arc::new(review)),
                _ => None,
            })
    {
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

fn pull_check_label(pull: &PullRequest) -> String {
    if pull.checks.failed > 0 {
        format!("{} checks failed", pull.checks.failed)
    } else if pull.checks.pending > 0 {
        format!("{} checks pending", pull.checks.pending)
    } else {
        "Checks passed".into()
    }
}

async fn mark_pull_read(client: &WorkdeckClient, pull: &PullRequest) -> Result<(), String> {
    client
        .request(WorkdeckRequest::MarkActivityRead {
            request_id: RequestId::new(),
            target: ActivityTarget::PullRequest {
                repository: pull.repository.clone(),
                number: pull.number,
            },
            revision: pull.activity_revision.clone(),
        })
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn effective_unread(pull: &PullRequest, locally_read: &BTreeSet<String>) -> bool {
    pull.unread && !locally_read.contains(&pull.activity_revision)
}

fn group_pull_requests(
    pulls: &[PullRequest],
    locally_read: &BTreeSet<String>,
    retained_selection: Option<&str>,
) -> Vec<(&'static str, Vec<PullRequest>)> {
    let unread = pulls
        .iter()
        .filter(|pull| {
            effective_unread(pull, locally_read)
                || (pull.unread && retained_selection == Some(pull_key(pull).as_str()))
        })
        .cloned()
        .collect::<Vec<_>>();
    let others = pulls
        .iter()
        .filter(|pull| {
            !(effective_unread(pull, locally_read)
                || pull.unread && retained_selection == Some(pull_key(pull).as_str()))
        })
        .cloned()
        .collect::<Vec<_>>();
    [("Unread", unread), ("Others", others)]
        .into_iter()
        .filter(|(_, pulls)| !pulls.is_empty())
        .collect()
}

fn pull_tone(pull: &PullRequest) -> &'static str {
    if pull.checks.failed > 0 {
        "danger"
    } else if pull.checks.pending > 0 || pull.state == PullRequestState::Draft {
        "attention"
    } else {
        "ready"
    }
}

fn pull_matches_view(
    filter: PullRequestFilter,
    state: PullRequestState,
    key: &str,
    effective_unread: bool,
    retained_selection: Option<&str>,
) -> bool {
    filter.includes(state, effective_unread)
        || (filter == PullRequestFilter::Unread && retained_selection == Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_api::FixtureWorkdeckClient;

    #[test]
    fn open_filter_includes_drafts_but_not_finished_pull_requests() {
        assert!(PullRequestFilter::Open.includes(PullRequestState::Open, false));
        assert!(PullRequestFilter::Open.includes(PullRequestState::Draft, false));
        assert!(!PullRequestFilter::Open.includes(PullRequestState::Merged, false));
        assert!(!PullRequestFilter::Open.includes(PullRequestState::Closed, false));
        assert!(PullRequestFilter::Unread.includes(PullRequestState::Closed, true));
        assert!(!PullRequestFilter::Unread.includes(PullRequestState::Open, false));
    }

    #[test]
    fn filter_ids_round_trip() {
        for (filter, _) in PullRequestFilter::ALL {
            assert_eq!(PullRequestFilter::from_id(filter.id()), filter);
        }
        assert_eq!(
            PullRequestFilter::from_id("unknown"),
            PullRequestFilter::Open
        );
    }

    #[test]
    fn unread_view_retains_the_open_pull_after_its_cursor_advances() {
        assert!(!pull_matches_view(
            PullRequestFilter::Unread,
            PullRequestState::Open,
            "repo#42",
            false,
            None,
        ));
        assert!(pull_matches_view(
            PullRequestFilter::Unread,
            PullRequestState::Open,
            "repo#42",
            false,
            Some("repo#42"),
        ));
    }

    #[test]
    fn unread_group_is_always_first_and_empty_groups_are_removed() {
        let pulls = workdeck_api::fixtures::polished().pull_requests;
        let groups = group_pull_requests(&pulls, &BTreeSet::new(), None);
        assert_eq!(groups.first().map(|group| group.0), Some("Unread"));
        assert!(groups.iter().all(|group| !group.1.is_empty()));
    }

    #[test]
    fn selected_unread_pull_stays_in_place_after_local_read_advances() {
        let pulls = workdeck_api::fixtures::polished().pull_requests;
        let selected = pulls.first().expect("polished pull request");
        let selected_key = pull_key(selected);
        let locally_read = BTreeSet::from([selected.activity_revision.clone()]);

        assert!(!effective_unread(selected, &locally_read));
        let groups = group_pull_requests(&pulls, &locally_read, Some(&selected_key));
        let unread = groups
            .iter()
            .find(|(label, _)| *label == "Unread")
            .expect("selected row is retained in its current group");
        assert_eq!(unread.1.first().map(pull_key), Some(selected_key));
    }

    #[test]
    fn portfolio_loader_merges_provider_lists_without_duplicate_keys() {
        let client = FixtureWorkdeckClient::polished().into_client();
        let mut pool = futures::executor::LocalPool::new();
        let load = pool
            .run_until(load_pull_requests(
                client,
                None,
                vec![
                    "example/sampleapp".into(),
                    "example/workdeck".into(),
                    "example/sampleapp".into(),
                ],
            ))
            .expect("fixture portfolio pulls");
        let pulls = load.pulls;
        assert_eq!(pulls.len(), 2);
        assert!(pulls[0].updated_at >= pulls[1].updated_at);
        assert_eq!(
            pulls.iter().map(pull_key).collect::<BTreeSet<_>>().len(),
            pulls.len()
        );
    }
}
