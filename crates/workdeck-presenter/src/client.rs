//! Bounded native implementation of the renderer-independent Workdeck protocol.

use crate::{
    ArtifactPreview, AttentionScanOptions, ReviewSnapshot, ReviewUnitDetail, RuntimeHandle,
    UiPreferences, WorkspaceSnapshot,
};
use futures::{FutureExt, future::BoxFuture};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};
use workdeck_api as api;
use workdeck_domain as domain;

const REQUEST_CAPACITY: usize = 64;
const WORKER_COUNT: usize = 4;

struct LocalCommand {
    request: api::WorkdeckRequest,
    reply: async_channel::Sender<Result<api::Envelope<api::WorkdeckResponse>, api::WorkdeckError>>,
}

/// Native renderer client. Database, repository, provider, and filesystem
/// access stays on the presenter's bounded workers.
pub struct LocalWorkdeckClient;

impl LocalWorkdeckClient {
    pub fn spawn_default() -> anyhow::Result<api::WorkdeckClient> {
        Self::spawn(RuntimeHandle::spawn_default()?)
    }

    pub fn spawn(runtime: RuntimeHandle) -> anyhow::Result<api::WorkdeckClient> {
        let (requests, receiver) = async_channel::bounded::<LocalCommand>(REQUEST_CAPACITY);
        let (events, event_receiver) =
            async_channel::bounded::<api::WorkdeckEvent>(REQUEST_CAPACITY);
        let revision = Arc::new(AtomicU64::new(0));
        let workspace = Arc::new(Mutex::new(None));
        let previews = Arc::new(Mutex::new(BTreeMap::<String, ArtifactPreview>::new()));
        let cancellations = Arc::new(Mutex::new(BTreeMap::<
            String,
            crate::AttentionScanCancellation,
        >::new()));

        for index in 0..WORKER_COUNT {
            let receiver = receiver.clone();
            let runtime = runtime.clone();
            let revision = revision.clone();
            let workspace = workspace.clone();
            let events = events.clone();
            let previews = previews.clone();
            let cancellations = cancellations.clone();
            thread::Builder::new()
                .name(format!("workdeck-api-{index}"))
                .spawn(move || {
                    while let Ok(command) = receiver.recv_blocking() {
                        let request_id = command.request.request_id().clone();
                        let result = handle_request(
                            &runtime,
                            &workspace,
                            &previews,
                            &cancellations,
                            &events,
                            command.request,
                        )
                        .map(|payload| {
                            let revision = api::Revision(
                                revision.fetch_add(1, Ordering::AcqRel).saturating_add(1),
                            );
                            let _ =
                                events.try_send(api::WorkdeckEvent::SnapshotUpdated { revision });
                            api::Envelope {
                                request_id,
                                revision,
                                payload,
                            }
                        });
                        let _ = command.reply.send_blocking(result);
                    }
                })?;
        }

        Ok(api::WorkdeckClient::new(LocalTransport {
            requests,
            events,
            event_receiver,
            previews,
            cancellations,
        }))
    }
}

struct LocalTransport {
    requests: async_channel::Sender<LocalCommand>,
    events: async_channel::Sender<api::WorkdeckEvent>,
    event_receiver: api::WorkdeckEventStream,
    previews: Arc<Mutex<BTreeMap<String, ArtifactPreview>>>,
    cancellations: Arc<Mutex<BTreeMap<String, crate::AttentionScanCancellation>>>,
}

impl api::WorkdeckTransport for LocalTransport {
    fn request(
        &self,
        request: api::WorkdeckRequest,
    ) -> BoxFuture<'static, Result<api::Envelope<api::WorkdeckResponse>, api::WorkdeckError>> {
        let requests = self.requests.clone();
        async move {
            let (reply, response) = async_channel::bounded(1);
            requests
                .send(LocalCommand { request, reply })
                .await
                .map_err(|_| api::WorkdeckError::Internal("native runtime stopped".into()))?;
            response.recv().await.map_err(|_| {
                api::WorkdeckError::Internal("native runtime dropped its response".into())
            })?
        }
        .boxed()
    }

    fn subscribe(&self) -> api::WorkdeckEventStream {
        self.event_receiver.clone()
    }

    fn cancel(&self, operation_id: api::OperationId) {
        if let Ok(mut cancellations) = self.cancellations.lock()
            && let Some(cancellation) = cancellations.remove(&operation_id.0)
        {
            cancellation.cancel();
        }
        if let Ok(mut previews) = self.previews.lock() {
            previews.remove(&operation_id.0);
        }
        let _ = self
            .events
            .try_send(api::WorkdeckEvent::OperationCancelled { operation_id });
    }
}

