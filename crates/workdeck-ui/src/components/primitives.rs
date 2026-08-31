use super::{IconGlyph, WorkdeckIcon};
use dioxus::prelude::*;

#[component]
pub fn IconButton(
    label: String,
    glyph: IconGlyph,
    #[props(default = false)] selected: bool,
    #[props(default = false)] disabled: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button {
            class: if selected { "icon-button is-selected" } else { "icon-button" },
            r#type: "button",
            title: "{label}",
            aria_label: "{label}",
            aria_pressed: selected,
            disabled,
            onclick: move |event| onclick.call(event),
            WorkdeckIcon { glyph, size: 17 }
        }
    }
}

#[component]
pub fn Badge(children: Element, #[props(default = "neutral".to_owned())] tone: String) -> Element {
    rsx!(span { class: "badge badge--{tone}", {children} })
}

#[component]
pub fn StatusDot(#[props(default = "ready".to_owned())] tone: String) -> Element {
    rsx!(span {
        class: "status-dot status-dot--{tone}",
        aria_hidden: "true"
    })
}

#[component]
pub fn ProgressBar(value: usize, total: usize, label: String) -> Element {
    let percentage = if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    };
    rsx! {
        div { class: "progress", role: "progressbar", aria_label: "{label}", aria_valuemin: "0", aria_valuemax: "{total}", aria_valuenow: "{value}",
            div { class: "progress__fill", style: "width: {percentage:.2}%" }
        }
    }
}

#[component]
pub fn EmptyState(glyph: IconGlyph, title: String, message: String) -> Element {
    rsx! {
        section { class: "empty-state",
            div { class: "empty-state__icon", WorkdeckIcon { glyph, size: 22 } }
            h2 { "{title}" }
            p { "{message}" }
        }
    }
}
