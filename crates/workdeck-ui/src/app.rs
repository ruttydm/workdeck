use crate::{
    Area, SearchActivation, WORKDECK_CSS,
    components::{
        AppRail, AppTitlebar, CommandPalette, Inspector, Navigator, PaneLayoutContext, StatusBar,
    },
    surfaces::{
        ArtifactsSurface, ChangesSurface, CiSurface, GitSurface, InboxSurface, OnboardingSurface,
        PullRequestsSurface, SearchSurface, WorkspacesSurface,
    },
};
use dioxus::prelude::*;
use std::{collections::BTreeSet, time::Duration};
use workdeck_api::{
    ActivityTarget, Availability, OperationId, RequestId, ReviewId, SearchResultKind, TaskProgress,
    UiPreferencePatch, WorkdeckClient, WorkdeckEvent, WorkdeckRequest, WorkdeckResponse,
    WorktreeId,
};

const PREWARM_GRAPH_MAX_AGE: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, PartialEq)]
enum ResizePane {
    Navigator,
    Inspector,
}

/// An application command delivered by the native shell. The monotonically
/// increasing sequence makes repeated invocations of the same command visible
/// to Dioxus without coupling `workdeck-ui` to a desktop runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeUiInvocation {
    pub sequence: u64,
    pub command: NativeUiCommand,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeUiCommand {
    #[default]
    Inbox,
    Workspaces,
    Git,
    PullRequests,
    Ci,
    Search,
    Artifacts,
    CommandPalette,
}

impl NativeUiCommand {
    fn area(self) -> Option<Area> {
        match self {
            Self::Inbox => Some(Area::Inbox),
            Self::Workspaces => Some(Area::Workspaces),
            Self::Git => Some(Area::Git),
            Self::PullRequests => Some(Area::PullRequests),
            Self::Ci => Some(Area::Ci),
            Self::Search => Some(Area::Search),
            Self::Artifacts => Some(Area::Artifacts),
            Self::CommandPalette => None,
        }
    }
}

