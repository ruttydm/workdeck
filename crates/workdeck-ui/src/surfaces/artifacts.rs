use crate::components::{
    Badge, EmptyState, IconGlyph, PaneLayoutContext, PaneResizer, WorkdeckIcon,
};
use dioxus::core::use_drop;
use dioxus::prelude::*;
use std::time::Duration;
use workdeck_api::{
    ArtifactKind, ArtifactPreviewSession, ArtifactRecord, OperationId, RequestId, WorkdeckClient,
    WorkdeckRequest, WorkdeckResponse,
};

const ARTIFACT_CACHE_MAX_AGE: Duration = Duration::from_secs(300);

#[component]
pub fn ArtifactsSurface(client: WorkdeckClient, initial_artifacts: Vec<ArtifactRecord>) -> Element {
    let mut generation = use_signal(|| 0_u64);
    let mut selected = use_signal(|| None::<workdeck_api::ArtifactId>);
    let mut preview = use_signal(|| None::<ArtifactPreviewSession>);
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut library_visible = use_signal(|| true);
    let mut resizing = use_signal(|| None::<(f64, f64)>);
    let mut retained_artifacts = use_signal(|| initial_artifacts.clone());
    let pane_layout = use_context::<PaneLayoutContext>();
    let request_client = client.clone();
    let artifacts_resource = use_resource(move || {
        let client = request_client.clone();
        let _generation = generation();
        async move {
            client
                .request_cached(
                    WorkdeckRequest::LoadArtifacts {
                        request_id: RequestId::new(),
                    },
                    ARTIFACT_CACHE_MAX_AGE,
                )
                .await
        }
    });
    let loaded_artifacts = artifacts_resource
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|response| match &response.payload {
            WorkdeckResponse::Artifacts(artifacts) => Some(artifacts.clone()),
            _ => None,
        });
    if let Some(artifacts) = loaded_artifacts.as_ref()
        && retained_artifacts() != *artifacts
    {
        retained_artifacts.set(artifacts.clone());
    }
    let artifacts = match loaded_artifacts {
        Some(artifacts) => artifacts,
        None => retained_artifacts(),
    };
    let selected_id = selected().or_else(|| artifacts.first().map(|artifact| artifact.id.clone()));
    let selected_artifact = artifacts
        .iter()
        .find(|artifact| Some(&artifact.id) == selected_id.as_ref())
        .cloned();
    let cleanup_client = client.clone();
    use_drop(move || {
        if let Some(session) = preview.peek().as_ref() {
            cleanup_client.cancel(session.session_id.clone());
        }
    });
    let import_client = client.clone();
    let open_client = client.clone();
    let cancel_client = client.clone();
    let cancel_preview = EventHandler::<OperationId>::new(move |session_id| {
        cancel_client.cancel(session_id);
        preview.set(None);
    });

    rsx! {
        section {
            class: "surface artifacts-surface",
            style: "--artifact-library-width: {pane_layout.current().artifact_library}px",
            onpointermove: move |event| if let Some((start_x, start_width)) = resizing() {
                pane_layout.update(|widths| widths.artifact_library = (start_width + event.client_coordinates().x - start_x).clamp(240.0, 520.0));
            },
            onpointerup: move |_| if resizing().is_some() {
                resizing.set(None);
                pane_layout.persist();
            },
            header { class: "surface-heading surface-heading--compact",
                div { p { class: "eyebrow", "EVIDENCE" } h1 { "Artifacts" } p { "Inspect reports and build output without giving them access to Workdeck." } }
                div { class: "heading-actions",
                    if !artifacts.is_empty() {
                        button {
                            class: if library_visible() { "icon-button is-selected" } else { "icon-button" },
                            r#type: "button",
                            aria_label: if library_visible() { "Hide artifact library" } else { "Show artifact library" },
                            aria_pressed: library_visible(),
                            title: if library_visible() { "Hide artifact library" } else { "Show artifact library" },
                            onclick: move |_| library_visible.toggle(),
                            WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 }
                        }
                    }
                    button {
                        class: "button button--primary",
                        r#type: "button",
                        disabled: busy(),
                        onclick: move |_| {
                            let client = import_client.clone();
                            busy.set(true);
                            error.set(None);
                            spawn(async move {
                                match client.request(WorkdeckRequest::ChooseArtifactImport { request_id: RequestId::new() }).await {
                                    Ok(_) => generation += 1,
                                    Err(value) => error.set(Some(value.to_string())),
                                }
                                busy.set(false);
                            });
                        },
                        WorkdeckIcon { glyph: IconGlyph::Plus, size: 14 }
                        if busy() { "Importing…" } else { "Import artifact" }
                    }
                }
            }
            if let Some(message) = error() { div { class: "inline-error surface-error", role: "alert", "{message}" } }
            if artifacts.is_empty() {
                EmptyState { glyph: IconGlyph::Artifacts, title: "No artifacts".to_owned(), message: "Import a secure ZIP or download one from an attached CI run.".to_owned() }
            } else {
                div { class: if library_visible() { "artifact-layout" } else { "artifact-layout library-collapsed" },
                    if library_visible() {
                    div { class: "artifact-library", role: "listbox", aria_label: "Artifact library",
                        for artifact in artifacts.clone() {
                            button { class: if Some(&artifact.id) == selected_id.as_ref() { "artifact-row is-selected" } else { "artifact-row" }, r#type: "button", role: "option", aria_selected: Some(&artifact.id) == selected_id.as_ref(), onclick: move |_| {
                                    if let Some(session) = preview() { cancel_preview.call(session.session_id); }
                                    selected.set(Some(artifact.id.clone()));
                                },
                                aria_label: "Open artifact {artifact.name} from {artifact.source}",
                                span { class: "artifact-row__icon", WorkdeckIcon { glyph: IconGlyph::Artifacts, size: 18 } }
                                span { class: "artifact-row__body", strong { "{artifact.name}" } small { "{artifact.source}" } }
                                Badge { tone: "neutral", {format!("{:?}", artifact.kind)} }
                            }
                        }
                    }
                    PaneResizer {
                        label: "Resize artifact library".to_owned(),
                        value: pane_layout.current().artifact_library,
                        min: 240.0,
                        max: 520.0,
                        default_value: 320.0,
                        onstart: move |event: PointerEvent| resizing.set(Some((event.client_coordinates().x, pane_layout.current().artifact_library))),
                        onchange: move |next| {
                            pane_layout.update(|widths| widths.artifact_library = next);
                            pane_layout.persist();
                        },
                    }
                    }
                    if let Some(artifact) = selected_artifact {
                        article { class: "artifact-preview",
                            header { class: "artifact-preview__header",
                                div { h2 { "{artifact.name}" } p { "{artifact.source}" } }
                                div { class: "heading-actions",
                                    if let Some(session) = preview() {
                                        button { class: "button", r#type: "button", onclick: move |_| cancel_preview.call(session.session_id.clone()), "Close preview" }
                                    } else if artifact.preview_available {
                                        button { class: "button", r#type: "button", onclick: move |_| {
                                            let client = open_client.clone();
                                            let artifact_id = artifact.id.clone();
                                            error.set(None);
                                            spawn(async move {
                                                match client.request(WorkdeckRequest::OpenArtifact { request_id: RequestId::new(), artifact_id }).await {
                                                    Ok(response) => match response.payload {
                                                        WorkdeckResponse::ArtifactPreview(session) => preview.set(Some(session)),
                                                        _ => error.set(Some("Workdeck returned an unexpected preview response.".into())),
                                                    },
                                                    Err(value) => error.set(Some(value.to_string())),
                                                }
                                            });
                                        }, WorkdeckIcon { glyph: IconGlyph::External, size: 14 } "Open preview" }
                                    }
                                }
                            }
                            if let Some(session) = preview() {
                                div { class: "artifact-frame",
                                    div { class: "browser-chrome", span { class: "browser-dot" } span { class: "browser-dot" } span { class: "browser-dot" } code { "{session.url}" } }
                                    iframe { src: "{session.url}", title: "Artifact preview", "sandbox": "allow-scripts", "referrerpolicy": "no-referrer" }
                                }
                            } else if artifact.kind == ArtifactKind::Html && artifact.preview_available {
                                div { class: "artifact-frame-placeholder",
                                    div { class: "artifact-report-demo", Badge { tone: "success", "Ready" } h3 { "Sandboxed HTML report" } p { "Open the preview to start a loopback-only, exact-origin helper tied to this tab." } }
                                }
                            } else {
                                EmptyState { glyph: IconGlyph::File, title: "Preview unavailable".to_owned(), message: "This artifact can be inspected from its verified metadata.".to_owned() }
                            }
                            dl { class: "metadata-list artifact-metadata", div { dt { "Size" } dd { "{artifact.size_bytes} bytes" } } div { dt { "Entries" } dd { "{artifact.entry_count}" } } div { dt { "Isolation" } dd { "Loopback · exact origin · CSP" } } }
                        }
                    }
                }
            }
        }
    }
}
