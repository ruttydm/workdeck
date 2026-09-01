use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{ScopedEpochState, bump_scoped_epoch, reconcile_scoped_epochs, scoped_epoch};

/// One file-view registration after extension ownership has been attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredFileView {
    pub extension_id: String,
    pub view_id: String,
}

#[must_use]
pub fn qualified_file_view_key(extension_id: &str, view_id: &str) -> String {
    format!("{extension_id}:{view_id}")
}

#[must_use]
pub fn registered_file_view_key(view: &RegisteredFileView) -> String {
    qualified_file_view_key(&view.extension_id, &view.view_id)
}

/// Resolve a bare local or qualified view id without reserving extension ids.
#[must_use]
pub fn resolve_registered_file_view<'a>(
    views: &'a [RegisteredFileView],
    extension_id: &str,
    view_id: &str,
) -> Option<&'a RegisteredFileView> {
    let key = if view_id.contains(':') {
        view_id.to_owned()
    } else {
        qualified_file_view_key(extension_id, view_id)
    };
    views
        .iter()
        .find(|view| registered_file_view_key(view) == key)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileViewSelectionTarget<'a> {
    Registered(&'a RegisteredFileView),
    Refused(String),
}

/// Decide whether one native view can become the selected file presentation.
///
/// The caller performs the protocol-specific match request; this common policy
/// gives selection and mode entry identical handling for false results, child
/// failures, unknown ids, and host raw-only constraints.
pub fn resolve_file_view_selection_target<'a, F, E>(
    extension_id: &str,
    file: &F,
    registered: Option<&'a RegisteredFileView>,
    unavailable_reason: Option<&str>,
    view_id: &str,
    matches: impl FnOnce(&RegisteredFileView, &F) -> Result<bool, E>,
) -> FileViewSelectionTarget<'a> {
    if let Some(reason) = unavailable_reason {
        return FileViewSelectionTarget::Refused(reason.to_owned());
    }
    let Some(registered) = registered else {
        return FileViewSelectionTarget::Refused(format!(
            "Extension {extension_id} targeted unknown file view \"{view_id}\""
        ));
    };
    match matches(registered, file) {
        Ok(true) => FileViewSelectionTarget::Registered(registered),
        Ok(false) => FileViewSelectionTarget::Refused(format!(
            "File view \"{view_id}\" does not match the selected file • using raw diff"
        )),
        Err(_) => FileViewSelectionTarget::Refused(format!(
            "Extension {} file view \"{}\" failed matching the selected file",
            registered.extension_id, registered.view_id
        )),
    }
}

/// Raw is implicit: only files explicitly switched away from raw have an entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileViewSelectionState(Arc<BTreeMap<String, String>>);

impl FileViewSelectionState {
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        Self(Arc::new(entries.into_iter().collect()))
    }

    #[must_use]
    pub fn get(&self, file_id: &str) -> Option<&str> {
        self.0.get(file_id).map(String::as_str)
    }

    #[must_use]
    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.0
    }

    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[must_use]
pub fn reconcile_file_view_selections(
    current: &FileViewSelectionState,
    file_ids: &[String],
    view_keys: &BTreeSet<String>,
) -> FileViewSelectionState {
    let valid_files = file_ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let next = current
        .0
        .iter()
        .filter(|(file_id, view_key)| {
            valid_files.contains(file_id.as_str()) && view_keys.contains(view_key.as_str())
        })
        .map(|(file_id, view_key)| (file_id.clone(), view_key.clone()))
        .collect::<BTreeMap<_, _>>();
    if next.len() == current.0.len() {
        current.clone()
    } else {
        FileViewSelectionState(Arc::new(next))
    }
}

