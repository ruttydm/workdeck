use super::{IconButton, IconGlyph, StatusDot, WorkdeckIcon};
use crate::Area;
use dioxus::prelude::*;
use workdeck_api::{BootstrapSnapshot, OperationId, ProviderState, TaskProgress};

#[component]
pub fn GitWorkspaceTabs(
    pull_requests: bool,
    #[props(default)] pull_count: Option<usize>,
    oncommits: EventHandler<()>,
    onpullrequests: EventHandler<()>,
) -> Element {
    rsx! {
        nav {
            class: "git-workspace-tabs",
            role: "tablist",
            aria_label: "Git workspace",
            "data-workdeck-drag-region": "false",
            button {
                class: if !pull_requests { "git-workspace-tab is-selected" } else { "git-workspace-tab" },
                role: "tab",
                aria_selected: !pull_requests,
                r#type: "button",
                onclick: move |_| oncommits.call(()),
                WorkdeckIcon { glyph: IconGlyph::Git, size: 14 }
                "Commits"
            }
            button {
                class: if pull_requests { "git-workspace-tab is-selected" } else { "git-workspace-tab" },
                role: "tab",
                aria_selected: pull_requests,
                r#type: "button",
                onclick: move |_| onpullrequests.call(()),
                WorkdeckIcon { glyph: IconGlyph::PullRequest, size: 14 }
                "Pull requests"
                if let Some(count) = pull_count {
                    span { class: "git-workspace-tab__count", "{count}" }
                }
            }
        }
    }
}

#[component]
pub fn AppRail(active: Area, onselect: EventHandler<Area>) -> Element {
    let entries = [
        (Area::Inbox, IconGlyph::Inbox, "Updates", "⌘1"),
        (Area::Workspaces, IconGlyph::Workspaces, "Workspaces", "⌘2"),
        (Area::Git, IconGlyph::Git, "Git", "⌘3"),
        (Area::Ci, IconGlyph::Ci, "CI", "⌘5"),
        (Area::Search, IconGlyph::Search, "Search", "⌘6"),
    ];
    rsx! {
        nav { class: "app-rail", aria_label: "Global navigation",
            div { class: "app-rail__primary",
                for (area, glyph, label, shortcut) in entries {
                    button {
                        key: "{area.id()}",
                        class: if active == area || (area == Area::Git && active.is_git_workspace()) { "rail-button is-selected" } else { "rail-button" },
                        r#type: "button",
                        title: "{label} · {shortcut}",
                        aria_label: "{label}",
                        aria_current: if active == area || (area == Area::Git && active.is_git_workspace()) { "page" } else { "false" },
                        onclick: move |_| onselect.call(area),
                        WorkdeckIcon { glyph, size: 18 }
                    }
                }
            }
            div { class: "app-rail__bottom",
                button {
                    class: if active == Area::Artifacts { "rail-button is-selected" } else { "rail-button" },
                    r#type: "button",
                    title: "Artifacts · ⌘7",
                    aria_label: "Artifacts",
                    aria_current: if active == Area::Artifacts { "page" } else { "false" },
                    onclick: move |_| onselect.call(Area::Artifacts),
                    WorkdeckIcon { glyph: IconGlyph::Artifacts, size: 18 }
                }
            }
        }
    }
}