fn handle_request(
    runtime: &RuntimeHandle,
    cache: &Mutex<Option<WorkspaceSnapshot>>,
    previews: &Mutex<BTreeMap<String, ArtifactPreview>>,
    cancellations: &Mutex<BTreeMap<String, crate::AttentionScanCancellation>>,
    events: &async_channel::Sender<api::WorkdeckEvent>,
    request: api::WorkdeckRequest,
) -> Result<api::WorkdeckResponse, api::WorkdeckError> {
    match request {
        api::WorkdeckRequest::Bootstrap { .. } => {
            let workspace = load_workspace(runtime, cache)?;
            Ok(api::WorkdeckResponse::Bootstrap(map_bootstrap(
                &workspace,
                load_preferences(runtime)?,
            )))
        }
        api::WorkdeckRequest::LoadPortfolio { .. } => Ok(api::WorkdeckResponse::Portfolio(
            map_portfolio(&load_workspace(runtime, cache)?),
        )),
        api::WorkdeckRequest::DiscoverPortfolio { roots, .. } => {
            let roots = roots.into_iter().map(std::path::PathBuf::from).collect();
            receive(runtime.discover_portfolio(roots))?;
            Ok(api::WorkdeckResponse::Portfolio(map_portfolio(
                &load_workspace(runtime, cache)?,
            )))
        }
        api::WorkdeckRequest::ChoosePortfolioRoot { .. } => {
            if let Some(root) = rfd::FileDialog::new()
                .set_title("Add Workdeck portfolio root")
                .pick_folder()
            {
                receive(runtime.discover_portfolio(vec![root]))?;
            }
            Ok(api::WorkdeckResponse::Portfolio(map_portfolio(
                &load_workspace(runtime, cache)?,
            )))
        }
        api::WorkdeckRequest::LoadInbox { .. } => Ok(api::WorkdeckResponse::Inbox(map_inbox(
            &load_workspace(runtime, cache)?,
        ))),
        api::WorkdeckRequest::MarkActivityRead {
            target, revision, ..
        } => {
            let revision = revision.trim().to_owned();
            if revision.is_empty() {
                return Err(api::WorkdeckError::InvalidRequest(
                    "activity revision cannot be empty".into(),
                ));
            }
            let (key, kind) = match &target {
                api::ActivityTarget::CommitBranch { worktree_id } => (
                    format!("commit_branch:{}", worktree_id.0),
                    domain::ActivityKind::CommitBranch,
                ),
                api::ActivityTarget::PullRequest { repository, number } => (
                    format!("pull_request:{repository}#{number}"),
                    domain::ActivityKind::PullRequest,
                ),
            };
            let cursor = domain::ActivityReadCursor {
                key,
                kind,
                revision: revision.clone(),
                read_at: chrono::Utc::now(),
            };
            receive(runtime.save_activity_read_cursor(cursor.clone()))?;
            let _ = load_workspace(runtime, cache)?;
            Ok(api::WorkdeckResponse::ActivityRead(
                api::ActivityReadState {
                    target,
                    revision,
                    unread: false,
                    read_at: cursor.read_at,
                },
            ))
        }
        api::WorkdeckRequest::ScanInbox { operation_id, .. } => {
            let workspace = cached_or_load(runtime, cache)?;
            let scan = runtime.scan_inbox(
                workspace.worktrees.clone(),
                &workspace.worktree_attention,
                AttentionScanOptions::default(),
            );
            cancellations
                .lock()
                .map_err(|_| {
                    api::WorkdeckError::Integrity("scan cancellation lock was poisoned".into())
                })?
                .insert(operation_id.0.clone(), scan.cancellation);
            while let Ok(event) = scan.receiver.recv() {
                match event {
                    crate::InboxScanEvent::Started { total, .. } => {
                        let _ =
                            events.try_send(api::WorkdeckEvent::TaskProgress(api::TaskProgress {
                                operation_id: operation_id.clone(),
                                label: "Scanning commit updates".into(),
                                completed: 0,
                                total,
                                cancellable: true,
                            }));
                    }
                    crate::InboxScanEvent::Progress {
                        completed, total, ..
                    } => {
                        let _ =
                            events.try_send(api::WorkdeckEvent::TaskProgress(api::TaskProgress {
                                operation_id: operation_id.clone(),
                                label: "Scanning commit updates".into(),
                                completed,
                                total,
                                cancellable: true,
                            }));
                    }
                    crate::InboxScanEvent::Finished { cancelled, .. } => {
                        if cancelled {
                            let _ = events.try_send(api::WorkdeckEvent::OperationCancelled {
                                operation_id: operation_id.clone(),
                            });
                        }
                        break;
                    }
                    crate::InboxScanEvent::Failed(message) => {
                        cancellations
                            .lock()
                            .ok()
                            .and_then(|mut values| values.remove(&operation_id.0));
                        return Err(api::WorkdeckError::Internal(message));
                    }
                    crate::InboxScanEvent::Updated(_) => {}
                }
            }
            if let Ok(mut values) = cancellations.lock() {
                values.remove(&operation_id.0);
            }
            Ok(api::WorkdeckResponse::Inbox(map_inbox(&load_workspace(
                runtime, cache,
            )?)))
        }
        api::WorkdeckRequest::Search { query, .. } => Ok(api::WorkdeckResponse::Search(
            map_search(&cached_or_load(runtime, cache)?, &query),
        )),
        api::WorkdeckRequest::LoadGitGraph {
            worktree_id,
            cursor,
            ..
        } => load_git(runtime, cache, worktree_id, cursor).map(api::WorkdeckResponse::GitGraph),
        api::WorkdeckRequest::LoadReview { review_id, .. } => {
            load_review(runtime, domain::ReviewSetId::from(review_id.0))
                .map(api::WorkdeckResponse::Review)
        }
        api::WorkdeckRequest::PrepareWorktreeReview { worktree_id, .. } => {
            let review =
                receive(runtime.prepare_worktree_review(domain::WorktreeId::from(worktree_id.0)))?;
            load_review(runtime, review.id).map(api::WorkdeckResponse::Review)
        }
        api::WorkdeckRequest::PrepareCommitRangeReview {
            worktree_id,
            base,
            head,
            ..
        } => {
            let workspace = cached_or_load(runtime, cache)?;
            let worktree_id = domain::WorktreeId::from(worktree_id.0);
            let worktree = workspace
                .worktrees
                .iter()
                .find(|worktree| worktree.id == worktree_id)
                .ok_or_else(|| {
                    api::WorkdeckError::InvalidRequest("worktree does not exist".into())
                })?;
            let checkout = workspace
                .checkouts
                .iter()
                .find(|checkout| checkout.id == worktree.checkout_id)
                .ok_or_else(|| {
                    api::WorkdeckError::Integrity("worktree checkout is missing".into())
                })?;
            let review = receive(runtime.prepare_commit_range_review(
                checkout.repository_id.clone(),
                base,
                head,
            ))?;
            load_review(runtime, review.id).map(api::WorkdeckResponse::Review)
        }
        api::WorkdeckRequest::PreparePullRequestReview {
            repository,
            number,
            title,
            ..
        } => {
            let workspace = cached_or_load(runtime, cache)?;
            if let Some(existing) = workspace.review_sets.iter().find(|review| {
                review.sources.iter().any(|source| {
                    matches!(source, domain::ReviewSource::PullRequest { repository: attached, number: attached_number, .. } if attached == &repository && attached_number == &number)
                })
            }) {
                return load_review(runtime, existing.review.id.clone())
                    .map(api::WorkdeckResponse::Review);
            }
            let review = receive(runtime.create_review(title))?;
            receive(runtime.attach_source(
                review.id.clone(),
                domain::ReviewSource::PullRequest {
                    provider: "github".into(),
                    repository,
                    number,
                },
            ))?;
            receive(runtime.capture_review(review.id.clone()))?;
            load_review(runtime, review.id).map(api::WorkdeckResponse::Review)
        }
        api::WorkdeckRequest::LoadPullRequests { repository, .. } => {
            let workspace = cached_or_load(runtime, cache)?;
            let repository = resolve_provider_repository(&workspace, repository)?;
            Ok(api::WorkdeckResponse::PullRequests(load_pull_requests(
                &repository,
                &workspace.activity_read_cursors,
            )?))
        }
        api::WorkdeckRequest::LoadCiRuns { repository, .. } => {
            let workspace = cached_or_load(runtime, cache)?;
            let repository = resolve_provider_repository(&workspace, repository)?;
            Ok(api::WorkdeckResponse::CiRuns(load_ci_runs(&repository)?))
        }
        api::WorkdeckRequest::LoadCiJobLog {
            repository,
            job_id,
            job_name,
            ..
        } => {
            let log = receive(runtime.load_github_job_log(
                repository,
                job_id,
                job_name,
                crate::GitHubCancellation::default(),
            ))?;
            Ok(api::WorkdeckResponse::CiJobLog(api::CiJobLog {
                job_id: log.job_id.to_string(),
                job_name: log.job_name,
                lines: log.text.lines().map(str::to_owned).collect(),
                original_bytes: log.original_bytes,
                truncated: log.truncated,
            }))
        }
        api::WorkdeckRequest::LoadArtifacts { .. } => Ok(api::WorkdeckResponse::Artifacts(
            map_artifacts(&cached_or_load(runtime, cache)?),
        )),
        api::WorkdeckRequest::ChooseArtifactImport { .. } => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("ZIP archive", &["zip"])
                .set_title("Import Workdeck artifact")
                .pick_file()
            {
                let name = path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("Artifact")
                    .to_owned();
                receive(runtime.import_artifact(path, name))?;
            }
            Ok(api::WorkdeckResponse::Artifacts(map_artifacts(
                &load_workspace(runtime, cache)?,
            )))
        }
        api::WorkdeckRequest::OpenArtifact { artifact_id, .. } => {
            let preview =
                receive(runtime.preview_artifact(domain::ArtifactId::from(artifact_id.0.clone())))?;
            let session_id = api::OperationId::new();
            let url = preview.url().to_owned();
            previews
                .lock()
                .map_err(|_| {
                    api::WorkdeckError::Integrity("artifact preview lock was poisoned".into())
                })?
                .insert(session_id.0.clone(), preview);
            Ok(api::WorkdeckResponse::ArtifactPreview(
                api::ArtifactPreviewSession {
                    session_id,
                    artifact_id,
                    url,
                },
            ))
        }
        api::WorkdeckRequest::CloseArtifact { session_id, .. } => {
            previews
                .lock()
                .map_err(|_| {
                    api::WorkdeckError::Integrity("artifact preview lock was poisoned".into())
                })?
                .remove(&session_id.0);
            Ok(api::WorkdeckResponse::Artifacts(map_artifacts(
                &cached_or_load(runtime, cache)?,
            )))
        }
        api::WorkdeckRequest::UpdateReviewMark {
            review_id,
            unit_id,
            state,
            ..
        } => {
            let domain_state = match state {
                api::ReviewMarkState::Unreviewed => domain::ReviewMarkState::Seen,
                api::ReviewMarkState::Reviewed => domain::ReviewMarkState::Reviewed,
                api::ReviewMarkState::NeedsAttention => domain::ReviewMarkState::Questioned,
                api::ReviewMarkState::Dismissed => domain::ReviewMarkState::Resolved,
            };
            receive(runtime.mark_unit(
                domain::ReviewUnitVersionId::from(unit_id.0.clone()),
                domain_state,
                "local",
            ))?;
            let review = receive(
                runtime.load_review_snapshot(domain::ReviewSetId::from(review_id.0.clone())),
            )?;
            Ok(api::WorkdeckResponse::ReviewMark(api::ReviewMark {
                review_id,
                unit_id,
                state,
                revision: api::Revision(review.attention.revision),
            }))
        }
        api::WorkdeckRequest::CreateCheckpoint { review_id, .. } => {
            let checkpoint =
                receive(runtime.capture_review(domain::ReviewSetId::from(review_id.0)))?;
            let review = receive(runtime.load_review_snapshot(checkpoint.review_set_id.clone()))?;
            Ok(api::WorkdeckResponse::Checkpoint(map_checkpoint(
                &review.attention,
                Some(&checkpoint),
            )))
        }
        api::WorkdeckRequest::UpdatePreferences { patch, .. } => {
            let mut preferences = load_preferences(runtime)?;
            preferences.apply(patch);
            save_preferences(runtime, &preferences)?;
            Ok(api::WorkdeckResponse::Preferences(preferences))
        }
    }
}

