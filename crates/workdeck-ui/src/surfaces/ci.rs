use crate::components::{
    Badge, EmptyState, IconGlyph, PaneLayoutContext, PaneResizer, StatusDot, WorkdeckIcon,
};
use dioxus::prelude::*;
use std::time::Duration;
use workdeck_api::{
    CiJob, CiJobLog, CiRun, CiStatus, RequestId, WorkdeckClient, WorkdeckRequest, WorkdeckResponse,
};

const CI_CACHE_MAX_AGE: Duration = Duration::from_secs(60);
const CI_LOG_CACHE_MAX_AGE: Duration = Duration::from_secs(600);

#[component]
pub fn CiSurface(
    client: WorkdeckClient,
    repositories: Vec<String>,
    repository: Option<String>,
    onrepositorychange: EventHandler<String>,
    initial_runs: Vec<CiRun>,
) -> Element {
    let mut generation = use_signal(|| 0_u64);
    let mut master_visible = use_signal(|| true);
    let mut resizing = use_signal(|| None::<(f64, f64)>);
    let mut retained_runs = use_signal(|| initial_runs.clone());
    let pane_layout = use_context::<PaneLayoutContext>();
    let request_client = client.clone();
    let refresh_client = client.clone();
    let requested_repository = repository.clone();
    let refresh_repository = repository.clone();
    let remote = use_resource(move || {
        let client = request_client.clone();
        let repository = requested_repository.clone();
        let _generation = generation();
        async move {
            client
                .request_cached(
                    WorkdeckRequest::LoadCiRuns {
                        request_id: RequestId::new(),
                        repository,
                    },
                    CI_CACHE_MAX_AGE,
                )
                .await
        }
    });
    let loaded_runs = remote
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|response| match &response.payload {
            WorkdeckResponse::CiRuns(runs) => Some(runs.clone()),
            _ => None,
        });
    if let Some(runs) = loaded_runs.as_ref()
        && retained_runs() != *runs
    {
        retained_runs.set(runs.clone());
    }
    let runs = match loaded_runs {
        Some(runs) => runs,
        None => retained_runs(),
    };
    let mut selected_run = use_signal(|| None::<String>);
    let selected_id = selected_run().or_else(|| runs.first().map(|run| run.id.clone()));
    let selected = runs
        .iter()
        .find(|run| Some(&run.id) == selected_id.as_ref())
        .cloned();
    rsx! {
        section {
            class: "surface ci-surface",
            style: "--master-list-width: {pane_layout.current().master_list}px",
            onpointermove: move |event| if let Some((start_x, start_width)) = resizing() {
                pane_layout.update(|widths| widths.master_list = (start_width + event.client_coordinates().x - start_x).clamp(300.0, 640.0));
            },
            onpointerup: move |_| if resizing().is_some() {
                resizing.set(None);
                pane_layout.persist();
            },
            div { class: if master_visible() { "master-detail" } else { "master-detail master-collapsed" },
                if master_visible() {
                aside { class: "master-list ci-master-list", aria_label: "CI run browser",
                    div { class: "master-list__toolbar",
                        select { class: "provider-select", aria_label: "GitHub repository", value: repository.clone().unwrap_or_default(), onchange: move |event| onrepositorychange.call(event.value()),
                            if repositories.is_empty() { option { value: "", "No GitHub repositories" } }
                            for provider in repositories.clone() { option { value: "{provider}", "{provider}" } }
                        }
                        strong { "Runs" }
                        button { class: "icon-button", r#type: "button", aria_label: "Refresh runs", onclick: move |_| {
                            refresh_client.invalidate_cached(&WorkdeckRequest::LoadCiRuns {
                                request_id: RequestId::new(),
                                repository: refresh_repository.clone(),
                            });
                            generation += 1;
                        }, WorkdeckIcon { glyph: IconGlyph::Refresh, size: 15 } }
                    }
                    if runs.is_empty() {
                        EmptyState { glyph: IconGlyph::Ci, title: "No CI runs".to_owned(), message: repository.as_ref().map_or_else(|| "No GitHub repositories are available in this portfolio.".to_owned(), |repository| format!("No workflow runs for {repository}.")) }
                    } else {
                        div { class: "ci-run-list", role: "listbox", aria_label: "CI runs",
                            for run in runs.clone() {
                                button {
                                    class: if Some(&run.id) == selected_id.as_ref() { "ci-row is-selected" } else { "ci-row" },
                                    role: "option",
                                    aria_selected: Some(&run.id) == selected_id.as_ref(),
                                    aria_label: "Open CI run {run.name} for {run.repository}",
                                    r#type: "button",
                                    onclick: move |_| selected_run.set(Some(run.id.clone())),
                                    StatusDot { tone: status_tone(run.status) }
                                    span { class: "ci-row__body", strong { "{run.name}" } small { "{run.repository} · {run.branch}" } }
                                    code { "{run.commit}" }
                                }
                            }
                        }
                    }
                }
                PaneResizer {
                    label: "Resize CI run list".to_owned(),
                    value: pane_layout.current().master_list,
                    min: 300.0,
                    max: 640.0,
                    default_value: 420.0,
                    onstart: move |event: PointerEvent| resizing.set(Some((event.client_coordinates().x, pane_layout.current().master_list))),
                    onchange: move |next| {
                        pane_layout.update(|widths| widths.master_list = next);
                        pane_layout.persist();
                    },
                }
                }
                if let Some(run) = selected {
                    article { class: "detail-pane ci-detail",
                        header { class: "detail-header",
                            div { class: "detail-header__eyebrow", Badge { tone: status_tone(run.status), {format!("{:?}", run.status)} } span { "{run.repository} · {run.commit}" } }
                            h1 { "{run.name}" }
                            p { {format!("{} · {}", run.branch, run.duration_seconds.map_or_else(|| "running".into(), |seconds| format!("{seconds}s")))} }
                            div { class: "heading-actions ci-detail__actions",
                                button {
                                    class: if master_visible() { "icon-button is-selected" } else { "icon-button" },
                                    r#type: "button",
                                    aria_label: if master_visible() { "Hide CI run list" } else { "Show CI run list" },
                                    aria_pressed: master_visible(),
                                    title: if master_visible() { "Hide CI run list" } else { "Show CI run list" },
                                    onclick: move |_| master_visible.toggle(),
                                    WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 }
                                }
                                a { class: "button", href: "{run.url}", target: "_blank", rel: "noreferrer", WorkdeckIcon { glyph: IconGlyph::External, size: 14 } "GitHub" }
                            }
                        }
                        div { class: "ci-jobs",
                            for job in run.jobs {
                                CiJobPanel { key: "job-{job.id}", client: client.clone(), repository: run.repository.clone(), job }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn CiJobPanel(client: WorkdeckClient, repository: String, job: CiJob) -> Element {
    let mut expanded = use_signal(|| true);
    let mut loading = use_signal(|| false);
    let mut loaded_log = use_signal(|| None::<CiJobLog>);
    let mut error = use_signal(|| None::<String>);
    let initial_lines = job.log_lines.clone();
    rsx! {
        section { class: "ci-job",
            header {
                button { class: "ci-job__toggle", r#type: "button", aria_expanded: expanded(), onclick: move |_| expanded.toggle(), WorkdeckIcon { glyph: if expanded() { IconGlyph::ChevronDown } else { IconGlyph::ChevronRight }, size: 13 } StatusDot { tone: status_tone(job.status) } strong { "{job.name}" } }
                Badge { tone: status_tone(job.status), {format!("{:?}", job.status)} }
            }
            if expanded() {
                div { class: "ci-steps",
                    for step in job.steps.clone() { div { class: "ci-step", StatusDot { tone: status_tone(step.status) } span { "{step.name}" } small { {step.duration_seconds.map_or_else(|| "—".into(), |seconds| format!("{seconds}s"))} } } }
                }
                if let Some(log) = loaded_log() {
                    div { class: "ci-log__meta", span { "{log.original_bytes} bytes" } if log.truncated { Badge { tone: "warning", "Truncated" } } }
                    pre { class: "ci-log", tabindex: "0", for line in log.lines { code { "{line}\n" } } }
                } else if !initial_lines.is_empty() {
                    pre { class: "ci-log", tabindex: "0", for line in initial_lines { code { "{line}\n" } } }
                } else if let Some(message) = error() {
                    div { class: "inline-error", role: "alert", "{message}" }
                    button { class: "button", r#type: "button", onclick: move |_| error.set(None), "Try again" }
                } else {
                    button { class: "button ci-log-load", r#type: "button", disabled: loading(), onclick: move |_| {
                        let Ok(job_id) = job.id.parse::<u64>() else { error.set(Some("Provider returned an invalid job identifier.".into())); return; };
                        let client = client.clone();
                        let repository = repository.clone();
                        let job_name = job.name.clone();
                        loading.set(true);
                        spawn(async move {
                            match client.request_cached(WorkdeckRequest::LoadCiJobLog { request_id: RequestId::new(), repository, job_id, job_name }, CI_LOG_CACHE_MAX_AGE).await {
                                Ok(response) => if let WorkdeckResponse::CiJobLog(log) = response.payload { loaded_log.set(Some(log)); },
                                Err(value) => error.set(Some(value.to_string())),
                            }
                            loading.set(false);
                        });
                    }, if loading() { "Loading log…" } else { "Load job log" } }
                }
            }
        }
    }
}

fn status_tone(status: CiStatus) -> String {
    match status {
        CiStatus::Passed => "success",
        CiStatus::Failed | CiStatus::Cancelled => "danger",
        CiStatus::Running | CiStatus::Queued => "attention",
        CiStatus::Skipped => "neutral",
    }
    .into()
}
