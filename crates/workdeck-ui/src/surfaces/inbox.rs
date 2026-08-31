use crate::components::{EmptyState, IconGlyph, ProgressBar, WorkdeckIcon};
use chrono::{DateTime, Utc};
use dioxus::prelude::*;
use workdeck_api::{
    ActivityKind, ActivityTarget, BootstrapSnapshot, InboxItem, OperationId, RequestId,
    WorkdeckClient, WorkdeckRequest,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum UpdateFilter {
    #[default]
    Unread,
    All,
}

#[component]
pub fn InboxSurface(
    client: WorkdeckClient,
    snapshot: BootstrapSnapshot,
    onopen: EventHandler<ActivityTarget>,
    onchanged: EventHandler<()>,
) -> Element {
    let mut filter = use_signal(UpdateFilter::default);
    let mut scanning = use_signal(|| None::<OperationId>);
    let mut marking = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let visible = snapshot
        .inbox
        .items
        .iter()
        .filter(|item| filter() == UpdateFilter::All || item.unread)
        .cloned()
        .collect::<Vec<_>>();
    let commit_items = visible
        .iter()
        .filter(|item| item.kind == ActivityKind::CommitBranch)
        .cloned()
        .collect::<Vec<_>>();
    let pull_items = visible
        .iter()
        .filter(|item| item.kind == ActivityKind::PullRequest)
        .cloned()
        .collect::<Vec<_>>();

    rsx! {
        section { class: "surface inbox-surface",
            header { class: "updates-header",
                div { class: "updates-header__title",
                    h1 { "Updates" }
                    p { "New commits and pull-request activity across your projects." }
                }
                div { class: "updates-header__tabs", role: "tablist", aria_label: "Update filter",
                    button { class: if filter() == UpdateFilter::Unread { "updates-tab is-selected" } else { "updates-tab" }, r#type: "button", role: "tab", aria_selected: filter() == UpdateFilter::Unread, onclick: move |_| filter.set(UpdateFilter::Unread), "Unread" span { "{snapshot.inbox.unread}" } }
                    button { class: if filter() == UpdateFilter::All { "updates-tab is-selected" } else { "updates-tab" }, r#type: "button", role: "tab", aria_selected: filter() == UpdateFilter::All, onclick: move |_| filter.set(UpdateFilter::All), "All" }
                }
                div { class: "updates-header__actions",
                    if snapshot.inbox.unread > 0 {
                        button {
                            class: "button button--quiet",
                            r#type: "button",
                            disabled: marking(),
                            onclick: {
                                let client = client.clone();
                                let unread = snapshot.inbox.items.iter().filter(|item| item.unread).cloned().collect::<Vec<_>>();
                                move |_| {
                                    let client = client.clone();
                                    let unread = unread.clone();
                                    marking.set(true);
                                    error.set(None);
                                    spawn(async move {
                                        for item in unread {
                                            if let Err(value) = mark_read(&client, &item).await {
                                                error.set(Some(value));
                                                marking.set(false);
                                                return;
                                            }
                                        }
                                        marking.set(false);
                                        onchanged.call(());
                                    });
                                }
                            },
                            if marking() { "Marking…" } else { "Mark all read" }
                        }
                    }
                    button { class: "icon-button", r#type: "button", aria_label: "Refresh updates", title: "Refresh updates", disabled: scanning().is_some(), onclick: move |_| {
                        let client = client.clone();
                        let operation_id = OperationId::new();
                        scanning.set(Some(operation_id.clone()));
                        error.set(None);
                        spawn(async move {
                            match client.request(WorkdeckRequest::ScanInbox { request_id: RequestId::new(), operation_id }).await {
                                Ok(_) => onchanged.call(()),
                                Err(value) => error.set(Some(value.to_string())),
                            }
                            scanning.set(None);
                        });
                    }, WorkdeckIcon { glyph: IconGlyph::Refresh, size: 15 } }
                }
            }
            if let Some(message) = error() { div { class: "inline-error surface-error", role: "alert", "{message}" } }
            if let Some(scan) = &snapshot.inbox.scan {
                div { class: "scan-progress",
                    div { span { "{scan.label}" } strong { "{scan.completed}/{scan.total}" } }
                    ProgressBar { value: scan.completed, total: scan.total, label: scan.label.clone() }
                }
            }
            if visible.is_empty() {
                EmptyState {
                    glyph: IconGlyph::Check,
                    title: if filter() == UpdateFilter::Unread { "You're caught up".to_owned() } else { "No commit or pull-request activity yet".to_owned() },
                    message: if filter() == UpdateFilter::Unread { "New commits and pull-request changes will appear here.".to_owned() } else { "Refresh the portfolio or open Pull requests to load provider activity.".to_owned() }
                }
            } else {
                div { class: "inbox-groups",
                    UpdateGroup {
                        title: "Commits".to_owned(),
                        items: commit_items,
                        reference_time: snapshot.inbox.updated_at,
                        client: client.clone(),
                        onopen: move |target| onopen.call(target),
                        onchanged: move |_| onchanged.call(()),
                    }
                    UpdateGroup {
                        title: "Pull requests".to_owned(),
                        items: pull_items,
                        reference_time: snapshot.inbox.updated_at,
                        client: client.clone(),
                        onopen: move |target| onopen.call(target),
                        onchanged: move |_| onchanged.call(()),
                    }
                }
            }
        }
    }
}

#[component]
fn UpdateGroup(
    title: String,
    items: Vec<InboxItem>,
    reference_time: DateTime<Utc>,
    client: WorkdeckClient,
    onopen: EventHandler<ActivityTarget>,
    onchanged: EventHandler<()>,
) -> Element {
    if items.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "inbox-group",
            header { class: "section-heading",
                h2 { "{title}" }
                span { "{items.len()}" }
            }
            div { class: "inbox-list",
                for item in items {
                    UpdateRow {
                        key: "{item.id}",
                        item,
                        reference_time,
                        client: client.clone(),
                        onopen: move |target| onopen.call(target),
                        onchanged: move |_| onchanged.call(()),
                    }
                }
            }
        }
    }
}