fn receive<T>(
    receiver: std::sync::mpsc::Receiver<anyhow::Result<T>>,
) -> Result<T, api::WorkdeckError> {
    receiver
        .recv()
        .map_err(|_| api::WorkdeckError::Internal("native runtime disconnected".into()))?
        .map_err(map_error)
}

fn map_error(error: anyhow::Error) -> api::WorkdeckError {
    let message = format!("{error:#}");
    if message.to_ascii_lowercase().contains("unavailable") {
        api::WorkdeckError::RepositoryUnavailable(message)
    } else {
        api::WorkdeckError::Internal(message)
    }
}

fn load_workspace(
    runtime: &RuntimeHandle,
    cache: &Mutex<Option<WorkspaceSnapshot>>,
) -> Result<WorkspaceSnapshot, api::WorkdeckError> {
    let workspace = receive(runtime.load_workspace())?;
    *cache
        .lock()
        .map_err(|_| api::WorkdeckError::Integrity("workspace cache lock was poisoned".into()))? =
        Some(workspace.clone());
    Ok(workspace)
}

fn cached_or_load(
    runtime: &RuntimeHandle,
    cache: &Mutex<Option<WorkspaceSnapshot>>,
) -> Result<WorkspaceSnapshot, api::WorkdeckError> {
    if let Some(workspace) = cache
        .lock()
        .map_err(|_| api::WorkdeckError::Integrity("workspace cache lock was poisoned".into()))?
        .clone()
    {
        Ok(workspace)
    } else {
        load_workspace(runtime, cache)
    }
}

fn load_preferences(runtime: &RuntimeHandle) -> Result<api::UiPreferences, api::WorkdeckError> {
    let stored = receive(runtime.load_ui_preferences())?;
    if let Some(preferences) = stored
        .viewport_state
        .and_then(|value| serde_json::from_value(value).ok())
    {
        return Ok(preferences);
    }
    Ok(api::UiPreferences {
        navigator_visible: !stored.sidebar_collapsed,
        inspector_visible: false,
        navigator_width: 256.0,
        inspector_width: 320.0,
        ..api::UiPreferences::default()
    })
}

fn save_preferences(
    runtime: &RuntimeHandle,
    preferences: &api::UiPreferences,
) -> Result<(), api::WorkdeckError> {
    let mut stored: UiPreferences = receive(runtime.load_ui_preferences())?;
    stored.sidebar_collapsed = !preferences.navigator_visible;
    stored.viewport_state = Some(
        serde_json::to_value(preferences)
            .map_err(|error| api::WorkdeckError::Internal(error.to_string()))?,
    );
    receive(runtime.save_ui_preferences(stored))
}

fn map_bootstrap(
    workspace: &WorkspaceSnapshot,
    preferences: api::UiPreferences,
) -> api::BootstrapSnapshot {
    api::BootstrapSnapshot {
        portfolio: map_portfolio(workspace),
        suggested_roots: suggested_portfolio_roots(),
        inbox: map_inbox(workspace),
        reviews: workspace
            .review_sets
            .iter()
            .map(map_review_summary)
            .collect(),
        pull_requests: Vec::new(),
        ci_runs: Vec::new(),
        artifacts: map_artifacts(workspace),
        preferences,
        provider_state: api::ProviderState::Ready,
    }
}

fn suggested_portfolio_roots() -> Vec<String> {
    let Some(user_dirs) = directories::UserDirs::new() else {
        return Vec::new();
    };

    ["Projects", "Sites"]
        .map(|name| user_dirs.home_dir().join(name))
        .into_iter()
        .filter(|root| root.is_dir())
        .filter_map(|root| root.to_str().map(str::to_owned))
        .collect()
}