#[component]
pub fn WorkdeckApp(
    client: WorkdeckClient,
    #[props(default)] initial_area: Option<Area>,
    #[props(default)] native_command: Option<Signal<NativeUiInvocation>>,
) -> Element {
    let mut active = use_signal(move || initial_area.unwrap_or(Area::Inbox));
    let mut navigator_visible = use_signal(|| true);
    let mut inspector_visible = use_signal(|| false);
    let mut selected_project = use_signal(|| None::<workdeck_api::ProjectId>);
    let mut selected_worktree = use_signal(|| None::<workdeck_api::WorktreeId>);
    let mut prewarmed_worktree = use_signal(|| None::<workdeck_api::WorktreeId>);
    let mut selected_review = use_signal(|| None::<ReviewId>);
    let mut selected_provider_repository = use_signal(|| None::<String>);
    let mut refresh_generation = use_signal(|| 0_u64);
    let mut command_open = use_signal(|| false);
    let mut preferences_applied = use_signal(|| false);
    let mut navigator_width = use_signal(|| 256.0_f64);
    let mut inspector_width = use_signal(|| 320.0_f64);
    let mut pane_widths = use_signal(workdeck_api::PaneWidths::default);
    let mut resizing = use_signal(|| None::<(ResizePane, f64, f64)>);
    let mut task_progress = use_signal(|| None::<TaskProgress>);
    let mut task_error = use_signal(|| None::<String>);
    let mut shell_mounted = use_signal(|| None::<MountedEvent>);
    let mut handled_native_command = use_signal(|| 0_u64);
    use_context_provider(|| PaneLayoutContext::new(pane_widths, client.clone()));

    use_effect(move || {
        let Some(command_signal) = native_command else {
            return;
        };
        let invocation = command_signal();
        if invocation.sequence == 0 || invocation.sequence <= handled_native_command() {
            return;
        }
        handled_native_command.set(invocation.sequence);
        if let Some(area) = invocation.command.area() {
            active.set(area);
        } else {
            command_open.set(true);
        }
    });
    let event_stream = use_hook(|| client.subscribe());
    use_future(move || {
        let events = event_stream.clone();
        async move {
            while let Ok(event) = events.recv().await {
                match event {
                    WorkdeckEvent::TaskProgress(progress) => task_progress.set(Some(progress)),
                    WorkdeckEvent::OperationCancelled { operation_id }
                        if task_progress()
                            .as_ref()
                            .is_some_and(|value| value.operation_id == operation_id) =>
                    {
                        task_progress.set(None)
                    }
                    WorkdeckEvent::Error { error, .. } => task_error.set(Some(error.to_string())),
                    _ => {}
                }
            }
        }
    });
    let bootstrap_client = client.clone();
    let bootstrap = use_resource(move || {
        let client = bootstrap_client.clone();
        let _generation = refresh_generation();
        async move {
            client
                .request(WorkdeckRequest::Bootstrap {
                    request_id: RequestId::new(),
                })
                .await
        }
    });

    let snapshot = bootstrap
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|envelope| match &envelope.payload {
            WorkdeckResponse::Bootstrap(snapshot) => Some(snapshot.clone()),
            _ => None,
        });

    if !preferences_applied()
        && let Some(snapshot) = snapshot.as_ref()
    {
        navigator_visible.set(snapshot.preferences.navigator_visible);
        inspector_visible.set(snapshot.preferences.inspector_visible);
        navigator_width.set(snapshot.preferences.navigator_width.max(208.0));
        inspector_width.set(snapshot.preferences.inspector_width.max(240.0));
        pane_widths.set(snapshot.preferences.pane_widths.clone().bounded());
        if initial_area.is_none()
            && let Some(area) = Area::from_id(&snapshot.preferences.selected_area)
        {
            active.set(area);
        }
        preferences_applied.set(true);
    }

    if let Some(snapshot) = snapshot.as_ref() {
        let default_worktree = default_available_worktree(snapshot);
        if selected_worktree().is_none()
            && let Some(worktree_id) = default_worktree.clone()
        {
            selected_worktree.set(Some(worktree_id));
        }
        let prewarm_target = selected_worktree()
            .filter(|worktree_id| is_available_worktree(snapshot, worktree_id))
            .or(default_worktree);
        if prewarm_target.is_some() && prewarmed_worktree() != prewarm_target {
            prewarmed_worktree.set(prewarm_target.clone());
            let client = client.clone();
            spawn(async move {
                let _ = client
                    .request_cached(
                        WorkdeckRequest::LoadGitGraph {
                            request_id: RequestId::new(),
                            worktree_id: prewarm_target,
                            cursor: None,
                        },
                        PREWARM_GRAPH_MAX_AGE,
                    )
                    .await;
            });
        }
    }

    let layout_style = format!(
        "--navigator-width: {}px; --inspector-width: {}px",
        navigator_width(),
        inspector_width()
    );
    let rail_preferences = client.clone();
    let title_preferences = client.clone();
    let navigator_preferences = client.clone();
    let inspector_preferences = client.clone();
    let resize_preferences = client.clone();
    let navigator_reset_preferences = client.clone();
    let navigator_key_preferences = client.clone();
    let inspector_reset_preferences = client.clone();
    let inspector_key_preferences = client.clone();
    let command_preferences = client.clone();
    let status_client = client.clone();
    let refresh_all_client = client.clone();
    let provider_repositories = snapshot
        .as_ref()
        .map(provider_repositories)
        .unwrap_or_default();
    let active_provider_repository =
        selected_provider_repository().filter(|selected| provider_repositories.contains(selected));
    let active_ci_repository = active_provider_repository
        .clone()
        .or_else(|| provider_repositories.first().cloned());
    let git_surface_key = selected_worktree()
        .map(|worktree| format!("git-{}", worktree.0))
        .unwrap_or_else(|| "git-portfolio".into());

    rsx! {
        // Keep the compiled design system in the Rust renderer. Direct Cargo
        // desktop builds do not rewrite Manganis stylesheet placeholders.
        document::Style { "{WORKDECK_CSS}" }
        main {
            class: "workdeck-app",
            "data-theme": "system",
            tabindex: "-1",
            onmounted: move |event| {
                shell_mounted.set(Some(event.clone()));
                restore_shell_focus(Some(event));
            },
            onkeydown: move |event| {
                let key = event.key().to_string().to_ascii_lowercase();
                if event.modifiers().contains(Modifiers::META) {
                    match key.as_str() {
                        "k" => command_open.set(true),
                        "1" => active.set(Area::Inbox),
                        "2" => active.set(Area::Workspaces),
                        "3" => active.set(Area::Git),
                        "4" => active.set(Area::PullRequests),
                        "5" => active.set(Area::Ci),
                        "6" => active.set(Area::Search),
                        "7" => active.set(Area::Artifacts),
                        _ => {}
                    }
                } else if key == "escape" {
                    command_open.set(false);
                }
            },
            AppTitlebar {
                active: active(),
                pull_request_count: snapshot.as_ref().map_or(0, |value| value.pull_requests.len()),
                provider_state: snapshot.as_ref().map(|value| value.provider_state),
                navigator_available: area_uses_navigator(active()),
                navigator_visible: navigator_visible(),
                inspector_available: area_supports_inspector(active()),
                inspector_visible: inspector_visible(),
                oncommand: move |_| command_open.set(true),
                onselect: {
                    let client = title_preferences.clone();
                    move |area| {
                        active.set(area);
                        persist_preferences(client.clone(), UiPreferencePatch { selected_area: Some(area.id().into()), ..Default::default() });
                    }
                },
                ontoggle_navigator: move |_| { WritableExt::toggle(&mut navigator_visible); persist_preferences(navigator_preferences.clone(), UiPreferencePatch { navigator_visible: Some(navigator_visible()), ..Default::default() }); },
                ontoggle_inspector: move |_| { WritableExt::toggle(&mut inspector_visible); persist_preferences(inspector_preferences.clone(), UiPreferencePatch { inspector_visible: Some(inspector_visible()), ..Default::default() }); },
                onrefresh: move |_| {
                    refresh_all_client.clear_cached();
                    refresh_generation += 1;
                },
            }
            AppRail { active: active(), onselect: move |area| { active.set(area); persist_preferences(rail_preferences.clone(), UiPreferencePatch { selected_area: Some(area.id().into()), ..Default::default() }); } }
            section { class: "workdeck-frame",
                if let Some(snapshot) = snapshot {
                    div {
                        class: if resizing().is_some() { "workspace-layout is-resizing" } else { "workspace-layout" },
                        style: "{layout_style}",
                        onpointermove: move |event| {
                            if let Some((pane, start_x, start_width)) = resizing() {
                                let delta = event.client_coordinates().x - start_x;
                                match pane {
                                    ResizePane::Navigator => navigator_width.set((start_width + delta).clamp(208.0, 360.0)),
                                    ResizePane::Inspector => inspector_width.set((start_width - delta).clamp(240.0, 720.0)),
                                }
                            }
                        },
                        onpointerup: move |_| {
                            if resizing().is_some() {
                                persist_preferences(resize_preferences.clone(), UiPreferencePatch { navigator_width: Some(navigator_width()), inspector_width: Some(inspector_width()), ..Default::default() });
                                resizing.set(None);
                            }
                        },
                        if area_uses_navigator(active()) && navigator_visible() && snapshot.portfolio.project_count > 0 {
                            Navigator {
                                snapshot: snapshot.clone(),
                                selected_project: selected_project(),
                                onselect_project: move |project| selected_project.set(project),
                            }
                            div {
                                class: "pane-resizer pane-resizer--navigator",
                                role: "separator",
                                tabindex: "0",
                                aria_orientation: "vertical",
                                aria_label: "Resize navigator",
                                aria_valuemin: "208",
                                aria_valuemax: "360",
                                aria_valuenow: "{navigator_width():.0}",
                                title: "Resize navigator · drag, use ←/→, or double-click to reset",
                                onpointerdown: move |event| resizing.set(Some((ResizePane::Navigator, event.client_coordinates().x, navigator_width()))),
                                ondoubleclick: move |_| {
                                    navigator_width.set(256.0);
                                    persist_preferences(navigator_reset_preferences.clone(), UiPreferencePatch { navigator_width: Some(256.0), ..Default::default() });
                                },
                                onkeydown: move |event: KeyboardEvent| {
                                    let next = match event.key() {
                                        Key::ArrowLeft => Some(navigator_width() - 16.0),
                                        Key::ArrowRight => Some(navigator_width() + 16.0),
                                        Key::Home => Some(208.0),
                                        Key::End => Some(360.0),
                                        _ => None,
                                    };
                                    if let Some(next) = next {
                                        event.prevent_default();
                                        let next = next.clamp(208.0, 360.0);
                                        navigator_width.set(next);
                                        persist_preferences(navigator_key_preferences.clone(), UiPreferencePatch { navigator_width: Some(next), ..Default::default() });
                                    }
                                }
                            }
                        }
                        section { class: "workbench", aria_label: "{active().label()}",
                            if snapshot.portfolio.project_count == 0 {
                                OnboardingSurface {
                                    client: client.clone(),
                                    suggested_roots: snapshot.suggested_roots.clone(),
                                    oncomplete: move |_| refresh_generation += 1,
                                }
                            } else { match active() {
                                Area::Inbox => rsx!(InboxSurface { client: client.clone(), snapshot: snapshot.clone(), onchanged: move |_| { task_progress.set(None); refresh_generation += 1; }, onopen: move |target| match target {
                                    ActivityTarget::CommitBranch { worktree_id } => { selected_worktree.set(Some(worktree_id)); active.set(Area::Git); }
                                    ActivityTarget::PullRequest { repository, .. } => { selected_provider_repository.set(Some(repository)); active.set(Area::PullRequests); }
                                } }),
                                Area::Workspaces => rsx!(WorkspacesSurface { client: client.clone(), snapshot: snapshot.clone(), selected_project: selected_project(), onopen_git: move |worktree| { selected_worktree.set(Some(worktree)); active.set(Area::Git); }, onchanged: move |_| refresh_generation += 1 }),
                                Area::Git => rsx!(GitSurface {
                                    key: "{git_surface_key}",
                                    client: client.clone(),
                                    worktree_id: selected_worktree(),
                                }),
                                Area::Search => rsx!(SearchSurface { client: client.clone(), saved_searches: snapshot.preferences.saved_searches.clone(), recent_searches: snapshot.preferences.recent_searches.clone(), onactivate: move |target: SearchActivation| match target.kind {
                                    SearchResultKind::Review | SearchResultKind::File | SearchResultKind::Symbol | SearchResultKind::Markdown => {
                                        let review = target.target.strip_prefix("review:").unwrap_or(&target.id);
                                        selected_review.set(Some(ReviewId(review.to_owned())));
                                        active.set(Area::Changes);
                                    }
                                    SearchResultKind::Worktree => { selected_worktree.set(Some(WorktreeId(target.id))); active.set(Area::Git); }
                                    SearchResultKind::Checkout => {
                                        if let Some(worktree) = target.target.strip_prefix("git:") {
                                            selected_worktree.set(Some(WorktreeId(worktree.to_owned())));
                                            active.set(Area::Git);
                                        } else {
                                            active.set(Area::Workspaces);
                                        }
                                    }
                                    SearchResultKind::PullRequest => active.set(Area::PullRequests),
                                    SearchResultKind::Ci => active.set(Area::Ci),
                                    SearchResultKind::Artifact => active.set(Area::Artifacts),
                                    SearchResultKind::Branch => {
                                        let worktree = target.target.strip_prefix("git:").unwrap_or(&target.id);
                                        selected_worktree.set(Some(WorktreeId(worktree.to_owned())));
                                        active.set(Area::Git);
                                    }
                                    SearchResultKind::Commit => active.set(Area::Git),
                                    _ => active.set(Area::Workspaces),
                                } }),
                                Area::PullRequests => rsx!(PullRequestsSurface {
                                    key: "pr-{active_provider_repository.clone().unwrap_or_default()}",
                                    client: client.clone(),
                                    repositories: provider_repositories.clone(),
                                    repository: active_provider_repository.clone(),
                                    onrepositorychange: move |repository| selected_provider_repository.set(repository),
                                    initial_pulls: snapshot.pull_requests.clone(),
                                    onchanged: move |_| refresh_generation += 1,
                                }),
                                Area::Ci => rsx!(CiSurface { key: "ci-{active_ci_repository.clone().unwrap_or_default()}", client: client.clone(), repositories: provider_repositories.clone(), repository: active_ci_repository.clone(), onrepositorychange: move |repository| selected_provider_repository.set(Some(repository)), initial_runs: snapshot.ci_runs.clone() }),
                                Area::Artifacts => rsx!(ArtifactsSurface { client: client.clone(), initial_artifacts: snapshot.artifacts.clone() }),
                                Area::Changes => {
                                    if let Some(review_id) = selected_review().or_else(|| snapshot.reviews.first().map(|review| review.id.clone())) {
                                        rsx!(ChangesSurface { key: "changes-{review_id}", client: client.clone(), review_id })
                                    } else {
                                        rsx!(section { class: "detail-empty", h2 { "No changes selected" } p { "Select a commit range or open changes from a pull request." } })
                                    }
                                },
                            } }
                        }
                        if area_supports_inspector(active()) && inspector_visible() && snapshot.portfolio.project_count > 0 {
                            div {
                                class: "pane-resizer pane-resizer--inspector",
                                role: "separator",
                                tabindex: "0",
                                aria_orientation: "vertical",
                                aria_label: "Resize inspector",
                                aria_valuemin: "240",
                                aria_valuemax: "720",
                                aria_valuenow: "{inspector_width():.0}",
                                title: "Resize inspector · drag, use ←/→, or double-click to reset",
                                onpointerdown: move |event| resizing.set(Some((ResizePane::Inspector, event.client_coordinates().x, inspector_width()))),
                                ondoubleclick: move |_| {
                                    inspector_width.set(320.0);
                                    persist_preferences(inspector_reset_preferences.clone(), UiPreferencePatch { inspector_width: Some(320.0), ..Default::default() });
                                },
                                onkeydown: move |event: KeyboardEvent| {
                                    let next = match event.key() {
                                        Key::ArrowLeft => Some(inspector_width() + 16.0),
                                        Key::ArrowRight => Some(inspector_width() - 16.0),
                                        Key::Home => Some(240.0),
                                        Key::End => Some(720.0),
                                        _ => None,
                                    };
                                    if let Some(next) = next {
                                        event.prevent_default();
                                        let next = next.clamp(240.0, 720.0);
                                        inspector_width.set(next);
                                        persist_preferences(inspector_key_preferences.clone(), UiPreferencePatch { inspector_width: Some(next), ..Default::default() });
                                    }
                                }
                            }
                            Inspector { active: active(), snapshot: snapshot.clone() }
                        }
                    }
                    StatusBar { snapshot, progress: task_progress(), error: task_error(), oncancel: move |operation: OperationId| { status_client.cancel(operation); task_progress.set(None); } }
                } else if let Some(Err(error)) = bootstrap.read().as_ref() {
                    section { class: "fatal-state",
                        h1 { "Workdeck could not open its catalog" }
                        p { "{error}" }
                        button { class: "button button--primary", r#type: "button", onclick: move |_| refresh_generation += 1, "Try again" }
                    }
                } else {
                    section { class: "shell-loading", aria_label: "Loading Workdeck",
                        div { class: "loading-spinner" }
                        p { "Opening your workspace…" }
                    }
                }
            }
            if command_open() { CommandPalette {
                onselect: move |area| { active.set(area); persist_preferences(command_preferences.clone(), UiPreferencePatch { selected_area: Some(area.id().into()), ..Default::default() }); },
                onclose: move |_| {
                    command_open.set(false);
                    restore_shell_focus(shell_mounted());
                }
            } }
        }
    }
}