#[component]
pub fn AppTitlebar(
    active: Area,
    pull_request_count: usize,
    provider_state: Option<ProviderState>,
    navigator_available: bool,
    navigator_visible: bool,
    inspector_available: bool,
    inspector_visible: bool,
    oncommand: EventHandler<()>,
    onselect: EventHandler<Area>,
    ontoggle_navigator: EventHandler<MouseEvent>,
    ontoggle_inspector: EventHandler<MouseEvent>,
    onrefresh: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        header {
            class: if active.is_git_workspace() { "titlebar titlebar--git" } else { "titlebar" },
            "data-workdeck-drag-region": "true",
            div { class: "titlebar__traffic-space", aria_hidden: "true" }
            div { class: "titlebar__context", "data-workdeck-drag-region": "true",
                if navigator_available {
                    IconButton { label: "Toggle navigator", glyph: IconGlyph::Navigator, selected: navigator_visible, onclick: move |event| ontoggle_navigator.call(event) }
                }
                span { class: "titlebar__product", "Workdeck" }
                if active.is_git_workspace() {
                    GitWorkspaceTabs {
                        pull_requests: active == Area::PullRequests,
                        pull_count: (pull_request_count > 0).then_some(pull_request_count),
                        oncommits: move |_| onselect.call(Area::Git),
                        onpullrequests: move |_| onselect.call(Area::PullRequests),
                    }
                } else {
                    span { class: "titlebar__separator", "/" }
                    span { class: "titlebar__workspace", "{active.shell_label()}" }
                }
            }
            div { class: "titlebar__drag-space", "data-workdeck-drag-region": "true" }
            div { class: "titlebar__actions",
                if provider_state == Some(ProviderState::Offline) {
                    span { class: "titlebar__provider", StatusDot { tone: "danger" } "GitHub offline · local commits remain available" }
                }
                IconButton { label: "Refresh", glyph: IconGlyph::Refresh, onclick: move |event| onrefresh.call(event) }
                button { class: "command-trigger", r#type: "button", onclick: move |_| oncommand.call(()),
                    WorkdeckIcon { glyph: IconGlyph::Command, size: 14 }
                    span { "Search" }
                    kbd { "⌘K" }
                }
                if inspector_available {
                    IconButton { label: "Toggle inspector", glyph: IconGlyph::Inspector, selected: inspector_visible, onclick: move |event| ontoggle_inspector.call(event) }
                }
            }
        }
    }
}