fn map_portfolio(workspace: &WorkspaceSnapshot) -> api::PortfolioSnapshot {
    let projects = workspace
        .projects
        .iter()
        .map(|project| {
            let repositories = workspace
                .repositories
                .iter()
                .filter(|repository| repository.project_id == project.id)
                .map(|repository| {
                    let checkouts = workspace
                        .checkouts
                        .iter()
                        .filter(|checkout| checkout.repository_id == repository.id)
                        .map(|checkout| {
                            let worktrees = workspace
                                .worktrees
                                .iter()
                                .filter(|worktree| worktree.checkout_id == checkout.id)
                                .map(|worktree| map_worktree(workspace, worktree))
                                .collect::<Vec<_>>();
                            api::CheckoutNode {
                                id: api::CheckoutId(checkout.id.to_string()),
                                label: path_label(&checkout.path),
                                available: workspace.checkout_availability(checkout).is_openable(),
                                worktrees,
                            }
                        })
                        .collect::<Vec<_>>();
                    let attention = checkouts
                        .iter()
                        .flat_map(|checkout| &checkout.worktrees)
                        .filter(|worktree| worktree.changes > 0 || worktree.commits > 0)
                        .count();
                    api::RepositoryNode {
                        id: api::RepositoryId(repository.id.to_string()),
                        name: repository.name.clone(),
                        provider: repository
                            .provider_owner
                            .as_ref()
                            .zip(repository.provider_name.as_ref())
                            .map(|(owner, name)| format!("{owner}/{name}")),
                        checkouts,
                        attention,
                    }
                })
                .collect::<Vec<_>>();
            let attention = repositories
                .iter()
                .map(|repository| repository.attention)
                .sum();
            api::ProjectNode {
                id: api::ProjectId(project.id.to_string()),
                name: project.name.clone(),
                empty: repositories.is_empty(),
                repositories,
                attention,
            }
        })
        .collect::<Vec<_>>();
    api::PortfolioSnapshot {
        project_count: projects.len(),
        repository_count: workspace.repositories.len(),
        worktree_count: workspace.worktrees.len(),
        unavailable_count: workspace
            .worktrees
            .iter()
            .filter(|worktree| !workspace.worktree_is_openable(worktree))
            .count(),
        projects,
        scanned_at: workspace.loaded_at,
    }
}

fn map_worktree(
    workspace: &WorkspaceSnapshot,
    worktree: &domain::WorktreeRecord,
) -> api::WorktreeNode {
    let attention = workspace
        .worktree_attention
        .iter()
        .find(|attention| attention.worktree_id == worktree.id);
    let availability = if worktree.prunable {
        api::Availability::Prunable
    } else {
        match workspace.worktree_availability(worktree) {
            crate::WorktreeAvailability::Available => api::Availability::Available,
            crate::WorktreeAvailability::CatalogUnavailable
            | crate::WorktreeAvailability::ObservedUnavailable => api::Availability::Unavailable,
            crate::WorktreeAvailability::ScanWarning => api::Availability::ScanWarning,
        }
    };
    api::WorktreeNode {
        id: api::WorktreeId(worktree.id.to_string()),
        label: path_label(&worktree.path),
        branch: worktree.branch.clone(),
        path_hint: path_hint(&worktree.path),
        availability,
        changes: attention.map_or(0, |attention| attention.change_count),
        commits: attention.map_or(0, |attention| attention.commit_count),
        last_seen: worktree.last_seen_at,
    }
}

fn map_inbox(workspace: &WorkspaceSnapshot) -> api::InboxSnapshot {
    let mut items = Vec::new();
    for attention in workspace
        .worktree_attention
        .iter()
        .filter(|attention| attention.commit_count > 0 && attention.error.is_none())
    {
        let Some(worktree) = workspace
            .worktrees
            .iter()
            .find(|worktree| worktree.id == attention.worktree_id)
        else {
            continue;
        };
        let Some(checkout) = workspace
            .checkouts
            .iter()
            .find(|checkout| checkout.id == worktree.checkout_id)
        else {
            continue;
        };
        let Some(repository) = workspace
            .repositories
            .iter()
            .find(|repository| repository.id == checkout.repository_id)
        else {
            continue;
        };
        let Some(project) = workspace
            .projects
            .iter()
            .find(|project| project.id == repository.project_id)
        else {
            continue;
        };
        let revision = if attention.fingerprint.is_empty() {
            worktree
                .head
                .clone()
                .unwrap_or_else(|| attention.scanned_at.to_rfc3339())
        } else {
            attention.fingerprint.clone()
        };
        let key = format!("commit_branch:{}", worktree.id);
        let unread = !workspace
            .activity_read_cursors
            .iter()
            .any(|cursor| cursor.matches(&key, &revision));
        let branch = worktree
            .branch
            .clone()
            .unwrap_or_else(|| "detached HEAD".into());
        items.push(api::InboxItem {
            id: key,
            target: api::ActivityTarget::CommitBranch {
                worktree_id: api::WorktreeId(worktree.id.to_string()),
            },
            kind: api::ActivityKind::CommitBranch,
            revision,
            unread,
            project: project.name.clone(),
            repository: repository.name.clone(),
            title: format!(
                "{} new {} on {branch}",
                attention.commit_count,
                if attention.commit_count == 1 {
                    "commit"
                } else {
                    "commits"
                }
            ),
            branch,
            summary: if attention.change_count == 0 {
                "Branch advanced since it was last read".into()
            } else {
                format!(
                    "{} changed files in the current branch",
                    attention.change_count
                )
            },
            changes: attention.change_count,
            commits: attention.commit_count,
            updated_at: attention.scanned_at,
        });
    }
    items.sort_by_key(|item| {
        (
            std::cmp::Reverse(item.unread),
            std::cmp::Reverse(item.updated_at),
        )
    });
    api::InboxSnapshot {
        unread: items.iter().filter(|item| item.unread).count(),
        commit_updates: items.len(),
        pull_request_updates: 0,
        items,
        scan: None,
        updated_at: workspace.loaded_at,
    }
}

fn map_review_summary(attention: &crate::ReviewAttention) -> api::ReviewSummary {
    api::ReviewSummary {
        id: api::ReviewId(attention.review.id.to_string()),
        title: attention.review.title.clone(),
        project: "Portfolio".into(),
        repository: repository_label(&attention.sources),
        branch: branch_label(&attention.sources),
        revision: attention.revision,
        reviewed: attention.reviewed,
        total: attention.total,
        incoming: attention.changed,
        updated_at: attention.review.updated_at,
    }
}

fn map_checkpoint(
    attention: &crate::ReviewAttention,
    checkpoint: Option<&domain::ReviewCheckpoint>,
) -> api::ReviewCheckpoint {
    let source_revision = checkpoint
        .and_then(|checkpoint| checkpoint.sources.first())
        .map(|source| source.revision.clone())
        .or_else(|| {
            attention
                .source_advances
                .first()
                .map(|advance| advance.from.clone())
        })
        .unwrap_or_else(|| "uncaptured".into());
    api::ReviewCheckpoint {
        revision: checkpoint.map_or(attention.revision, |checkpoint| checkpoint.sequence),
        frozen_at: checkpoint.map_or(attention.review.updated_at, |checkpoint| {
            checkpoint.created_at
        }),
        reviewed: attention.reviewed,
        total: attention.total,
        source_revision,
    }
}

fn load_review(
    runtime: &RuntimeHandle,
    review_set_id: domain::ReviewSetId,
) -> Result<api::ReviewSet, api::WorkdeckError> {
    let snapshot = receive(runtime.load_review_snapshot(review_set_id))?;
    let mut units = Vec::with_capacity(snapshot.units.len());
    for summary in &snapshot.units {
        units.push(map_review_unit(receive(
            runtime.load_unit(summary.version.id.clone()),
        )?));
    }
    let commits = load_review_commits(runtime, &snapshot);
    let plan = units
        .iter()
        .find(|unit| unit.kind == api::ReviewUnitKind::MarkdownSection)
        .map(|unit| api::MarkdownDocument {
            title: unit.title.clone(),
            path: unit.path.clone(),
            sections: vec![api::MarkdownSection {
                id: unit.id.0.clone(),
                heading: unit.title.clone(),
                level: 1,
                body: unit
                    .source
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                reviewed: unit.mark == api::ReviewMarkState::Reviewed,
            }],
        });
    Ok(api::ReviewSet {
        summary: map_review_summary(&snapshot.attention),
        checkpoint: map_checkpoint(&snapshot.attention, snapshot.selected_checkpoint.as_ref()),
        units,
        commits,
        plan,
    })
}

