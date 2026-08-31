use crate::components::{Badge, EmptyState, IconGlyph, StatusDot, WorkdeckIcon};
use dioxus::prelude::*;
use std::collections::BTreeSet;
use workdeck_api::{
    Availability, BootstrapSnapshot, ProjectNode, RequestId, WorkdeckClient, WorkdeckRequest,
    WorktreeId,
};

#[component]
pub fn WorkspacesSurface(
    client: WorkdeckClient,
    snapshot: BootstrapSnapshot,
    selected_project: Option<workdeck_api::ProjectId>,
    onopen_git: EventHandler<WorktreeId>,
    onchanged: EventHandler<()>,
) -> Element {
    let mut query = use_signal(String::new);
    let mut collapsed = use_signal(BTreeSet::<String>::new);
    let mut adding = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let needle = query().trim().to_ascii_lowercase();
    let projects = snapshot
        .portfolio
        .projects
        .iter()
        .filter(|project| {
            selected_project
                .as_ref()
                .is_none_or(|selected| selected == &project.id)
        })
        .filter(|project| workspace_project_matches(project, &needle))
        .cloned()
        .collect::<Vec<_>>();
    rsx! {
        section { class: "surface workspace-surface",
            header { class: "surface-heading surface-heading--compact",
                div { p { class: "eyebrow", "PORTFOLIO" } h1 { "Workspaces" } p { "Every repository identity, checkout, and linked worktree in one place." } }
                div { class: "heading-actions",
                    button { class: "button button--primary", r#type: "button", disabled: adding(), onclick: move |_| {
                        let client = client.clone();
                        adding.set(true);
                        error.set(None);
                        spawn(async move {
                            match client.request(WorkdeckRequest::ChoosePortfolioRoot { request_id: RequestId::new() }).await {
                                Ok(_) => onchanged.call(()),
                                Err(value) => error.set(Some(value.to_string())),
                            }
                            adding.set(false);
                        });
                    }, WorkdeckIcon { glyph: IconGlyph::Plus, size: 14 } if adding() { "Adding…" } else { "Add root" } }
                }
            }
            if let Some(message) = error() { div { class: "inline-error surface-error", role: "alert", "{message}" } }
            div { class: "workspace-toolbar",
                label { class: "search-field",
                    WorkdeckIcon { glyph: IconGlyph::Search, size: 14 }
                    input { r#type: "search", placeholder: "Filter projects, repositories, branches…", value: "{query}", oninput: move |event| query.set(event.value()) }
                }
                button { class: "text-button", r#type: "button", onclick: move |_| collapsed.set(BTreeSet::new()), "Expand all" }
                button { class: "text-button", r#type: "button", onclick: move |_| {
                    let keys = snapshot.portfolio.projects.iter().flat_map(workspace_keys).collect();
                    collapsed.set(keys);
                }, "Collapse all" }
                span { class: "workspace-summary", {format!("{} projects · {} repositories · {} worktrees", snapshot.portfolio.project_count, snapshot.portfolio.repository_count, snapshot.portfolio.worktree_count)} }
            }
            if projects.is_empty() {
                EmptyState { glyph: IconGlyph::Search, title: "No matching workspaces".to_owned(), message: "Try a project, repository, checkout, path, or branch name.".to_owned() }
            } else {
                div { class: "workspace-table", role: "treegrid", aria_label: "Portfolio hierarchy",
                    div { class: "workspace-table__header", role: "row",
                        span { role: "columnheader", "Name" }
                        span { role: "columnheader", "State" }
                        span { role: "columnheader", "Last seen" }
                        span { role: "columnheader", "Attention" }
                    }
                    for project in projects {
                        {render_project(project, collapsed, onopen_git)}
                    }
                }
            }
        }
    }
}

fn render_project(
    project: ProjectNode,
    mut collapsed: Signal<BTreeSet<String>>,
    onopen_git: EventHandler<WorktreeId>,
) -> Element {
    let project_key = format!("project:{}", project.id);
    let project_collapsed = collapsed().contains(&project_key);
    rsx! {
        div { class: "workspace-project", key: "{project.id}",
            button { class: "workspace-row workspace-row--project", role: "row", aria_level: "1", aria_expanded: !project_collapsed, r#type: "button", onclick: move |_| toggle_collapsed(&mut collapsed, project_key.clone()),
                span { class: "workspace-cell workspace-cell--name", role: "gridcell", WorkdeckIcon { glyph: if project_collapsed { IconGlyph::ChevronRight } else { IconGlyph::ChevronDown }, size: 13 } WorkdeckIcon { glyph: IconGlyph::Workspaces, size: 15 } strong { "{project.name}" } }
                span { class: "workspace-cell", role: "gridcell", if project.empty { Badge { "Empty" } } else { "Active" } }
                span { class: "workspace-cell muted", role: "gridcell", "—" }
                span { class: "workspace-cell", role: "gridcell", if project.attention > 0 { Badge { tone: "incoming", {project.attention.to_string()} } } }
            }
            if !project_collapsed {
                for repository in project.repositories {
                    {render_repository(repository, collapsed, onopen_git)}
                }
            }
        }
    }
}

