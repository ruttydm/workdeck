use crate::components::{
    Badge, CodeLanguageBadge, IconGlyph, PaneLayoutContext, PaneResizer, WorkdeckIcon,
    language_from_path,
};
use dioxus::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use workdeck_api::{DiffLine, ReviewSet, ReviewUnit};

use super::review::{SplitDiff, UnifiedDiff, visible_diff};

#[derive(Clone, PartialEq)]
pub struct TaskDiffContext {
    pub eyebrow: String,
    pub title: String,
    pub status: Option<String>,
    pub external_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskDiffMode {
    Unified,
    Split,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DirectoryNode {
    additions: usize,
    deletions: usize,
    directories: BTreeMap<String, DirectoryNode>,
    files: BTreeMap<String, ChangedFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ChangedFile {
    path: String,
    additions: usize,
    deletions: usize,
    units: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChangedTreeEntry {
    path: String,
    label: String,
    depth: usize,
    additions: usize,
    deletions: usize,
    kind: ChangedTreeEntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChangedTreeEntryKind {
    Directory,
    File,
}

#[component]
pub fn TaskChangesSurface(review: Arc<ReviewSet>, context: TaskDiffContext) -> Element {
    let mut mode = use_signal(|| TaskDiffMode::Unified);
    let mut selected_path = use_signal(|| None::<String>);
    let mut show_whitespace = use_signal(|| true);
    let mut show_files = use_signal(|| true);
    let mut resizing = use_signal(|| None::<(f64, f64)>);
    let pane_layout = use_context::<PaneLayoutContext>();
    let files_width = pane_layout.current().review_tree;
    let file_paths = changed_file_paths(&review);
    let active_path = selected_path()
        .filter(|path| file_paths.iter().any(|candidate| candidate == path))
        .or_else(|| file_paths.first().cloned());
    let additions = review
        .units
        .iter()
        .map(|unit| unit.additions)
        .sum::<usize>();
    let deletions = review
        .units
        .iter()
        .map(|unit| unit.deletions)
        .sum::<usize>();

    rsx! {
        section {
            class: "task-changes",
            style: "--changed-files-width: {files_width}px",
            onpointermove: move |event| {
                if let Some((start_x, start_width)) = resizing() {
                    let next = (start_width - (event.client_coordinates().x - start_x)).clamp(220.0, 480.0);
                    pane_layout.update(|widths| widths.review_tree = next);
                }
            },
            onpointerup: move |_| {
                if resizing().is_some() {
                    resizing.set(None);
                    pane_layout.persist();
                }
            },
            TaskDiffToolbar {
                context: context.clone(),
                mode: mode(),
                show_whitespace: show_whitespace(),
                show_files: show_files(),
                file_count: file_paths.len(),
                additions,
                deletions,
                onmode: move |next| mode.set(next),
                ontogglewhitespace: move |_| show_whitespace.toggle(),
                ontogglefiles: move |_| show_files.toggle(),
            }
            div {
                class: if show_files() { "task-changes__body" } else { "task-changes__body files-collapsed" },
                TaskFileDiff {
                    review: review.clone(),
                    path: active_path.clone(),
                    mode: mode(),
                    show_whitespace: show_whitespace(),
                }
                if show_files() {
                    PaneResizer {
                        label: "Resize changed files".to_owned(),
                        class_name: "task-files-resizer".to_owned(),
                        value: files_width,
                        min: 220.0,
                        max: 480.0,
                        default_value: 272.0,
                        reverse: true,
                        onstart: move |event: PointerEvent| resizing.set(Some((event.client_coordinates().x, files_width))),
                        onchange: move |next| {
                            pane_layout.update(|widths| widths.review_tree = next);
                            pane_layout.persist();
                        },
                    }
                    ChangedFilesTree {
                        review: review.clone(),
                        selected_path: active_path,
                        onselect: move |path| selected_path.set(Some(path)),
                    }
                }
            }
        }
    }
}

#[component]
fn TaskDiffToolbar(
    context: TaskDiffContext,
    mode: TaskDiffMode,
    show_whitespace: bool,
    show_files: bool,
    file_count: usize,
    additions: usize,
    deletions: usize,
    onmode: EventHandler<TaskDiffMode>,
    ontogglewhitespace: EventHandler<()>,
    ontogglefiles: EventHandler<()>,
) -> Element {
    rsx! {
        header { class: "task-diff-toolbar",
            div { class: "task-diff-toolbar__context",
                small { "{context.eyebrow}" }
                strong { title: "{context.title}", "{context.title}" }
            }
            div { class: "segmented-control task-diff-toolbar__modes", role: "tablist", aria_label: "Diff layout",
                button {
                    class: if mode == TaskDiffMode::Unified { "segment is-selected" } else { "segment" },
                    role: "tab",
                    aria_selected: mode == TaskDiffMode::Unified,
                    r#type: "button",
                    onclick: move |_| onmode.call(TaskDiffMode::Unified),
                    "Unified"
                }
                button {
                    class: if mode == TaskDiffMode::Split { "segment is-selected" } else { "segment" },
                    role: "tab",
                    aria_selected: mode == TaskDiffMode::Split,
                    r#type: "button",
                    onclick: move |_| onmode.call(TaskDiffMode::Split),
                    "Split"
                }
            }
            span { class: "task-diff-toolbar__stats",
                span { "{file_count} files" }
                span { class: "text-success", "+{additions}" }
                span { class: "text-danger", "−{deletions}" }
            }
            if let Some(status) = context.status { Badge { tone: "neutral", "{status}" } }
            button {
                class: if show_whitespace { "icon-button is-selected" } else { "icon-button" },
                r#type: "button",
                aria_label: if show_whitespace { "Hide whitespace-only lines" } else { "Show whitespace-only lines" },
                aria_pressed: show_whitespace,
                title: "Toggle whitespace-only lines",
                onclick: move |_| ontogglewhitespace.call(()),
                "¶"
            }
            button {
                class: if show_files { "icon-button is-selected" } else { "icon-button" },
                r#type: "button",
                aria_label: if show_files { "Hide changed files" } else { "Show changed files" },
                aria_pressed: show_files,
                title: if show_files { "Hide changed files" } else { "Show changed files" },
                onclick: move |_| ontogglefiles.call(()),
                WorkdeckIcon { glyph: IconGlyph::Navigator, size: 14 }
            }
            if let Some(url) = context.external_url {
                a { class: "icon-button", href: "{url}", target: "_blank", rel: "noreferrer", aria_label: "Open on GitHub", title: "Open on GitHub",
                    WorkdeckIcon { glyph: IconGlyph::External, size: 14 }
                }
            }
        }
    }
}

#[component]
fn TaskFileDiff(
    review: Arc<ReviewSet>,
    path: Option<String>,
    mode: TaskDiffMode,
    show_whitespace: bool,
) -> Element {
    let Some(path) = path else {
        return rsx!(div { class: "task-changes__state", WorkdeckIcon { glyph: IconGlyph::Check, size: 20 } strong { "No changed files" } });
    };
    let units = review
        .units
        .iter()
        .filter(|unit| unit.path == path)
        .cloned()
        .collect::<Vec<_>>();
    let additions = units.iter().map(|unit| unit.additions).sum::<usize>();
    let deletions = units.iter().map(|unit| unit.deletions).sum::<usize>();
    let lines = visible_diff(
        units
            .iter()
            .flat_map(|unit| unit.diff.clone())
            .collect::<Vec<DiffLine>>(),
        show_whitespace,
    );
    let file_name = path.rsplit('/').next().unwrap_or(&path);

    rsx! {
        article { class: "task-file-diff",
            header { class: "task-file-diff__header",
                div { WorkdeckIcon { glyph: IconGlyph::File, size: 14 } span { strong { "{file_name}" } small { "{path}" } } }
                div { class: "task-file-diff__meta",
                    CodeLanguageBadge { language: language_from_path(&path).to_owned() }
                    span { class: "task-file-diff__stats", span { class: "text-success", "+{additions}" } span { class: "text-danger", "−{deletions}" } }
                }
            }
            match mode {
                TaskDiffMode::Unified => rsx!(UnifiedDiff { lines }),
                TaskDiffMode::Split => rsx!(SplitDiff { lines }),
            }
        }
    }
}

#[component]
fn ChangedFilesTree(
    review: Arc<ReviewSet>,
    selected_path: Option<String>,
    onselect: EventHandler<String>,
) -> Element {
    let mut collapsed = use_signal(BTreeSet::<String>::new);
    let entries = changed_tree_entries(&review.units);
    let file_count = entries
        .iter()
        .filter(|entry| entry.kind == ChangedTreeEntryKind::File)
        .count();

    rsx! {
        aside { class: "changed-files-tree", aria_label: "Changed files",
            header { class: "changed-files-tree__header",
                strong { "Changed files" }
                span { "{file_count}" }
            }
            div { class: "changed-files-tree__rows", role: "tree", aria_label: "Changed file hierarchy",
                for entry in entries.into_iter().filter(|entry| tree_entry_visible(entry, &collapsed())) {
                    if entry.kind == ChangedTreeEntryKind::Directory {
                        {
                            let is_collapsed = collapsed().contains(&entry.path);
                            let path = entry.path.clone();
                            rsx! {
                                button {
                                    key: "dir-{entry.path}",
                                    class: "changed-tree-row changed-tree-row--directory",
                                    style: "--tree-depth: {entry.depth}",
                                    role: "treeitem",
                                    aria_level: entry.depth + 1,
                                    aria_expanded: !is_collapsed,
                                    r#type: "button",
                                    title: "{entry.path}",
                                    onclick: move |_| {
                                        if collapsed().contains(&path) { collapsed.write().remove(&path); }
                                        else { collapsed.write().insert(path.clone()); }
                                    },
                                    WorkdeckIcon { glyph: if is_collapsed { IconGlyph::ChevronRight } else { IconGlyph::ChevronDown }, size: 12 }
                                    WorkdeckIcon { glyph: IconGlyph::Workspaces, size: 13 }
                                    span { "{entry.label}" }
                                    span { class: "changed-tree-row__diff", span { class: "text-success", "+{entry.additions}" } span { class: "text-danger", "−{entry.deletions}" } }
                                }
                            }
                        }
                    } else {
                        {
                            let path = entry.path.clone();
                            rsx! {
                                button {
                                    key: "file-{entry.path}",
                                    class: if selected_path.as_ref() == Some(&entry.path) { "changed-tree-row changed-tree-row--file is-selected" } else { "changed-tree-row changed-tree-row--file" },
                                    style: "--tree-depth: {entry.depth}",
                                    role: "treeitem",
                                    aria_level: entry.depth + 1,
                                    aria_selected: selected_path.as_ref() == Some(&entry.path),
                                    r#type: "button",
                                    title: "{entry.path}",
                                    onclick: move |_| onselect.call(path.clone()),
                                    span { class: "changed-tree-row__spacer" }
                                    WorkdeckIcon { glyph: IconGlyph::File, size: 13 }
                                    span { "{entry.label}" }
                                    span { class: "changed-tree-row__diff", span { class: "text-success", "+{entry.additions}" } span { class: "text-danger", "−{entry.deletions}" } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn changed_file_paths(review: &ReviewSet) -> Vec<String> {
    review
        .units
        .iter()
        .map(|unit| unit.path.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn changed_tree_entries(units: &[ReviewUnit]) -> Vec<ChangedTreeEntry> {
    let mut root = DirectoryNode::default();
    for unit in units {
        insert_unit(&mut root, unit);
    }
    let mut entries = Vec::new();
    flatten_tree(&root, "", 0, &mut entries);
    entries
}

fn insert_unit(root: &mut DirectoryNode, unit: &ReviewUnit) {
    let components = unit
        .path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some((file_name, directories)) = components.split_last() else {
        return;
    };
    root.additions += unit.additions;
    root.deletions += unit.deletions;
    let mut node = root;
    for directory in directories {
        node = node.directories.entry((*directory).to_owned()).or_default();
        node.additions += unit.additions;
        node.deletions += unit.deletions;
    }
    let file = node.files.entry((*file_name).to_owned()).or_default();
    file.path = unit.path.clone();
    file.additions += unit.additions;
    file.deletions += unit.deletions;
    file.units += 1;
}

fn flatten_tree(
    node: &DirectoryNode,
    parent: &str,
    depth: usize,
    entries: &mut Vec<ChangedTreeEntry>,
) {
    for (name, directory) in &node.directories {
        let path = join_tree_path(parent, name);
        entries.push(ChangedTreeEntry {
            path: path.clone(),
            label: name.clone(),
            depth,
            additions: directory.additions,
            deletions: directory.deletions,
            kind: ChangedTreeEntryKind::Directory,
        });
        flatten_tree(directory, &path, depth + 1, entries);
    }
    for (name, file) in &node.files {
        entries.push(ChangedTreeEntry {
            path: file.path.clone(),
            label: name.clone(),
            depth,
            additions: file.additions,
            deletions: file.deletions,
            kind: ChangedTreeEntryKind::File,
        });
    }
}

fn join_tree_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else {
        format!("{parent}/{child}")
    }
}

fn tree_entry_visible(entry: &ChangedTreeEntry, collapsed: &BTreeSet<String>) -> bool {
    let mut path = entry.path.as_str();
    while let Some((parent, _)) = path.rsplit_once('/') {
        if collapsed.contains(parent) {
            return false;
        }
        path = parent;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_files_are_grouped_into_real_directory_levels() {
        let review =
            workdeck_api::fixtures::review(&workdeck_api::ReviewId::from("review-sampleapp"))
                .expect("fixture review");
        let entries = changed_tree_entries(&review.units);
        assert!(
            entries
                .iter()
                .any(|entry| { entry.kind == ChangedTreeEntryKind::Directory && entry.depth == 0 })
        );
        assert!(
            entries
                .iter()
                .any(|entry| { entry.kind == ChangedTreeEntryKind::File && entry.depth > 0 })
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.kind == ChangedTreeEntryKind::File)
                .count(),
            changed_file_paths(&review).len()
        );
    }

    #[test]
    fn collapsing_a_directory_hides_only_its_descendants() {
        let entries = [
            ChangedTreeEntry {
                path: "src".into(),
                label: "src".into(),
                depth: 0,
                additions: 1,
                deletions: 0,
                kind: ChangedTreeEntryKind::Directory,
            },
            ChangedTreeEntry {
                path: "src/app.rs".into(),
                label: "app.rs".into(),
                depth: 1,
                additions: 1,
                deletions: 0,
                kind: ChangedTreeEntryKind::File,
            },
            ChangedTreeEntry {
                path: "README.md".into(),
                label: "README.md".into(),
                depth: 0,
                additions: 1,
                deletions: 0,
                kind: ChangedTreeEntryKind::File,
            },
        ];
        let collapsed = BTreeSet::from(["src".to_owned()]);
        assert!(tree_entry_visible(&entries[0], &collapsed));
        assert!(!tree_entry_visible(&entries[1], &collapsed));
        assert!(tree_entry_visible(&entries[2], &collapsed));
    }
}