fn map_review_unit(detail: ReviewUnitDetail) -> api::ReviewUnit {
    let language = format!("{:?}", detail.analysis.language).to_ascii_lowercase();
    let path = detail.summary.path();
    let transition = map_transition(detail.summary.transition());
    let content = detail.content;
    let previous = detail.previous_content.unwrap_or_default();
    let current_spans = syntax_spans_by_line(Path::new(&path), &content);
    let previous_spans = syntax_spans_by_line(Path::new(&path), &previous);
    let source = content
        .lines()
        .enumerate()
        .map(|(index, text)| api::SourceLine {
            number: index + 1,
            text: text.to_owned(),
            spans: current_spans.get(index).cloned().unwrap_or_default(),
        })
        .collect();
    let diff = line_diff(&previous, &content, &previous_spans, &current_spans);
    let calls = detail
        .analysis
        .calls
        .iter()
        .enumerate()
        .map(|(index, edge)| api::CallNode {
            id: format!("call-{index}"),
            label: edge.callee.clone(),
            detail: format!("{} · line {}", edge.caller, edge.line),
            depth: 0,
            direction: api::CallDirection::Callee,
        })
        .collect();
    let mut ast = Vec::new();
    if let Some(root) = detail.analysis.canonical_tree.as_ref() {
        flatten_ast(root, 0, &mut ast);
    }
    api::ReviewUnit {
        id: api::ReviewUnitId(detail.summary.version.id.to_string()),
        path,
        title: detail.summary.version.title,
        language,
        kind: map_unit_kind(detail.summary.version.kind),
        mark: detail
            .summary
            .review
            .as_ref()
            .map_or(api::ReviewMarkState::Unreviewed, |mark| match mark.state {
                domain::ReviewMarkState::Questioned => api::ReviewMarkState::NeedsAttention,
                state if state.closes_review() => api::ReviewMarkState::Reviewed,
                _ => api::ReviewMarkState::Unreviewed,
            }),
        transition,
        additions: diff
            .iter()
            .filter(|line| line.kind == api::DiffLineKind::Added)
            .count(),
        deletions: diff
            .iter()
            .filter(|line| line.kind == api::DiffLineKind::Removed)
            .count(),
        diff,
        source,
        calls,
        ast,
    }
}

fn line_diff(
    previous: &str,
    current: &str,
    previous_spans: &[Vec<api::SyntaxSpan>],
    current_spans: &[Vec<api::SyntaxSpan>],
) -> Vec<api::DiffLine> {
    let diff = similar::TextDiff::from_lines(previous, current);
    let mut old_number = 0;
    let mut new_number = 0;
    diff.iter_all_changes()
        .map(|change| {
            let (old, new, kind) = match change.tag() {
                similar::ChangeTag::Delete => {
                    old_number += 1;
                    (Some(old_number), None, api::DiffLineKind::Removed)
                }
                similar::ChangeTag::Insert => {
                    new_number += 1;
                    (None, Some(new_number), api::DiffLineKind::Added)
                }
                similar::ChangeTag::Equal => {
                    old_number += 1;
                    new_number += 1;
                    (
                        Some(old_number),
                        Some(new_number),
                        api::DiffLineKind::Context,
                    )
                }
            };
            api::DiffLine {
                old_number: old,
                new_number: new,
                kind,
                text: change.value().trim_end_matches('\n').to_owned(),
                spans: match kind {
                    api::DiffLineKind::Removed => old
                        .and_then(|line| previous_spans.get(line.saturating_sub(1)))
                        .cloned()
                        .unwrap_or_default(),
                    _ => new
                        .and_then(|line| current_spans.get(line.saturating_sub(1)))
                        .cloned()
                        .unwrap_or_default(),
                },
            }
        })
        .collect()
}

fn syntax_spans_by_line(path: &Path, source: &str) -> Vec<Vec<api::SyntaxSpan>> {
    let mut lines = vec![Vec::new(); source.lines().count()];
    for span in workdeck_analysis::highlight(path, source).unwrap_or_default() {
        if span.start_line != span.end_line {
            continue;
        }
        if let Some(line) = lines.get_mut(span.start_line) {
            line.push(api::SyntaxSpan {
                start: span.start_column,
                end: span.end_column,
                token: span.token,
            });
        }
    }
    for line in &mut lines {
        line.sort_by_key(|span| (span.start, span.end));
    }
    lines
}

fn flatten_ast(
    node: &workdeck_analysis::CanonicalNode,
    depth: usize,
    output: &mut Vec<api::AstNode>,
) {
    let index = output.len();
    output.push(api::AstNode {
        id: format!("ast-{index}"),
        kind: node.kind.clone(),
        label: node.field.clone().unwrap_or_else(|| node.kind.clone()),
        depth,
        line: node.start_line as usize + 1,
    });
    for child in &node.children {
        flatten_ast(child, depth + 1, output);
    }
}

fn map_unit_kind(kind: domain::ReviewUnitKind) -> api::ReviewUnitKind {
    match kind {
        domain::ReviewUnitKind::MarkdownSection | domain::ReviewUnitKind::PlanClaim => {
            api::ReviewUnitKind::MarkdownSection
        }
        domain::ReviewUnitKind::CiStep | domain::ReviewUnitKind::LogAnnotation => {
            api::ReviewUnitKind::CiRun
        }
        domain::ReviewUnitKind::ArtifactRegion => api::ReviewUnitKind::Artifact,
        domain::ReviewUnitKind::Symbol
        | domain::ReviewUnitKind::Ast
        | domain::ReviewUnitKind::Contract => api::ReviewUnitKind::Symbol,
        domain::ReviewUnitKind::File | domain::ReviewUnitKind::Hunk => api::ReviewUnitKind::File,
    }
}

fn map_transition(transition: domain::UnitTransition) -> api::UnitTransition {
    match transition {
        domain::UnitTransition::New => api::UnitTransition::New,
        domain::UnitTransition::Unchanged => api::UnitTransition::Unchanged,
        domain::UnitTransition::Moved | domain::UnitTransition::Rebased => {
            api::UnitTransition::Moved
        }
        domain::UnitTransition::FormatOnly => api::UnitTransition::FormattingOnly,
        domain::UnitTransition::Removed => api::UnitTransition::Removed,
        domain::UnitTransition::Modified
        | domain::UnitTransition::DependencyImpact
        | domain::UnitTransition::Ambiguous => api::UnitTransition::Changed,
    }
}