fn render_repository(
    repository: workdeck_api::RepositoryNode,
    mut collapsed: Signal<BTreeSet<String>>,
    onopen_git: EventHandler<WorktreeId>,
) -> Element {
    let key = format!("repository:{}", repository.id);
    let is_collapsed = collapsed().contains(&key);
    rsx! {
        div {
            button { class: "workspace-row workspace-row--repository", role: "row", aria_level: "2", aria_expanded: !is_collapsed, r#type: "button", onclick: move |_| toggle_collapsed(&mut collapsed, key.clone()),
                span { class: "workspace-cell workspace-cell--name", role: "gridcell", WorkdeckIcon { glyph: if is_collapsed { IconGlyph::ChevronRight } else { IconGlyph::ChevronDown }, size: 13 } WorkdeckIcon { glyph: IconGlyph::Git, size: 15 } strong { "{repository.name}" } if let Some(provider) = &repository.provider { small { "{provider}" } } }
                span { class: "workspace-cell", role: "gridcell", "Repository" }
                span { class: "workspace-cell muted", role: "gridcell", "—" }
                span { class: "workspace-cell", role: "gridcell", if repository.attention > 0 { {repository.attention.to_string()} } }
            }
            if !is_collapsed {
                for checkout in repository.checkouts {
                    {render_checkout(checkout, collapsed, onopen_git)}
                }
            }
        }
    }
}

fn render_checkout(
    checkout: workdeck_api::CheckoutNode,
    mut collapsed: Signal<BTreeSet<String>>,
    onopen_git: EventHandler<WorktreeId>,
) -> Element {
    let key = format!("checkout:{}", checkout.id);
    let is_collapsed = collapsed().contains(&key);
    rsx! {
        div {
            button { class: "workspace-row workspace-row--checkout", role: "row", aria_level: "3", aria_expanded: !is_collapsed, r#type: "button", onclick: move |_| toggle_collapsed(&mut collapsed, key.clone()),
                span { class: "workspace-cell workspace-cell--name", role: "gridcell", WorkdeckIcon { glyph: if is_collapsed { IconGlyph::ChevronRight } else { IconGlyph::ChevronDown }, size: 13 } span { "{checkout.label}" } }
                span { class: "workspace-cell", role: "gridcell", if checkout.available { "Available" } else { "Unavailable" } }
                span { class: "workspace-cell muted", role: "gridcell", "—" }
                span { class: "workspace-cell", role: "gridcell" }
            }
            if !is_collapsed {
                for worktree in checkout.worktrees {
                    button { class: "workspace-row workspace-row--worktree", role: "row", aria_level: "4", r#type: "button", disabled: !matches!(worktree.availability, Availability::Available | Availability::ScanWarning), onclick: move |_| onopen_git.call(worktree.id.clone()),
                        span { class: "workspace-cell workspace-cell--name", role: "gridcell",
                            StatusDot { tone: match worktree.availability { Availability::Available => "ready", Availability::ScanWarning => "attention", _ => "danger" } }
                            span { strong { {worktree.branch.clone().unwrap_or_else(|| "detached".into())} } small { "{worktree.path_hint}" } }
                        }
                        span { class: "workspace-cell", role: "gridcell", {format!("{:?}", worktree.availability)} }
                        span { class: "workspace-cell muted", role: "gridcell", {worktree.last_seen.format("%b %-d").to_string()} }
                        span { class: "workspace-cell", role: "gridcell", if worktree.changes > 0 || worktree.commits > 0 { Badge { tone: "incoming", {format!("{} changes · {} commits", worktree.changes, worktree.commits)} } } }
                    }
                }
            }
        }
    }
}

fn toggle_collapsed(collapsed: &mut Signal<BTreeSet<String>>, key: String) {
    let mut values = collapsed();
    if !values.insert(key.clone()) {
        values.remove(&key);
    }
    collapsed.set(values);
}

fn workspace_project_matches(project: &ProjectNode, needle: &str) -> bool {
    needle.is_empty()
        || project.name.to_ascii_lowercase().contains(needle)
        || project.repositories.iter().any(|repository| {
            repository.name.to_ascii_lowercase().contains(needle)
                || repository
                    .provider
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(needle)
                || repository.checkouts.iter().any(|checkout| {
                    checkout.label.to_ascii_lowercase().contains(needle)
                        || checkout.worktrees.iter().any(|worktree| {
                            worktree.path_hint.to_ascii_lowercase().contains(needle)
                                || worktree
                                    .branch
                                    .as_deref()
                                    .unwrap_or_default()
                                    .to_ascii_lowercase()
                                    .contains(needle)
                        })
                })
        })
}

fn workspace_keys(project: &ProjectNode) -> BTreeSet<String> {
    let mut keys = BTreeSet::from([format!("project:{}", project.id)]);
    for repository in &project.repositories {
        keys.insert(format!("repository:{}", repository.id));
        for checkout in &repository.checkouts {
            keys.insert(format!("checkout:{}", checkout.id));
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_workspace_search_reaches_branches() {
        let mut project = workdeck_api::fixtures::polished()
            .portfolio
            .projects
            .remove(0);
        project.repositories[0].checkouts[0].worktrees[0].branch = Some("agent/activity".into());
        assert!(workspace_project_matches(&project, "agent/activity"));
        assert!(!workspace_project_matches(&project, "does-not-exist"));
    }
}
