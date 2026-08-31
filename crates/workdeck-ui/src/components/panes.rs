use dioxus::prelude::*;
use workdeck_api::{PaneWidths, RequestId, UiPreferencePatch, WorkdeckClient, WorkdeckRequest};

/// Shared presentation state for every task-owned split pane. Keeping this in
/// renderer context lets nested PR and commit change views preserve the same
/// geometry without coupling them to the application shell.
#[derive(Clone, Copy)]
pub struct PaneLayoutContext {
    pub widths: Signal<PaneWidths>,
    persist_callback: Callback<()>,
}

impl PaneLayoutContext {
    pub fn new(widths: Signal<PaneWidths>, client: WorkdeckClient) -> Self {
        let persist_callback = Callback::new(move |_| {
            let client = client.clone();
            let pane_widths = widths();
            spawn(async move {
                let _ = client
                    .request(WorkdeckRequest::UpdatePreferences {
                        request_id: RequestId::new(),
                        patch: UiPreferencePatch {
                            pane_widths: Some(pane_widths),
                            ..UiPreferencePatch::default()
                        },
                    })
                    .await;
            });
        });
        Self {
            widths,
            persist_callback,
        }
    }

    pub fn persist(&self) {
        self.persist_callback.call(());
    }

    pub fn current(&self) -> PaneWidths {
        (self.widths)()
    }

    pub fn update(&self, update: impl FnOnce(&mut PaneWidths)) {
        let mut widths = self.widths;
        update(&mut widths.write());
    }
}

/// A visible, focusable separator with pointer and keyboard parity. The owner
/// handles pointer movement so dragging remains stable even when the cursor
/// leaves this five-pixel hit region.
#[component]
pub fn PaneResizer(
    label: String,
    #[props(default)] class_name: String,
    value: f64,
    min: f64,
    max: f64,
    default_value: f64,
    #[props(default)] reverse: bool,
    onstart: EventHandler<PointerEvent>,
    onchange: EventHandler<f64>,
) -> Element {
    rsx! {
        div {
            class: "surface-resizer {class_name}",
            role: "separator",
            tabindex: "0",
            aria_label: "{label}",
            aria_orientation: "vertical",
            aria_valuemin: "{min:.0}",
            aria_valuemax: "{max:.0}",
            aria_valuenow: "{value:.0}",
            title: "{label} · drag, use ←/→, or double-click to reset",
            onpointerdown: move |event| onstart.call(event),
            ondoubleclick: move |_| onchange.call(default_value),
            onkeydown: move |event: KeyboardEvent| {
                let direction = if reverse { -1.0 } else { 1.0 };
                let next = match event.key() {
                    Key::ArrowLeft => Some(value - 16.0 * direction),
                    Key::ArrowRight => Some(value + 16.0 * direction),
                    Key::Home => Some(min),
                    Key::End => Some(max),
                    _ => None,
                };
                if let Some(next) = next {
                    event.prevent_default();
                    onchange.call(next.clamp(min, max));
                }
            }
        }
    }
}