fn load_review_commits(
    runtime: &RuntimeHandle,
    snapshot: &ReviewSnapshot,
) -> Vec<api::CommitSummary> {
    let Some(worktree_id) = snapshot.sources.iter().find_map(|source| match source {
        domain::ReviewSource::LocalWorktree { worktree_id, .. } => Some(worktree_id.clone()),
        _ => None,
    }) else {
        return Vec::new();
    };
    receive(runtime.load_git_workspace(worktree_id, 100))
        .map(|workspace| {
            workspace
                .graph
                .rows
                .into_iter()
                .map(|row| api::CommitSummary {
                    oid: row.commit.oid,
                    subject: row.commit.summary,
                    author: row.commit.author_name,
                    timestamp: row.commit.authored_at,
                    additions: 0,
                    deletions: 0,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn load_git(
    runtime: &RuntimeHandle,
    cache: &Mutex<Option<WorkspaceSnapshot>>,
    worktree_id: Option<api::WorktreeId>,
    cursor: Option<String>,
) -> Result<api::GitGraph, api::WorkdeckError> {
    let workspace = cached_or_load(runtime, cache)?;
    let worktree_id = worktree_id
        .map(|id| domain::WorktreeId::from(id.0))
        .or_else(|| {
            workspace
                .worktrees
                .iter()
                .find(|worktree| workspace.worktree_is_openable(worktree))
                .map(|worktree| worktree.id.clone())
        })
        .ok_or_else(|| api::WorkdeckError::RepositoryUnavailable("no worktree selected".into()))?;
    let offset = cursor
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let graph = receive(
        runtime.load_git_workspace(worktree_id.clone(), offset.saturating_add(300).min(2_000)),
    )?;
    Ok(map_git_graph(graph.graph, worktree_id, offset))
}

fn map_git_graph(
    graph: workdeck_git::GitGraphSnapshot,
    worktree_id: domain::WorktreeId,
    offset: usize,
) -> api::GitGraph {
    let status = graph.status.clone();
    let total = graph.rows.len();
    let mut rows = graph
        .rows
        .into_iter()
        .skip(offset)
        .map(|row| {
            let references = row
                .commit
                .references
                .iter()
                .map(|reference| reference.name.clone())
                .collect();
            api::GitGraphRow {
                short_oid: row.commit.oid.chars().take(8).collect(),
                oid: row.commit.oid,
                subject: row.commit.summary,
                author: row.commit.author_name,
                timestamp: row.commit.authored_at,
                lane: row.lane,
                lanes_before: row.lanes_before,
                lanes_after: row.lanes_after,
                edges: row
                    .edges
                    .into_iter()
                    .map(|edge| api::GitGraphEdge {
                        from_lane: edge.from_lane,
                        to_lane: edge.to_lane,
                        parent_oid: edge.parent_oid,
                    })
                    .collect(),
                references,
                head: row
                    .commit
                    .references
                    .iter()
                    .any(|reference| reference.checked_out),
                wip: false,
                additions: 0,
                deletions: 0,
                files_changed: 0,
            }
        })
        .collect::<Vec<_>>();
    if offset == 0 && status.is_dirty() {
        let lane = rows.first().map_or(0, |row| row.lane);
        let parent_oid = rows.first().map(|row| row.oid.clone());
        if let Some(first) = rows.first_mut()
            && !first.lanes_before.contains(&lane)
        {
            first.lanes_before.push(lane);
            first.lanes_before.sort_unstable();
        }
        rows.insert(
            0,
            api::GitGraphRow {
                oid: format!("wip:{}", worktree_id.as_str()),
                short_oid: "WIP".into(),
                subject: "Working tree changes".into(),
                author: "Local changes".into(),
                timestamp: rows
                    .first()
                    .map_or_else(chrono::Utc::now, |row| row.timestamp),
                lane,
                lanes_before: Vec::new(),
                lanes_after: vec![lane],
                edges: parent_oid
                    .into_iter()
                    .map(|parent_oid| api::GitGraphEdge {
                        from_lane: lane,
                        to_lane: lane,
                        parent_oid,
                    })
                    .collect(),
                references: vec!["Working tree".into()],
                head: false,
                wip: true,
                additions: 0,
                deletions: 0,
                files_changed: status.total(),
            },
        );
    }
    let references = graph
        .references
        .into_iter()
        .map(|reference| api::GitReference {
            name: reference.name,
            kind: match reference.kind {
                workdeck_git::GitReferenceKind::LocalBranch => api::GitReferenceKind::LocalBranch,
                workdeck_git::GitReferenceKind::RemoteBranch => api::GitReferenceKind::RemoteBranch,
                workdeck_git::GitReferenceKind::Tag => api::GitReferenceKind::Tag,
                workdeck_git::GitReferenceKind::Stash => api::GitReferenceKind::Stash,
                workdeck_git::GitReferenceKind::Head => api::GitReferenceKind::LocalBranch,
            },
            target: reference.target,
            current: reference.checked_out,
        })
        .collect();
    let has_more = graph.truncated || total > offset.saturating_add(300);
    api::GitGraph {
        worktree_id: Some(api::WorktreeId(worktree_id.to_string())),
        rows,
        references,
        has_more,
        next_cursor: has_more.then(|| offset.saturating_add(300).to_string()),
    }
}

fn resolve_provider_repository(
    workspace: &WorkspaceSnapshot,
    requested: Option<String>,
) -> Result<String, api::WorkdeckError> {
    if let Some(repository) = requested.filter(|value| !value.trim().is_empty()) {
        return Ok(repository);
    }
    workspace
        .repositories
        .iter()
        .find_map(|repository| {
            Some(format!(
                "{}/{}",
                repository.provider_owner.as_ref()?,
                repository.provider_name.as_ref()?
            ))
        })
        .ok_or_else(|| {
            api::WorkdeckError::ProviderUnavailable("no GitHub repository selected".into())
        })
}

fn load_pull_requests(
    repository: &str,
    read_cursors: &[domain::ActivityReadCursor],
) -> Result<Vec<api::PullRequest>, api::WorkdeckError> {
    workdeck_github::GitHubClient::default()
        .pull_requests(repository)
        .map_err(map_error)
        .map(|pulls| {
            pulls
                .into_iter()
                .take(100)
                .map(|pull| {
                    let activity_revision = pull.updated_at.to_rfc3339();
                    let key = format!("pull_request:{repository}#{}", pull.number);
                    let unread = !read_cursors
                        .iter()
                        .any(|cursor| cursor.matches(&key, &activity_revision));
                    api::PullRequest {
                        number: pull.number,
                        title: pull.title,
                        repository: repository.into(),
                        author: pull.author,
                        branch: pull.head_ref,
                        base: pull.base_ref,
                        state: if pull.draft {
                            api::PullRequestState::Draft
                        } else {
                            match pull.state.as_str() {
                                "closed" => api::PullRequestState::Closed,
                                "merged" => api::PullRequestState::Merged,
                                _ => api::PullRequestState::Open,
                            }
                        },
                        checks: api::CheckSummary {
                            passed: 0,
                            failed: 0,
                            pending: 0,
                        },
                        additions: 0,
                        deletions: 0,
                        files: 0,
                        comments: 0,
                        commits: 0,
                        activity_revision,
                        unread,
                        updated_at: pull.updated_at,
                        url: pull.url,
                    }
                })
                .collect()
        })
}

fn load_ci_runs(repository: &str) -> Result<Vec<api::CiRun>, api::WorkdeckError> {
    let client = workdeck_github::GitHubClient::default();
    let runs = client.workflow_runs(repository).map_err(map_error)?;
    Ok(runs
        .into_iter()
        .take(50)
        .enumerate()
        .map(|(index, run)| {
            let detail = (index == 0)
                .then(|| client.workflow_run(repository, &run.id.to_string()).ok())
                .flatten();
            let jobs = detail
                .as_ref()
                .map(|detail| {
                    detail
                        .jobs
                        .iter()
                        .map(|job| api::CiJob {
                            id: job.id.to_string(),
                            name: job.name.clone(),
                            status: map_ci_status(&job.status, job.conclusion.as_deref()),
                            steps: job
                                .steps
                                .iter()
                                .map(|step| api::CiStep {
                                    name: step.name.clone(),
                                    status: map_ci_status(&step.status, step.conclusion.as_deref()),
                                    duration_seconds: None,
                                })
                                .collect(),
                            log_lines: Vec::new(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            api::CiRun {
                id: run.id.to_string(),
                name: run.name,
                repository: repository.into(),
                branch: run.head_branch.unwrap_or_else(|| "detached".into()),
                commit: run.head_sha.chars().take(8).collect(),
                status: map_ci_status(&run.status, run.conclusion.as_deref()),
                jobs,
                started_at: run.run_started_at,
                duration_seconds: None,
                url: run.url,
            }
        })
        .collect())
}

fn map_ci_status(status: &str, conclusion: Option<&str>) -> api::CiStatus {
    match conclusion.unwrap_or(status) {
        "success" | "completed" => api::CiStatus::Passed,
        "failure" | "timed_out" | "action_required" | "startup_failure" => api::CiStatus::Failed,
        "cancelled" => api::CiStatus::Cancelled,
        "skipped" | "neutral" => api::CiStatus::Skipped,
        "queued" | "waiting" | "requested" | "pending" => api::CiStatus::Queued,
        _ => api::CiStatus::Running,
    }
}

fn map_artifacts(workspace: &WorkspaceSnapshot) -> Vec<api::ArtifactRecord> {
    workspace
        .artifacts
        .iter()
        .map(|artifact| api::ArtifactRecord {
            id: api::ArtifactId(artifact.id.to_string()),
            name: artifact.name.clone(),
            kind: if artifact.entrypoint.is_some() {
                api::ArtifactKind::Html
            } else {
                api::ArtifactKind::Zip
            },
            source: artifact.source.clone(),
            size_bytes: artifact.total_bytes,
            entry_count: artifact.file_count,
            imported_at: artifact.imported_at,
            preview_available: artifact.entrypoint.is_some(),
        })
        .collect()
}

fn map_search(workspace: &WorkspaceSnapshot, raw_query: &str) -> api::SearchSnapshot {
    let query = raw_query.trim();
    let needle = query.to_ascii_lowercase();
    if needle.is_empty() {
        return api::SearchSnapshot {
            query: query.into(),
            groups: Vec::new(),
            total: 0,
            truncated: false,
        };
    }
    let mut grouped =
        BTreeMap::<&'static str, (api::SearchResultKind, Vec<api::SearchResult>)>::new();
    let mut push = |label: &'static str, kind, result: api::SearchResult| {
        grouped
            .entry(label)
            .or_insert_with(|| (kind, Vec::new()))
            .1
            .push(result);
    };
    for project in &workspace.projects {
        if matches_query(
            &needle,
            [project.name.as_str(), project.description.as_str()],
        ) {
            push(
                "Projects",
                api::SearchResultKind::Project,
                api::SearchResult {
                    id: project.id.to_string(),
                    title: project.name.clone(),
                    subtitle: project.description.clone(),
                    metadata: "Project".into(),
                    target: "workspaces".into(),
                },
            );
        }
    }
    for repository in &workspace.repositories {
        if matches_query(
            &needle,
            [
                &repository.name,
                repository.provider.as_deref().unwrap_or(""),
            ],
        ) {
            push(
                "Repositories",
                api::SearchResultKind::Repository,
                api::SearchResult {
                    id: repository.id.to_string(),
                    title: repository.name.clone(),
                    subtitle: repository
                        .provider
                        .clone()
                        .unwrap_or_else(|| "Local Git".into()),
                    metadata: "Repository".into(),
                    target: "workspaces".into(),
                },
            );
        }
    }
    for checkout in &workspace.checkouts {
        let path = checkout.path.to_string_lossy();
        if matches_query(&needle, [path.as_ref()]) {
            let target = workspace
                .worktrees
                .iter()
                .find(|worktree| worktree.checkout_id == checkout.id)
                .filter(|worktree| workspace.worktree_is_openable(worktree))
                .map(|worktree| format!("git:{}", worktree.id))
                .unwrap_or_else(|| "workspaces".into());
            push(
                "Checkouts",
                api::SearchResultKind::Checkout,
                api::SearchResult {
                    id: checkout.id.to_string(),
                    title: path_label(&checkout.path),
                    subtitle: path_hint(&checkout.path),
                    metadata: if checkout.available {
                        "Available"
                    } else {
                        "Unavailable"
                    }
                    .into(),
                    target,
                },
            );
        }
    }
    for worktree in &workspace.worktrees {
        let path = worktree.path.to_string_lossy();
        if matches_query(
            &needle,
            [path.as_ref(), worktree.branch.as_deref().unwrap_or("")],
        ) {
            push(
                "Worktrees",
                api::SearchResultKind::Worktree,
                api::SearchResult {
                    id: worktree.id.to_string(),
                    title: path_label(&worktree.path),
                    subtitle: worktree
                        .branch
                        .clone()
                        .unwrap_or_else(|| "detached HEAD".into()),
                    metadata: path_hint(&worktree.path),
                    target: "git".into(),
                },
            );
        }
        if let Some(branch) = worktree.branch.as_deref()
            && branch.to_ascii_lowercase().contains(&needle)
        {
            push(
                "Branches",
                api::SearchResultKind::Branch,
                api::SearchResult {
                    id: worktree.id.to_string(),
                    title: branch.into(),
                    subtitle: path_hint(&worktree.path),
                    metadata: "Local branch".into(),
                    target: format!("git:{}", worktree.id),
                },
            );
        }
    }
    for review in &workspace.review_sets {
        if matches_query(&needle, [review.review.title.as_str()]) {
            push(
                "Reviews",
                api::SearchResultKind::Review,
                api::SearchResult {
                    id: review.review.id.to_string(),
                    title: review.review.title.clone(),
                    subtitle: format!("{} of {} reviewed", review.reviewed, review.total),
                    metadata: format!("{} incoming", review.changed),
                    target: "review".into(),
                },
            );
        }
    }
    for unit in &workspace.search_units {
        if matches_query(
            &needle,
            [
                unit.title.as_str(),
                unit.path.as_str(),
                unit.provenance.as_str(),
            ],
        ) {
            let kind = if unit.kind == domain::ReviewUnitKind::MarkdownSection {
                api::SearchResultKind::Markdown
            } else {
                api::SearchResultKind::File
            };
            push(
                "Content",
                kind,
                api::SearchResult {
                    id: unit.version_id.to_string(),
                    title: unit.title.clone(),
                    subtitle: unit.path.clone(),
                    metadata: unit.review_title.clone(),
                    target: format!("review:{}", unit.review_set_id),
                },
            );
        }
    }
    for artifact in &workspace.artifacts {
        if matches_query(&needle, [artifact.name.as_str(), artifact.source.as_str()]) {
            push(
                "Artifacts",
                api::SearchResultKind::Artifact,
                api::SearchResult {
                    id: artifact.id.to_string(),
                    title: artifact.name.clone(),
                    subtitle: artifact.source.clone(),
                    metadata: format!("{} files", artifact.file_count),
                    target: "artifacts".into(),
                },
            );
        }
    }
    let mut groups = grouped
        .into_iter()
        .map(|(label, (kind, mut results))| {
            results.truncate(20);
            api::SearchResultGroup {
                kind,
                label: label.into(),
                results,
            }
        })
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| search_order(group.kind));
    let total = groups.iter().map(|group| group.results.len()).sum();
    api::SearchSnapshot {
        query: query.into(),
        groups,
        total,
        truncated: total >= 100,
    }
}

fn matches_query<'a>(needle: &str, fields: impl IntoIterator<Item = &'a str>) -> bool {
    fields
        .into_iter()
        .any(|field| field.to_ascii_lowercase().contains(needle))
}

fn search_order(kind: api::SearchResultKind) -> usize {
    match kind {
        api::SearchResultKind::Project => 0,
        api::SearchResultKind::Repository => 1,
        api::SearchResultKind::Checkout => 2,
        api::SearchResultKind::Worktree => 3,
        api::SearchResultKind::Review => 4,
        api::SearchResultKind::File
        | api::SearchResultKind::Symbol
        | api::SearchResultKind::Markdown => 5,
        api::SearchResultKind::Commit | api::SearchResultKind::Branch => 6,
        api::SearchResultKind::PullRequest => 7,
        api::SearchResultKind::Ci => 8,
        api::SearchResultKind::Artifact => 9,
    }
}

fn repository_label(sources: &[domain::ReviewSource]) -> String {
    sources.first().map_or_else(
        || "Review".into(),
        |source| match source {
            domain::ReviewSource::PullRequest { repository, .. }
            | domain::ReviewSource::CiRun { repository, .. } => repository.clone(),
            domain::ReviewSource::Markdown { path, .. } => path_label(path),
            domain::ReviewSource::LocalWorktree { .. } => "Local worktree".into(),
            domain::ReviewSource::CommitRange { .. } => "Commit range".into(),
            domain::ReviewSource::Artifact { .. } => "Artifact".into(),
        },
    )
}

fn branch_label(sources: &[domain::ReviewSource]) -> String {
    sources.first().map_or_else(
        || "review".into(),
        |source| match source {
            domain::ReviewSource::CommitRange { head, .. } => head.clone(),
            domain::ReviewSource::PullRequest { number, .. } => format!("PR #{number}"),
            domain::ReviewSource::CiRun { run_id, .. } => run_id.clone(),
            _ => "checkpoint".into(),
        },
    )
}

fn path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("repository")
        .to_owned()
}

fn path_hint(path: &Path) -> String {
    let parts = path
        .components()
        .rev()
        .take(3)
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    parts.into_iter().rev().collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ApplicationPaths;

    #[test]
    fn local_client_bootstraps_a_fresh_isolated_catalog() {
        let temporary = tempfile::tempdir().unwrap();
        let runtime = RuntimeHandle::spawn(ApplicationPaths::at(temporary.path())).unwrap();
        let client = LocalWorkdeckClient::spawn(runtime).unwrap();
        let response =
            futures::executor::block_on(client.request(api::WorkdeckRequest::Bootstrap {
                request_id: api::RequestId::from("bootstrap"),
            }))
            .unwrap();
        let api::WorkdeckResponse::Bootstrap(snapshot) = response.payload else {
            panic!("expected bootstrap response");
        };
        assert_eq!(snapshot.portfolio.project_count, 0);
        assert!(snapshot.inbox.items.is_empty());
    }

    #[test]
    fn line_diff_preserves_both_line_number_spaces() {
        let diff = line_diff("one\ntwo\n", "one\nthree\n", &[], &[]);
        assert!(
            diff.iter().any(|line| {
                line.kind == api::DiffLineKind::Removed && line.old_number == Some(2)
            })
        );
        assert!(
            diff.iter().any(|line| {
                line.kind == api::DiffLineKind::Added && line.new_number == Some(2)
            })
        );
    }

    #[test]
    fn presenter_maps_semantic_spans_to_their_original_lines() {
        let source = "pub fn render(value: usize) -> usize {\n    value + 1\n}\n";
        let lines = syntax_spans_by_line(Path::new("src/lib.rs"), source);

        assert_eq!(lines.len(), 3);
        assert!(lines[0].iter().any(|span| span.token == "keyword"));
        assert!(lines[0].iter().any(|span| span.token == "function"));
        assert!(lines[0].iter().any(|span| span.token == "type"));
        assert!(lines[1].iter().any(|span| span.token == "number"));
        assert!(lines.iter().zip(source.lines()).all(|(spans, line)| {
            spans.iter().all(|span| {
                span.start < span.end
                    && span.end <= line.len()
                    && line.is_char_boundary(span.start)
                    && line.is_char_boundary(span.end)
            })
        }));
    }

    #[test]
    fn path_hints_never_expose_an_entire_absolute_path() {
        assert_eq!(
            path_hint(Path::new("/Users/example/Sites/sampleapp")),
            "example/Sites/sampleapp"
        );
    }

    #[test]
    fn git_mapping_preserves_topology_and_models_working_tree_as_a_wip_row() {
        let now = chrono::Utc::now();
        let graph = workdeck_git::GitGraphSnapshot {
            root: std::path::PathBuf::from("/temporary/repository"),
            head: Some("merge".into()),
            branch: Some("main".into()),
            rows: vec![workdeck_git::GitGraphRow {
                commit: workdeck_git::GitCommit {
                    oid: "merge".into(),
                    summary: "Merge feature".into(),
                    body: String::new(),
                    author_name: "Workdeck".into(),
                    author_email: "workdeck@example.test".into(),
                    authored_at: now,
                    parents: vec!["main-parent".into(), "feature-parent".into()],
                    references: Vec::new(),
                },
                lane: 0,
                edges: vec![
                    workdeck_git::GitGraphEdge {
                        from_lane: 0,
                        to_lane: 0,
                        parent_oid: "main-parent".into(),
                    },
                    workdeck_git::GitGraphEdge {
                        from_lane: 0,
                        to_lane: 1,
                        parent_oid: "feature-parent".into(),
                    },
                ],
                lanes_before: Vec::new(),
                lanes_after: vec![0, 1],
            }],
            references: Vec::new(),
            status: workdeck_git::GitStatusSummary {
                staged: 1,
                unstaged: 2,
                untracked: 1,
                conflicted: 0,
            },
            truncated: false,
        };

        let mapped = map_git_graph(graph, domain::WorktreeId::from("worktree-test"), 0);
        assert_eq!(mapped.rows.len(), 2);
        assert!(mapped.rows[0].wip);
        assert_eq!(mapped.rows[0].files_changed, 4);
        assert_eq!(mapped.rows[0].edges[0].parent_oid, "merge");
        assert_eq!(mapped.rows[1].lanes_before, vec![0]);
        assert_eq!(mapped.rows[1].lanes_after, vec![0, 1]);
        assert_eq!(mapped.rows[1].edges.len(), 2);
        assert_eq!(mapped.rows[1].edges[1].to_lane, 1);
        assert_eq!(mapped.rows[1].edges[1].parent_oid, "feature-parent");
    }
}