/// T3-style progressive disclosure: only the portfolio hierarchy owns a
/// persistent context navigator. Every other destination already contains its
/// own purpose-built master list, tree, or filter surface.
fn area_uses_navigator(area: Area) -> bool {
    area == Area::Workspaces
}

/// The global inspector is an opt-in evidence drawer for dense portfolio and
/// Git views. Changes, PR, CI, and artifact surfaces own their detail panes.
fn area_supports_inspector(area: Area) -> bool {
    area == Area::Workspaces
}

fn restore_shell_focus(mounted: Option<MountedEvent>) {
    if let Some(mounted) = mounted {
        spawn(async move {
            let _ = mounted.set_focus(true).await;
        });
    }
}

fn persist_preferences(client: WorkdeckClient, patch: UiPreferencePatch) {
    spawn(async move {
        let _ = client
            .request(WorkdeckRequest::UpdatePreferences {
                request_id: RequestId::new(),
                patch,
            })
            .await;
    });
}

fn provider_repositories(snapshot: &workdeck_api::BootstrapSnapshot) -> Vec<String> {
    let preferred_projects = snapshot
        .inbox
        .items
        .iter()
        .map(|item| item.project.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let mut repositories = Vec::new();
    for preferred in preferred_projects {
        for repository in snapshot
            .portfolio
            .projects
            .iter()
            .filter(|project| project.name.to_ascii_lowercase() == preferred)
            .flat_map(|project| &project.repositories)
        {
            if let Some(provider) = repository.provider.clone()
                && seen.insert(provider.clone())
            {
                repositories.push(provider);
            }
        }
    }
    for provider in snapshot
        .portfolio
        .projects
        .iter()
        .flat_map(|project| &project.repositories)
        .filter_map(|repository| repository.provider.clone())
    {
        if seen.insert(provider.clone()) {
            repositories.push(provider);
        }
    }
    repositories
}

fn default_available_worktree(snapshot: &workdeck_api::BootstrapSnapshot) -> Option<WorktreeId> {
    snapshot
        .inbox
        .items
        .iter()
        .find_map(|item| match &item.target {
            ActivityTarget::CommitBranch { worktree_id }
                if is_available_worktree(snapshot, worktree_id) =>
            {
                Some(worktree_id.clone())
            }
            _ => None,
        })
        .or_else(|| {
            snapshot
                .portfolio
                .projects
                .iter()
                .flat_map(|project| &project.repositories)
                .flat_map(|repository| &repository.checkouts)
                .flat_map(|checkout| &checkout.worktrees)
                .find(|worktree| worktree.availability == Availability::Available)
                .map(|worktree| worktree.id.clone())
        })
}

fn is_available_worktree(
    snapshot: &workdeck_api::BootstrapSnapshot,
    worktree_id: &WorktreeId,
) -> bool {
    snapshot
        .portfolio
        .projects
        .iter()
        .flat_map(|project| &project.repositories)
        .flat_map(|repository| &repository.checkouts)
        .flat_map(|checkout| &checkout.worktrees)
        .any(|worktree| {
            &worktree.id == worktree_id && worktree.availability == Availability::Available
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_context_prefers_current_unread_activity_and_deduplicates() {
        let snapshot = workdeck_api::fixtures::polished();
        let repositories = provider_repositories(&snapshot);
        assert_eq!(
            repositories.first().map(String::as_str),
            Some("example/sampleapp")
        );
        assert_eq!(
            repositories.iter().collect::<BTreeSet<_>>().len(),
            repositories.len()
        );
    }

    #[test]
    fn startup_prewarm_prefers_an_available_unread_worktree() {
        let snapshot = workdeck_api::fixtures::polished();
        assert_eq!(
            default_available_worktree(&snapshot),
            Some(WorktreeId::from("worktree-sampleapp"))
        );
    }

    #[test]
    fn only_workspaces_owns_the_global_hierarchy_navigator() {
        assert!(area_uses_navigator(Area::Workspaces));
        for area in [
            Area::Inbox,
            Area::Git,
            Area::Search,
            Area::Artifacts,
            Area::Changes,
            Area::PullRequests,
            Area::Ci,
        ] {
            assert!(!area_uses_navigator(area), "{area:?}");
        }
    }

    #[test]
    fn generic_inspector_never_competes_with_task_owned_detail_panes() {
        assert!(area_supports_inspector(Area::Workspaces));
        for area in [
            Area::Inbox,
            Area::Git,
            Area::Search,
            Area::Artifacts,
            Area::Changes,
            Area::PullRequests,
            Area::Ci,
        ] {
            assert!(!area_supports_inspector(area), "{area:?}");
        }
    }
}
