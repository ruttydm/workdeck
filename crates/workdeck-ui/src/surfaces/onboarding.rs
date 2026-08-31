use crate::components::{IconGlyph, WorkdeckIcon};
use dioxus::prelude::*;
use workdeck_api::{RequestId, WorkdeckClient, WorkdeckRequest, WorkdeckResponse};

#[component]
pub fn OnboardingSurface(
    client: WorkdeckClient,
    suggested_roots: Vec<String>,
    oncomplete: EventHandler<()>,
) -> Element {
    let mut selected = use_signal(|| suggested_roots.clone());
    let mut state = use_signal(|| None::<Result<String, String>>);
    rsx! {
        section { class: "onboarding", aria_label: "Set up Workdeck",
            div { class: "onboarding__mark", WorkdeckIcon { glyph: IconGlyph::Git, size: 24 } }
            p { class: "eyebrow", "Welcome to Workdeck" }
            h1 { "Bring your development portfolio into focus." }
            p { class: "onboarding__intro", "Workdeck indexes repository identity and read-only Git evidence in its own catalog. It never writes state into your projects." }
            fieldset { class: "root-picker",
                legend { "Portfolio roots" }
                for root in suggested_roots {
                    label { class: "root-option",
                        input {
                            r#type: "checkbox",
                            checked: selected().contains(&root),
                            onchange: {
                                let root = root.clone();
                                move |event: Event<FormData>| {
                                    let mut values = selected();
                                    if event.checked() {
                                        if !values.contains(&root) { values.push(root.clone()); }
                                    } else {
                                        values.retain(|value| value != &root);
                                    }
                                    selected.set(values);
                                }
                            }
                        }
                        span { WorkdeckIcon { glyph: IconGlyph::Workspaces, size: 16 } strong { "{root}" } }
                    }
                }
            }
            div { class: "onboarding__actions",
                button {
                    class: "button button--primary",
                    r#type: "button",
                    disabled: selected().is_empty() || matches!(state(), Some(Ok(ref value)) if value == "loading"),
                    onclick: move |_| {
                        let client = client.clone();
                        let roots = selected();
                        state.set(Some(Ok("loading".into())));
                        spawn(async move {
                            match client.request(WorkdeckRequest::DiscoverPortfolio { request_id: RequestId::new(), roots }).await {
                                Ok(response) if matches!(response.payload, WorkdeckResponse::Portfolio(_)) => {
                                    state.set(Some(Ok("complete".into())));
                                    oncomplete.call(());
                                }
                                Ok(_) => state.set(Some(Err("Workdeck returned an unexpected discovery response.".into()))),
                                Err(error) => state.set(Some(Err(error.to_string()))),
                            }
                        });
                    },
                    if matches!(state(), Some(Ok(ref value)) if value == "loading") { "Discovering…" } else { "Discover repositories" }
                }
                span { "You can add or rescan roots later from Workspaces." }
            }
            if let Some(Err(error)) = state() {
                div { class: "inline-error", role: "alert", "{error}" }
            }
        }
    }
}
