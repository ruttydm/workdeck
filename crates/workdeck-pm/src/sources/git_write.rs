//! Git object construction without an index, checkout, hooks or application writes.
use super::{git::BoundGit, *};
use crate::{ErrorCode, PmError, Result, Timestamp};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, PathBuf},
};
#[derive(Clone)]
struct Entry {
    mode: String,
    kind: String,
    oid: GitOid,
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn oid(bytes: &[u8]) -> Result<GitOid> {
    std::str::from_utf8(bytes)
        .map_err(|_| invalid("Git object result must be UTF8"))?
        .trim()
        .parse()
}
impl BoundGit {
    fn write_blob(&self, bytes: &[u8]) -> Result<GitOid> {
        let result = self.run(
            &["hash-object".into(), "-w".into(), "--stdin".into()],
            Some(bytes.to_vec()),
            None,
            4096,
        )?;
        if !result.status.success() {
            return Err(invalid("could not write isolated Git blob"));
        }
        oid(&result.stdout)
    }
    /// Preserve every untouched entry of the exact observed tree, including
    /// application files. Only explicit, canonical planning paths are overlaid.
    pub(super) fn write_tree(
        &self,
        base: Option<&GitOid>,
        changes: &BTreeMap<PathBuf, Option<Vec<u8>>>,
    ) -> Result<GitOid> {
        for path in changes.keys() {
            if path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
                || path.components().any(|c| {
                    c.as_os_str()
                        .to_str()
                        .is_none_or(|s| s.chars().any(char::is_control))
                })
            {
                return Err(invalid(
                    "Git changes require canonical portable relative paths",
                ));
            }
        }
        self.write_subtree(base, changes, 0)
    }
    fn write_subtree(
        &self,
        base: Option<&GitOid>,
        changes: &BTreeMap<PathBuf, Option<Vec<u8>>>,
        depth: usize,
    ) -> Result<GitOid> {
        if depth > 32 {
            return Err(invalid("publication paths exceed directory depth bound"));
        }
        let mut entries = BTreeMap::<String, Entry>::new();
        if let Some(base) = base {
            let bytes = self.output(
                &["ls-tree".into(), "-z".into(), base.as_str().into()],
                None,
                16 * 1024 * 1024,
            )?;
            for row in bytes.split(|&b| b == 0).filter(|r| !r.is_empty()) {
                let row = std::str::from_utf8(row)
                    .map_err(|_| invalid("publication tree names must be UTF8"))?;
                let (header, name) = row
                    .split_once('\t')
                    .ok_or_else(|| invalid("invalid tree entry"))?;
                let cols = header.split_whitespace().collect::<Vec<_>>();
                if cols.len() != 3 || name.contains('/') || entries.contains_key(name) {
                    return Err(invalid("invalid tree entry"));
                }
                entries.insert(
                    name.into(),
                    Entry {
                        mode: cols[0].into(),
                        kind: cols[1].into(),
                        oid: cols[2].parse()?,
                    },
                );
            }
        }
        let mut groups = BTreeMap::<String, BTreeMap<PathBuf, Option<Vec<u8>>>>::new();
        for (path, content) in changes {
            let mut parts = path.components();
            let first = parts
                .next()
                .ok_or_else(|| invalid("empty tree change"))?
                .as_os_str()
                .to_str()
                .ok_or_else(|| invalid("tree path must be UTF8"))?;
            let suffix = parts.as_path();
            if suffix.as_os_str().is_empty() {
                match content {
                    Some(bytes) => {
                        if entries.get(first).is_some_and(|e| e.kind == "tree") {
                            return Err(invalid("file change conflicts with existing directory"));
                        }
                        entries.insert(
                            first.into(),
                            Entry {
                                mode: "100644".into(),
                                kind: "blob".into(),
                                oid: self.write_blob(bytes)?,
                            },
                        );
                    }
                    None => {
                        entries.remove(first);
                    }
                }
            } else {
                groups
                    .entry(first.into())
                    .or_default()
                    .insert(suffix.to_owned(), content.clone());
            }
        }
        for (name, children) in groups {
            let existing = entries.get(&name);
            if existing.is_some_and(|e| e.kind != "tree") {
                return Err(invalid("directory change conflicts with existing file"));
            }
            let tree = self.write_subtree(existing.map(|e| &e.oid), &children, depth + 1)?;
            // Git permits empty trees; retaining one is harmless and avoids
            // unrelated directory deletion in a publication overlay.
            entries.insert(
                name,
                Entry {
                    mode: "040000".into(),
                    kind: "tree".into(),
                    oid: tree,
                },
            );
        }
        let mut input = Vec::new();
        for (name, entry) in entries {
            input.extend_from_slice(
                format!("{} {} {}\t{}\0", entry.mode, entry.kind, entry.oid, name).as_bytes(),
            );
        }
        let result = self.run(&["mktree".into(), "-z".into()], Some(input), None, 4096)?;
        if !result.status.success() {
            return Err(invalid("could not construct isolated Git tree"));
        }
        oid(&result.stdout)
    }
    pub(super) fn commit_tree(
        &self,
        tree: &GitOid,
        parent: Option<&GitOid>,
        message: &str,
        at: Timestamp,
    ) -> Result<GitOid> {
        let mut args: Vec<OsString> = vec![
            "-c".into(),
            "commit.gpgsign=false".into(),
            "commit-tree".into(),
            tree.as_str().into(),
        ];
        if let Some(parent) = parent {
            args.extend(["-p".into(), parent.as_str().into()]);
        }
        let timestamp = at.to_rfc3339();
        let env = [
            ("GIT_AUTHOR_NAME", "Workdeck"),
            ("GIT_AUTHOR_EMAIL", "workdeck@localhost"),
            ("GIT_COMMITTER_NAME", "Workdeck"),
            ("GIT_COMMITTER_EMAIL", "workdeck@localhost"),
            ("GIT_AUTHOR_DATE", timestamp.as_str()),
            ("GIT_COMMITTER_DATE", timestamp.as_str()),
        ];
        let result =
            self.run_environment(&args, Some(message.as_bytes().to_vec()), None, 4096, &env)?;
        if !result.status.success() {
            return Err(invalid("could not construct isolated publication commit"));
        }
        oid(&result.stdout)
    }
    pub(super) fn changed_paths(
        &self,
        before: &GitOid,
        after: &GitOid,
    ) -> Result<std::collections::BTreeSet<PathBuf>> {
        let bytes = self.output(
            &[
                "diff-tree".into(),
                "--name-only".into(),
                "--no-commit-id".into(),
                "--no-renames".into(),
                "--no-ext-diff".into(),
                "-r".into(),
                "-z".into(),
                before.as_str().into(),
                after.as_str().into(),
                "--".into(),
            ],
            None,
            16 * 1024 * 1024,
        )?;
        bytes
            .split(|&b| b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| {
                std::str::from_utf8(p)
                    .map(PathBuf::from)
                    .map_err(|_| invalid("publication changed paths must be UTF8"))
            })
            .collect()
    }
    pub(super) fn parents(&self, commit: &GitOid) -> Result<Vec<GitOid>> {
        let bytes = self.output(
            &[
                "rev-list".into(),
                "--parents".into(),
                "--max-count=1".into(),
                commit.as_str().into(),
            ],
            None,
            4096,
        )?;
        let text =
            std::str::from_utf8(&bytes).map_err(|_| invalid("invalid commit parent response"))?;
        let mut parts = text.split_whitespace();
        if parts.next() != Some(commit.as_str()) {
            return Err(invalid("commit identity response differs"));
        }
        parts.map(str::parse).collect()
    }
    pub(super) fn coordination_root_only(&self, commit: &GitOid) -> Result<()> {
        let tree = self.tree(commit)?;
        let bytes = self.output(
            &["ls-tree".into(), "-z".into(), tree.as_str().into()],
            None,
            64 * 1024,
        )?;
        let rows = bytes
            .split(|&b| b == 0)
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>();
        if rows.len() != 1
            || !rows[0].starts_with(b"040000 tree ")
            || !rows[0].ends_with(b"\t.workdeck")
        {
            return Err(invalid(
                "coordination commits must contain only their isolated .workdeck tree",
            ));
        }
        Ok(())
    }
    pub(super) fn ancestor(&self, ancestor: &GitOid, descendant: &GitOid) -> Result<bool> {
        let result = self.run(
            &[
                "merge-base".into(),
                "--is-ancestor".into(),
                ancestor.as_str().into(),
                descendant.as_str().into(),
            ],
            None,
            None,
            4096,
        )?;
        match result.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(invalid("cannot establish observed publication ancestry")),
        }
    }
    pub(super) fn push_candidate(
        &self,
        url: &str,
        reference: &GitRefName,
        candidate: &GitOid,
    ) -> Result<PushResult> {
        let result = self.run(
            &[
                "push".into(),
                "--porcelain".into(),
                "--no-verify".into(),
                "--recurse-submodules=no".into(),
                "--".into(),
                url.into(),
                format!("{candidate}:{reference}").into(),
            ],
            None,
            None,
            64 * 1024,
        )?;
        if result.status.success() {
            return Ok(PushResult::Acknowledged);
        }
        // Only a definite per-ref rejection permits a new semantic attempt.
        // Transport failures/unchanged remote tips never prove no push occurred.
        for row in result.stdout.split(|&b| b == b'\n') {
            let Ok(row) = std::str::from_utf8(row) else {
                continue;
            };
            let columns = row.split('\t').collect::<Vec<_>>();
            if columns.len() != 3
                || columns[0] != "!"
                || !columns[1].ends_with(&format!(":{reference}"))
            {
                continue;
            }
            if columns[2].starts_with("[rejected]")
                && [
                    "(fetch first)",
                    "(non-fast-forward)",
                    "(already exists)",
                    "(stale info)",
                ]
                .iter()
                .any(|r| columns[2].ends_with(r))
            {
                return Ok(PushResult::RetryableRejection);
            }
            if columns[2].starts_with("[remote rejected]")
                && columns[2].ends_with("(failed to update ref)")
            {
                return Ok(PushResult::RetryableRejection);
            }
            if columns[2].starts_with("[remote rejected]") || columns[2].starts_with("[rejected]") {
                return Ok(PushResult::Rejected);
            }
        }
        Ok(PushResult::Uncertain)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PushResult {
    Acknowledged,
    RetryableRejection,
    Rejected,
    Uncertain,
}