#[component]
pub fn CommandPalette(onselect: EventHandler<Area>, onclose: EventHandler<()>) -> Element {
    let mut query = use_signal(String::new);
    let needle = query().trim().to_ascii_lowercase();
    let commands = [
        (Area::Inbox, IconGlyph::Inbox, "Open Updates", "⌘1"),
        (
            Area::Workspaces,
            IconGlyph::Workspaces,
            "Open Workspaces",
            "⌘2",
        ),
        (Area::Git, IconGlyph::Git, "Git: Open commits", "⌘3"),
        (
            Area::PullRequests,
            IconGlyph::PullRequest,
            "Git: Open pull requests",
            "⌘4",
        ),
        (Area::Ci, IconGlyph::Ci, "Open CI", "⌘5"),
        (Area::Search, IconGlyph::Search, "Search portfolio", "⌘6"),
        (
            Area::Artifacts,
            IconGlyph::Artifacts,
            "Open Artifacts",
            "⌘7",
        ),
    ];
    rsx! {
        div { class: "modal-backdrop", role: "presentation", onclick: move |_| onclose.call(()),
            section { class: "command-palette", role: "dialog", aria_modal: "true", aria_label: "Command palette", onclick: move |event| event.stop_propagation(),
                label { class: "command-palette__input", WorkdeckIcon { glyph: IconGlyph::Search, size: 18 } input { r#type: "search", autofocus: true, placeholder: "Type a command…", value: "{query}", oninput: move |event| query.set(event.value()), onkeydown: move |event| if event.key().to_string() == "Escape" { onclose.call(()) } } kbd { "ESC" } }
                div { class: "command-palette__section", span { "NAVIGATE" }
                    for (area, glyph, label, shortcut) in commands.into_iter().filter(|(_, _, label, _)| needle.is_empty() || label.to_ascii_lowercase().contains(&needle)) {
                        button { class: "command-row", r#type: "button", onclick: move |_| { onselect.call(area); onclose.call(()); }, WorkdeckIcon { glyph, size: 16 } strong { "{label}" } if !shortcut.is_empty() { kbd { "{shortcut}" } } }
                    }
                }
            }
        }
    }
}

#[component]
pub fn Navigator(
    snapshot: BootstrapSnapshot,
    selected_project: Option<workdeck_api::ProjectId>,
    onselect_project: EventHandler<Option<workdeck_api::ProjectId>>,
) -> Element {
    let mut query = use_signal(String::new);
    let needle = query().trim().to_ascii_lowercase();
    let projects = snapshot
        .portfolio
        .projects
        .iter()
        .filter(|project| navigator_project_matches(project, &needle))
        .cloned()
        .collect::<Vec<_>>();
    rsx! {
        aside { class: "navigator", aria_label: "Context navigator",
            div { class: "navigator__header",
                div {
                    h2 { "Portfolio" }
                    p { {format!("{} projects · {} worktrees", snapshot.portfolio.project_count, snapshot.portfolio.worktree_count)} }
                }
            }
            div { class: "navigator__search",
                WorkdeckIcon { glyph: IconGlyph::Search, size: 14 }
                input { r#type: "search", placeholder: "Filter", aria_label: "Filter navigator", value: "{query}", oninput: move |event| query.set(event.value()) }
            }
            div { class: "navigator__content",
                button {
                    class: if selected_project.is_none() { "nav-row nav-row--project is-selected" } else { "nav-row nav-row--project" },
                    r#type: "button",
                    aria_pressed: selected_project.is_none(),
                    onclick: move |_| onselect_project.call(None),
                    WorkdeckIcon { glyph: IconGlyph::Workspaces, size: 15 }
                    span { class: "nav-row__label", "All projects" }
                    span { class: "nav-row__count", "{snapshot.portfolio.project_count}" }
                }
                for project in projects {
                    button {
                        key: "{project.id}",
                        class: if selected_project.as_ref() == Some(&project.id) { "nav-row nav-row--project is-selected" } else { "nav-row nav-row--project" },
                        r#type: "button",
                        aria_pressed: selected_project.as_ref() == Some(&project.id),
                        onclick: move |_| onselect_project.call(Some(project.id.clone())),
                        WorkdeckIcon { glyph: IconGlyph::Workspaces, size: 15 }
                        span { class: "nav-row__label", "{project.name}" }
                        if project.attention > 0 { span { class: "nav-row__count", "{project.attention}" } }
                    }
                }
            }
            div { class: "navigator__footer",
                span { {format!("{} unavailable", snapshot.portfolio.unavailable_count)} }
            }
        }
    }
}

#[component]
pub fn Inspector(active: Area, snapshot: BootstrapSnapshot) -> Element {
    let mut selected_tab = use_signal(|| "structure");
    let (heading, metrics, note) = inspector_summary(active, &snapshot);
    rsx! {
        aside { class: "inspector", aria_label: "Structure and evidence",
            div { class: "inspector__tabs", role: "tablist",
                button { class: if selected_tab() == "structure" { "inspector-tab is-selected" } else { "inspector-tab" }, role: "tab", aria_selected: selected_tab() == "structure", r#type: "button", onclick: move |_| selected_tab.set("structure"), "Structure" }
                button { class: if selected_tab() == "evidence" { "inspector-tab is-selected" } else { "inspector-tab" }, role: "tab", aria_selected: selected_tab() == "evidence", r#type: "button", onclick: move |_| selected_tab.set("evidence"), "Evidence" }
            }
            div { class: "inspector__content",
                if selected_tab() == "evidence" {
                    div { class: "inspector-section",
                        h3 { "Evidence" }
                        dl { class: "metadata-list",
                            div { dt { "Unread updates" } dd { {snapshot.inbox.unread.to_string()} } }
                            div { dt { "Commit branches" } dd { {snapshot.inbox.commit_updates.to_string()} } }
                            div { dt { "Pull requests" } dd { {snapshot.pull_requests.len().to_string()} } }
                            div { dt { "CI runs" } dd { {snapshot.ci_runs.len().to_string()} } }
                            div { dt { "Artifacts" } dd { {snapshot.artifacts.len().to_string()} } }
                        }
                    }
                    div { class: "inspector-section",
                        h3 { "Provider" }
                        div { class: "provider-state", StatusDot { tone: match snapshot.provider_state { ProviderState::Ready => "ready", ProviderState::Offline => "danger", _ => "attention" } } span { {format!("{:?}", snapshot.provider_state)} } }
                    }
                } else {
                    div { class: "inspector-section",
                        h3 { "{heading}" }
                        dl { class: "metadata-list",
                            for (label, value) in metrics {
                                div { dt { "{label}" } dd { "{value}" } }
                            }
                        }
                        if let Some(note) = note { p { class: "muted inspector-note", "{note}" } }
                    }
                }
            }
        }
    }
}

fn inspector_summary(
    active: Area,
    snapshot: &BootstrapSnapshot,
) -> (
    &'static str,
    Vec<(&'static str, String)>,
    Option<&'static str>,
) {
    match active {
        Area::Workspaces => (
            "Portfolio",
            vec![
                ("Projects", snapshot.portfolio.project_count.to_string()),
                (
                    "Repositories",
                    snapshot.portfolio.repository_count.to_string(),
                ),
                ("Worktrees", snapshot.portfolio.worktree_count.to_string()),
                (
                    "Unavailable",
                    snapshot.portfolio.unavailable_count.to_string(),
                ),
            ],
            None,
        ),
        Area::Git => (
            "Git evidence",
            vec![
                (
                    "Repositories",
                    snapshot.portfolio.repository_count.to_string(),
                ),
                ("Worktrees", snapshot.portfolio.worktree_count.to_string()),
                (
                    "Unavailable",
                    snapshot.portfolio.unavailable_count.to_string(),
                ),
            ],
            Some("History, references, stashes, and ranges are read-only."),
        ),
        _ => unreachable!("generic inspector rendered for a task-owned surface"),
    }
}

fn navigator_project_matches(project: &workdeck_api::ProjectNode, needle: &str) -> bool {
    needle.is_empty()
        || project.name.to_ascii_lowercase().contains(needle)
        || project.repositories.iter().any(|repository| {
            repository.name.to_ascii_lowercase().contains(needle)
                || repository.checkouts.iter().any(|checkout| {
                    checkout.worktrees.iter().any(|worktree| {
                        worktree
                            .branch
                            .as_deref()
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                            .contains(needle)
                            || worktree.path_hint.to_ascii_lowercase().contains(needle)
                    })
                })
        })
}

#[component]
pub fn StatusBar(
    snapshot: BootstrapSnapshot,
    progress: Option<TaskProgress>,
    error: Option<String>,
    oncancel: EventHandler<OperationId>,
) -> Element {
    rsx! {
        footer { class: "status-bar",
            div {
                StatusDot { tone: if snapshot.provider_state == ProviderState::Offline { "danger" } else { "ready" } }
                span { if snapshot.provider_state == ProviderState::Offline { "Provider offline" } else { "Ready" } }
            }
            div { class: "status-bar__summary", {format!("{} projects · {} worktrees · {} unread", snapshot.portfolio.project_count, snapshot.portfolio.worktree_count, snapshot.inbox.unread)} }
            div { class: "status-bar__tasks",
                if let Some(progress) = progress {
                    span { "{progress.label} · {progress.completed}/{progress.total}" }
                    if progress.cancellable { button { class: "status-cancel", r#type: "button", onclick: move |_| oncancel.call(progress.operation_id.clone()), "Cancel" } }
                } else if let Some(error) = error {
                    span { class: "text-danger", "{error}" }
                } else {
                    "No blocking tasks"
                }
            }
        }
    }
}