#[must_use]
pub fn select_file_view(
    current: &FileViewSelectionState,
    file_id: &str,
    view_key: Option<&str>,
) -> FileViewSelectionState {
    match view_key {
        None if !current.0.contains_key(file_id) => current.clone(),
        Some(view_key) if current.get(file_id) == Some(view_key) => current.clone(),
        None => {
            let mut next = current.0.as_ref().clone();
            next.remove(file_id);
            FileViewSelectionState(Arc::new(next))
        }
        Some(view_key) => {
            let mut next = current.0.as_ref().clone();
            next.insert(file_id.to_owned(), view_key.to_owned());
            FileViewSelectionState(Arc::new(next))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkFileViewTarget {
    pub key: String,
    pub file_ids: Vec<String>,
}

/// Resolve the matching set while the selected file still uses and matches the view.
#[must_use]
pub fn resolve_bulk_file_view_target<F, E>(
    current: &FileViewSelectionState,
    files: &[F],
    registered: &RegisteredFileView,
    selected_file_id: &str,
    mut file_id: impl FnMut(&F) -> &str,
    mut matches: impl FnMut(&RegisteredFileView, &F) -> Result<bool, E>,
) -> Option<BulkFileViewTarget> {
    let key = registered_file_view_key(registered);
    if current.get(selected_file_id) != Some(key.as_str()) {
        return None;
    }
    let file_ids = files
        .iter()
        .filter_map(|file| {
            matches(registered, file)
                .ok()
                .filter(|matches| *matches)
                .map(|_| file_id(file).to_owned())
        })
        .collect::<Vec<_>>();
    if !file_ids.iter().any(|id| id == selected_file_id) {
        return None;
    }
    file_ids
        .iter()
        .any(|id| current.get(id) != Some(key.as_str()))
        .then_some(BulkFileViewTarget { key, file_ids })
}

#[must_use]
pub fn select_file_view_for_files(
    current: &FileViewSelectionState,
    file_ids: &[String],
    view_key: &str,
) -> FileViewSelectionState {
    if file_ids
        .iter()
        .all(|file_id| current.get(file_id) == Some(view_key))
    {
        return current.clone();
    }
    let mut next = current.0.as_ref().clone();
    for file_id in file_ids {
        next.insert(file_id.clone(), view_key.to_owned());
    }
    FileViewSelectionState(Arc::new(next))
}

#[must_use]
pub fn file_view_layout_epoch(epochs: &ScopedEpochState, view_key: &str, file_id: &str) -> u64 {
    scoped_epoch(epochs, view_key, file_id)
}

#[must_use]
pub fn bump_file_view_epoch(
    current: &ScopedEpochState,
    view_key: &str,
    file_id: Option<&str>,
) -> ScopedEpochState {
    bump_scoped_epoch(current, view_key, file_id)
}

#[must_use]
pub fn reconcile_file_view_epochs(
    current: &ScopedEpochState,
    file_ids: &[String],
    view_keys: &BTreeSet<String>,
) -> ScopedEpochState {
    reconcile_scoped_epochs(current, file_ids, view_keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct File {
        id: String,
        path: String,
    }

    fn registered() -> RegisteredFileView {
        RegisteredFileView {
            extension_id: "preview".into(),
            view_id: "rendered".into(),
        }
    }

    #[test]
    fn keeps_valid_choices_across_reload_while_dropping_stale_ids_and_views() {
        let current = FileViewSelectionState::from_entries([
            ("readme".into(), "preview:rendered".into()),
            ("gone".into(), "other:view".into()),
            ("stale".into(), "removed:view".into()),
        ]);
        let next = reconcile_file_view_selections(
            &current,
            &["readme".into(), "stale".into()],
            &BTreeSet::from(["preview:rendered".into()]),
        );
        assert_eq!(
            next.entries(),
            &BTreeMap::from([("readme".into(), "preview:rendered".into())])
        );
    }

    #[test]
    fn preserves_selection_identity_when_reconciliation_removes_nothing() {
        let current =
            FileViewSelectionState::from_entries([("readme".into(), "preview:rendered".into())]);
        let next = reconcile_file_view_selections(
            &current,
            &["readme".into()],
            &BTreeSet::from(["preview:rendered".into()]),
        );
        assert!(next.ptr_eq(&current));
        let empty = FileViewSelectionState::default();
        assert!(reconcile_file_view_selections(&empty, &[], &BTreeSet::new()).ptr_eq(&empty));
    }

    #[test]
    fn stores_raw_implicitly_and_avoids_needless_state_changes() {
        let empty = FileViewSelectionState::default();
        let active = select_file_view(&empty, "readme", Some("preview:rendered"));
        assert_eq!(active.get("readme"), Some("preview:rendered"));
        assert!(select_file_view(&active, "readme", Some("preview:rendered")).ptr_eq(&active));
        assert!(
            select_file_view(&active, "readme", None)
                .entries()
                .is_empty()
        );
    }

    #[test]
    fn offers_a_bulk_target_only_while_the_selected_file_still_matches() {
        let registered = registered();
        let files = [
            File {
                id: "selected".into(),
                path: "selected.md".into(),
            },
            File {
                id: "other".into(),
                path: "other.md".into(),
            },
            File {
                id: "source".into(),
                path: "source.ts".into(),
            },
        ];
        let current =
            FileViewSelectionState::from_entries([("selected".into(), "preview:rendered".into())]);
        assert_eq!(
            resolve_bulk_file_view_target(
                &current,
                &files,
                &registered,
                "selected",
                |file| &file.id,
                |_, file| Ok::<_, ()>(file.path.ends_with(".md")),
            ),
            Some(BulkFileViewTarget {
                key: "preview:rendered".into(),
                file_ids: vec!["selected".into(), "other".into()],
            })
        );

        let nonmatching = [
            File {
                id: "selected".into(),
                path: "selected.bin".into(),
            },
            File {
                id: "other".into(),
                path: "other.md".into(),
            },
        ];
        assert_eq!(
            resolve_bulk_file_view_target(
                &current,
                &nonmatching,
                &registered,
                "selected",
                |file| &file.id,
                |_, file| Ok::<_, ()>(file.path.ends_with(".md")),
            ),
            None
        );
    }

    #[test]
    fn applies_one_view_to_matching_files_without_touching_nonmatches() {
        let current = FileViewSelectionState::from_entries([
            ("first".into(), "preview:old".into()),
            ("second".into(), "other:view".into()),
            ("untouched".into(), "raw:custom".into()),
        ]);
        let selected =
            select_file_view_for_files(&current, &["first".into(), "second".into()], "preview:new");
        assert_eq!(selected.get("first"), Some("preview:new"));
        assert_eq!(selected.get("second"), Some("preview:new"));
        assert_eq!(selected.get("untouched"), Some("raw:custom"));
        assert!(
            select_file_view_for_files(
                &selected,
                &["first".into(), "second".into()],
                "preview:new",
            )
            .ptr_eq(&selected)
        );
        assert!(select_file_view_for_files(&current, &[], "preview:new").ptr_eq(&current));
    }

    #[test]
    fn counts_view_wide_and_file_scoped_refreshes_without_collisions() {
        let first = bump_file_view_epoch(&ScopedEpochState::default(), "preview:rendered", None);
        assert_eq!(
            file_view_layout_epoch(&first, "preview:rendered", "readme"),
            1
        );
        let second = bump_file_view_epoch(&first, "preview:rendered", None);
        assert!(!second.ptr_eq(&first));
        assert_eq!(
            file_view_layout_epoch(&second, "preview:rendered", "readme"),
            2
        );
        let other = bump_file_view_epoch(&second, "other:view", None);
        assert_eq!(
            file_view_layout_epoch(&other, "preview:rendered", "readme"),
            2
        );
        let scoped = bump_file_view_epoch(&second, "preview:rendered", Some("readme"));
        assert_eq!(
            file_view_layout_epoch(&scoped, "preview:rendered", "readme"),
            3
        );
        assert_eq!(
            file_view_layout_epoch(&scoped, "preview:rendered", "other"),
            2
        );

        let control = bump_file_view_epoch(&ScopedEpochState::default(), "preview:bad\0view", None);
        let shaped = bump_file_view_epoch(&control, "preview:bad", Some("view"));
        assert_eq!(
            file_view_layout_epoch(&shaped, "preview:bad\0view", "other"),
            1
        );
        assert_eq!(file_view_layout_epoch(&shaped, "preview:bad", "view"), 1);
        assert_eq!(file_view_layout_epoch(&shaped, "preview:bad", "other"), 0);
        assert_eq!(
            file_view_layout_epoch(&ScopedEpochState::default(), "preview:rendered", "readme",),
            0
        );
    }

    #[test]
    fn drops_orphaned_epochs_and_preserves_identity_otherwise() {
        let current = bump_file_view_epoch(
            &bump_file_view_epoch(
                &bump_file_view_epoch(
                    &bump_file_view_epoch(&ScopedEpochState::default(), "preview:rendered", None),
                    "gone:view",
                    None,
                ),
                "preview:rendered",
                Some("readme"),
            ),
            "preview:rendered",
            Some("deleted"),
        );
        let kept = reconcile_file_view_epochs(
            &current,
            &["readme".into()],
            &BTreeSet::from(["preview:rendered".into()]),
        );
        assert_eq!(
            file_view_layout_epoch(&kept, "preview:rendered", "readme"),
            2
        );
        assert_eq!(
            file_view_layout_epoch(&kept, "preview:rendered", "deleted"),
            1
        );
        assert_eq!(file_view_layout_epoch(&kept, "gone:view", "readme"), 0);
        let unchanged = reconcile_file_view_epochs(
            &current,
            &["readme".into(), "deleted".into()],
            &BTreeSet::from(["preview:rendered".into(), "gone:view".into()]),
        );
        assert!(unchanged.ptr_eq(&current));
    }

    #[test]
    fn names_every_reason_a_view_cannot_be_selected() {
        let file = File {
            id: "readme".into(),
            path: "README.md".into(),
        };
        let registered = registered();
        assert_eq!(
            resolve_file_view_selection_target(
                "preview",
                &file,
                Some(&registered),
                None,
                "rendered",
                |_, _| Ok::<_, ()>(true),
            ),
            FileViewSelectionTarget::Registered(&registered)
        );
        assert_eq!(
            resolve_file_view_selection_target(
                "preview",
                &file,
                None,
                None,
                "rendered",
                |_, _| Ok::<_, ()>(true),
            ),
            FileViewSelectionTarget::Refused(
                "Extension preview targeted unknown file view \"rendered\"".into()
            )
        );
        assert_eq!(
            resolve_file_view_selection_target(
                "preview",
                &file,
                Some(&registered),
                None,
                "rendered",
                |_, _| Ok::<_, ()>(false),
            ),
            FileViewSelectionTarget::Refused(
                "File view \"rendered\" does not match the selected file • using raw diff".into()
            )
        );
        assert_eq!(
            resolve_file_view_selection_target(
                "preview",
                &file,
                Some(&registered),
                None,
                "rendered",
                |_, _| Err("matcher exploded"),
            ),
            FileViewSelectionTarget::Refused(
                "Extension preview file view \"rendered\" failed matching the selected file".into()
            )
        );
        assert_eq!(
            resolve_file_view_selection_target(
                "preview",
                &file,
                Some(&registered),
                Some("raw only"),
                "rendered",
                |_, _| Ok::<_, ()>(true),
            ),
            FileViewSelectionTarget::Refused("raw only".into())
        );
    }

    #[test]
    fn allows_a_view_named_raw_because_none_is_the_only_raw_sentinel() {
        let raw = RegisteredFileView {
            extension_id: "preview".into(),
            view_id: "raw".into(),
        };
        assert_eq!(registered_file_view_key(&raw), "preview:raw");
        assert_eq!(
            resolve_registered_file_view(std::slice::from_ref(&raw), "preview", "raw"),
            Some(&raw)
        );
        assert_eq!(
            resolve_registered_file_view(std::slice::from_ref(&raw), "other", "preview:raw"),
            Some(&raw)
        );
    }
}
