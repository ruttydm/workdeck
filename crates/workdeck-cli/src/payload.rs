use crate::git;
use crate::search::SearchTarget;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn status_payload(snapshot: &git::RepoSnapshot) -> Value {
    let mut counts = BTreeMap::<&str, usize>::new();
    for change in &snapshot.changes {
        *counts.entry(change.kind.label()).or_default() += 1;
    }
    json!({
        "repo_root": snapshot.root,
        "counts": counts,
        "groups": snapshot.groups.iter().map(|group| {
            json!({
                "path": group.path,
                "files": group.files.len(),
                "additions": group.total_additions,
                "deletions": group.total_deletions,
                "changes": group.files.iter().map(change_payload).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
        "changes": snapshot.changes.iter().map(change_payload).collect::<Vec<_>>(),
    })
}

pub fn change_payload(change: &git::ChangeEntry) -> Value {
    json!({
        "path": change.path,
        "kind": change.kind.label(),
        "stage": change.stage_label(),
        "staged": change.staged,
        "unstaged": change.unstaged,
        "additions": change.additions,
        "deletions": change.deletions,
    })
}

pub fn file_preview_payload(preview: &git::FilePreview) -> Value {
    json!({
        "title": preview.title,
        "content": preview.content,
        "truncated": preview.truncated,
        "binary": preview.binary,
    })
}

pub fn changes_grouped_by_status(changes: &[git::ChangeEntry]) -> Vec<Value> {
    let mut grouped = BTreeMap::<&str, Vec<Value>>::new();
    for change in changes {
        grouped
            .entry(change.kind.label())
            .or_default()
            .push(change_payload(change));
    }
    grouped
        .into_iter()
        .map(|(status, changes)| json!({ "status": status, "changes": changes }))
        .collect()
}

pub fn search_target_group(target: &SearchTarget) -> &'static str {
    match target {
        SearchTarget::File(_) => "files",
        SearchTarget::Change(_) => "changes",
        SearchTarget::Issue(_) => "issues",
        SearchTarget::AgentSession(_) => "agents",
        SearchTarget::GitCommit(_)
        | SearchTarget::GitBranch(_)
        | SearchTarget::GitStash(_)
        | SearchTarget::GitTag(_) => "git",
        SearchTarget::Project(_) | SearchTarget::Cycle(_) | SearchTarget::Label(_) => "issues",
        SearchTarget::Symbol { .. } => "files",
    }
}

pub fn search_target_payload(target: &SearchTarget) -> Value {
    match target {
        SearchTarget::File(path) => json!({ "kind": "file", "path": path }),
        SearchTarget::Change(path) => json!({ "kind": "change", "path": path }),
        SearchTarget::Issue(key) => json!({ "kind": "issue", "key": key }),
        SearchTarget::AgentSession(id) => json!({ "kind": "agent", "id": id }),
        SearchTarget::GitCommit(sha) => json!({ "kind": "git_commit", "sha": sha }),
        SearchTarget::GitBranch(name) => json!({ "kind": "git_branch", "name": name }),
        SearchTarget::GitStash(name) => json!({ "kind": "git_stash", "name": name }),
        SearchTarget::GitTag(name) => json!({ "kind": "git_tag", "name": name }),
        SearchTarget::Project(id) => json!({ "kind": "project", "id": id }),
        SearchTarget::Cycle(id) => json!({ "kind": "cycle", "id": id }),
        SearchTarget::Label(id) => json!({ "kind": "label", "id": id }),
        SearchTarget::Symbol { path, line, name } => {
            json!({ "kind": "symbol", "path": path, "line": line, "name": name })
        }
    }
}