#[component]
fn UpdateRow(
    item: InboxItem,
    reference_time: DateTime<Utc>,
    client: WorkdeckClient,
    onopen: EventHandler<ActivityTarget>,
    onchanged: EventHandler<()>,
) -> Element {
    let mut busy = use_signal(|| false);
    let age = reference_time.signed_duration_since(item.updated_at);
    let age_label = if age.num_minutes() < 60 {
        format!("{}m", age.num_minutes().max(1))
    } else if age.num_hours() < 24 {
        format!("{}h", age.num_hours())
    } else {
        format!("{}d", age.num_days())
    };
    let glyph = match item.kind {
        ActivityKind::CommitBranch => IconGlyph::Git,
        ActivityKind::PullRequest => IconGlyph::PullRequest,
    };
    rsx! {
        article { class: if item.unread { "inbox-row is-unread" } else { "inbox-row" },
            button {
                class: "inbox-row__main",
                r#type: "button",
                aria_label: "Open {item.title}",
                onclick: {
                    let client = client.clone();
                    let item = item.clone();
                    move |_| {
                        onopen.call(item.target.clone());
                        if item.unread {
                            let client = client.clone();
                            let item = item.clone();
                            spawn(async move {
                                if mark_read(&client, &item).await.is_ok() {
                                    onchanged.call(());
                                }
                            });
                        }
                    }
                },
                span { class: "inbox-row__status",
                    if item.unread { span { class: "unread-dot", aria_label: "Unread" } }
                    WorkdeckIcon { glyph, size: 15 }
                }
                span { class: "inbox-row__content",
                    span { class: "inbox-row__topline",
                        strong { "{item.title}" }
                        time { class: "inbox-row__age", datetime: item.updated_at.to_rfc3339(), "{age_label}" }
                    }
                    span { class: "inbox-row__context", "{item.project} / {item.repository} · {item.branch}" }
                    span { class: "inbox-row__reason", "{item.summary}" }
                }
                span { class: "inbox-row__metrics",
                    if item.commits > 0 { span { "{item.commits} commits" } }
                    if item.changes > 0 { span { "{item.changes} files" } }
                }
            }
            if item.unread {
                button {
                    class: "inbox-row__read",
                    r#type: "button",
                    aria_label: "Mark {item.title} as read",
                    disabled: busy(),
                    onclick: {
                        let client = client.clone();
                        let item = item.clone();
                        move |_| {
                            let client = client.clone();
                            let item = item.clone();
                            busy.set(true);
                            spawn(async move {
                                let _ = mark_read(&client, &item).await;
                                busy.set(false);
                                onchanged.call(());
                            });
                        }
                    },
                    if busy() { "…" } else { "Mark read" }
                }
            }
        }
    }
}

async fn mark_read(client: &WorkdeckClient, item: &InboxItem) -> Result<(), String> {
    client
        .request(WorkdeckRequest::MarkActivityRead {
            request_id: RequestId::new(),
            target: item.target.clone(),
            revision: item.revision.clone(),
        })
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_filter_defaults_to_unread() {
        assert_eq!(UpdateFilter::default(), UpdateFilter::Unread);
    }
}
