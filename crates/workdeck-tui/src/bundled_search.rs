//! The bundled `/` content search as a host-side extension tier.
//!
//! Mirrors Hunk's bundled `search` factory: `/` opens the status-line prompt,
//! Enter jumps to the next hunk matching the typed text, and `n` / `N` repeat
//! it in either direction, wrapping. The status row then reports where the
//! review landed, and the registered line highlighter marks every match inside
//! the diff with the landed line as the one current mark. The session is
//! process-wide, so each command hands it the visible files from its selection
//! instead of closing over a review.

use std::sync::{Arc, Mutex};

use workdeck_core::DiffFile;
use workdeck_extension_api::{
    BUNDLED_SEARCH_FIND_COMMAND_ID, BUNDLED_SEARCH_HIGHLIGHTER_ID, BUNDLED_SEARCH_NEXT_COMMAND_ID,
    BUNDLED_SEARCH_PREVIOUS_COMMAND_ID, BUNDLED_SEARCH_STATUS_ITEM_ID, ExtensionDiffFile,
    ExtensionDiffStats, ExtensionFileSide, ExtensionKeyEvent, ExtensionStatusAlignment,
    SearchPosition, SearchSession, WORKDECK_VENDOR_EXTENSION_ID, bundled_search_commands,
    search_marks_wire_value,
};

use crate::{
    BundledCommandClaim, CommandKeyDefaults, LineHighlightRuntime, LineHighlightRuntimeError,
    ResolvedKeymap, StatusItem, StatusPromptOptions, StatusPromptOwner, StatusPromptRequestOptions,
    matches_any_key_chord,
};

/// One bundled search command, as the dispatch table resolves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundledSearchCommand {
    Find,
    Next,
    Previous,
}

impl BundledSearchCommand {
    #[must_use]
    pub fn full_id(self) -> &'static str {
        match self {
            Self::Find => BUNDLED_SEARCH_FIND_COMMAND_ID,
            Self::Next => BUNDLED_SEARCH_NEXT_COMMAND_ID,
            Self::Previous => BUNDLED_SEARCH_PREVIOUS_COMMAND_ID,
        }
    }

    /// Resolve one fully qualified bundled search command id.
    #[must_use]
    pub fn from_full_id(id: &str) -> Option<Self> {
        match id {
            BUNDLED_SEARCH_FIND_COMMAND_ID => Some(Self::Find),
            BUNDLED_SEARCH_NEXT_COMMAND_ID => Some(Self::Next),
            BUNDLED_SEARCH_PREVIOUS_COMMAND_ID => Some(Self::Previous),
            _ => None,
        }
    }
}

/// The bundled search commands' keymap defaults, namespaced for user config.
#[must_use]
pub fn bundled_search_command_defaults() -> Vec<CommandKeyDefaults> {
    bundled_search_commands()
        .iter()
        .map(|command| CommandKeyDefaults {
            id: format!("{WORKDECK_VENDOR_EXTENSION_ID}.{}", command.id),
            aliases: Vec::new(),
            default_keys: command.default_keys.clone(),
        })
        .collect()
}

/// The chords one bundled command answers to after user bindings fold in.
#[must_use]
fn resolved_keys(resolved: &ResolvedKeymap, command: BundledSearchCommand) -> Vec<String> {
    let default = bundled_search_command_defaults()
        .into_iter()
        .find(|entry| entry.id == command.full_id())
        .map(|entry| entry.default_keys)
        .unwrap_or_default();
    resolved
        .keys
        .get(command.full_id())
        .cloned()
        .unwrap_or(default)
}

/// Resolve one key event to the bundled search command it invokes, if any.
#[must_use]
pub fn dispatch_bundled_search_command(
    resolved: &ResolvedKeymap,
    key: &ExtensionKeyEvent,
) -> Option<BundledSearchCommand> {
    [
        BundledSearchCommand::Find,
        BundledSearchCommand::Next,
        BundledSearchCommand::Previous,
    ]
    .into_iter()
    .find(|command| matches_any_key_chord(&resolved_keys(resolved, *command)).matches(key))
}

/// One bundled search command's menu/help projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledSearchCommandView {
    pub id: &'static str,
    pub title: &'static str,
    pub key_labels: Vec<String>,
}

/// The bundled search commands as menus and help see them, in registration order.
#[must_use]
pub fn bundled_search_command_views(resolved: &ResolvedKeymap) -> Vec<BundledSearchCommandView> {
    [
        BundledSearchCommand::Find,
        BundledSearchCommand::Next,
        BundledSearchCommand::Previous,
    ]
    .into_iter()
    .map(|command| BundledSearchCommandView {
        id: command.full_id(),
        title: match command {
            BundledSearchCommand::Find => "Search diff content",
            BundledSearchCommand::Next => "Next search match",
            BundledSearchCommand::Previous => "Previous search match",
        },
        key_labels: resolved_keys(resolved, command)
            .iter()
            .map(|key| crate::format_key_chord(key))
            .collect(),
    })
    .collect()
}

/// The bundled search commands' chord claims in the extension dispatch table.
///
/// A user extension command holding one of these chords only by default gives
/// it up: bundled commands win a chord conflict, like built-ins.
#[must_use]
pub fn bundled_search_command_claims(resolved: &ResolvedKeymap) -> Vec<BundledCommandClaim> {
    [
        BundledSearchCommand::Find,
        BundledSearchCommand::Next,
        BundledSearchCommand::Previous,
    ]
    .into_iter()
    .map(|command| BundledCommandClaim {
        id: command.full_id().to_owned(),
        keys: resolved_keys(resolved, command),
    })
    .collect()
}

/// The status-row key of the bundled search's persistent report item.
///
/// Not an `ext:` item: the search is Workdeck's own tier, needs no attribution,
/// and outlives user-extension registry replacement the way the filter does.
#[must_use]
pub fn bundled_search_status_item_key() -> String {
    format!("{WORKDECK_VENDOR_EXTENSION_ID}:{BUNDLED_SEARCH_STATUS_ITEM_ID}")
}

/// One search outcome's status-row report.
#[must_use]
pub fn bundled_search_status_item(
    spans: Vec<workdeck_extension_api::ExtensionStatusSpan>,
) -> StatusItem {
    StatusItem {
        id: bundled_search_status_item_key(),
        spans,
        alignment: ExtensionStatusAlignment::Left,
        priority: 1,
    }
}

/// Options for the `/` prompt, prefilled with the session's live query.
#[must_use]
pub fn bundled_search_prompt_options(initial: &str) -> StatusPromptOptions {
    StatusPromptOptions {
        prefix: "/".into(),
        placeholder: "search diff".into(),
        initial: initial.to_owned(),
        on_change: None,
    }
}

/// Request options for the `/` prompt: unattributed, like every host surface.
#[must_use]
pub fn bundled_search_prompt_request_options() -> StatusPromptRequestOptions {
    StatusPromptRequestOptions::default()
}

/// Who owns the `/` prompt, so settlement reaches the search session.
#[must_use]
pub fn bundled_search_prompt_owner() -> StatusPromptOwner {
    StatusPromptOwner::Vendor
}

/// The frozen context a `/` press captured for the search it may run.
#[derive(Debug, Clone, Default)]
pub struct PendingBundledSearch {
    /// Where the review was pointing when `/` was pressed.
    pub position: SearchPosition,
    /// The visible files frozen at `/`, exactly as a command context would carry them.
    pub files: Vec<ExtensionDiffFile>,
}

/// Shared session plus the highlighter runtime face the pipeline drives.
///
/// The pipeline calls the highlighter from background workers, so the session
/// lives behind the same mutex the command handlers use; every derivation stays
/// a pure function of the session's query and current target.
#[derive(Debug, Clone)]
pub struct BundledSearchRuntime {
    session: Arc<Mutex<SearchSession>>,
}

impl BundledSearchRuntime {
    #[must_use]
    pub fn new(session: Arc<Mutex<SearchSession>>) -> Self {
        Self { session }
    }

    #[must_use]
    pub fn session(&self) -> &Arc<Mutex<SearchSession>> {
        &self.session
    }

    fn file_view(file: &DiffFile) -> ExtensionDiffFile {
        // Only identity and patch text feed the marks; skip the full projection.
        ExtensionDiffFile {
            id: file.runtime_id.clone(),
            patch: file.patch.clone(),
            path: file.path.clone(),
            previous_path: None,
            language: None,
            stats: ExtensionDiffStats {
                additions: 0,
                deletions: 0,
            },
            metadata: serde_json::Value::Null,
            change_type: None,
            stats_truncated: false,
            hunks: Vec::new(),
            agent: None,
            is_untracked: false,
            is_binary: file.flags.binary,
            is_too_large: file.flags.too_large,
        }
    }
}

impl LineHighlightRuntime for BundledSearchRuntime {
    fn request_pending(&self) -> bool {
        false
    }

    fn highlight_file(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        _cancelled: &workdeck_extension_host::ExtensionRequestCancellation,
    ) -> Result<serde_json::Value, LineHighlightRuntimeError> {
        self.highlight_file_with_reader(
            highlighter_id,
            file,
            _cancelled,
            workdeck_extension_host::ExtensionDocumentReader::new(|_| Ok(None)),
        )
    }

    fn highlight_file_with_reader(
        &self,
        highlighter_id: &str,
        file: &DiffFile,
        _cancelled: &workdeck_extension_host::ExtensionRequestCancellation,
        _reader: workdeck_extension_host::ExtensionDocumentReader,
    ) -> Result<serde_json::Value, LineHighlightRuntimeError> {
        if highlighter_id != BUNDLED_SEARCH_HIGHLIGHTER_ID {
            return Err(LineHighlightRuntimeError::Failed(format!(
                "unknown bundled highlighter {highlighter_id:?}"
            )));
        }
        let session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let marks = session.marks_for(&Self::file_view(file));
        Ok(search_marks_wire_value(&marks.unwrap_or_default()))
    }

    fn notify_warning(&self, _message: String) {
        // The bundled runtime derives synchronously and reports its own
        // outcomes on the status row; there is no extension to notify.
    }
}

/// The search position the live selection describes, as the session compares it.
#[must_use]
pub fn bundled_search_position(
    selection: &workdeck_extension_api::ExtensionReviewSelection,
) -> SearchPosition {
    SearchPosition {
        file_id: selection.file.as_ref().map(|file| file.id.clone()),
        hunk_index: selection.hunk_index,
    }
}

/// Convert a match's side for `revealLine` navigation.
#[must_use]
pub fn bundled_search_side(side: ExtensionFileSide) -> workdeck_core::ReviewSide {
    match side {
        ExtensionFileSide::Old => workdeck_core::ReviewSide::Old,
        ExtensionFileSide::New => workdeck_core::ReviewSide::New,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StatusLineStore, UserKeyBinding, UserKeyBindingEntry, resolve_command_keys};
    use workdeck_extension_api::SearchSessionOptions;

    fn chord_event(chord: &str) -> ExtensionKeyEvent {
        let parsed = workdeck_extension_api::parse_key_chord(chord).unwrap();
        crate::synthesize_key_event(&parsed).key
    }

    fn resolved(bindings: &[UserKeyBindingEntry]) -> ResolvedKeymap {
        let mut defaults = crate::builtin_command_key_defaults();
        defaults.extend(bundled_search_command_defaults());
        resolve_command_keys(&defaults, bindings)
    }

    #[test]
    fn commands_ship_on_slash_n_and_capital_n_under_the_vendor_namespace() {
        let defaults = bundled_search_command_defaults();
        assert_eq!(
            defaults
                .iter()
                .map(|entry| (entry.id.as_str(), entry.default_keys.clone()))
                .collect::<Vec<_>>(),
            [
                (BUNDLED_SEARCH_FIND_COMMAND_ID, vec!["/".to_owned()]),
                (BUNDLED_SEARCH_NEXT_COMMAND_ID, vec!["n".to_owned()]),
                (BUNDLED_SEARCH_PREVIOUS_COMMAND_ID, vec!["N".to_owned()]),
            ]
        );
        let keymap = resolved(&[]);
        assert_eq!(
            dispatch_bundled_search_command(&keymap, &chord_event("/")),
            Some(BundledSearchCommand::Find)
        );
        assert_eq!(
            dispatch_bundled_search_command(&keymap, &chord_event("n")),
            Some(BundledSearchCommand::Next)
        );
        assert_eq!(
            dispatch_bundled_search_command(&keymap, &chord_event("N")),
            Some(BundledSearchCommand::Previous)
        );
    }

    #[test]
    fn a_filter_remapping_takes_slash_back_from_search_but_keeps_n_and_n() {
        let keymap = resolved(&[UserKeyBindingEntry::new(
            "workdeck.review.focusFilter",
            UserKeyBinding::Chord("/".into()),
        )]);
        assert_eq!(
            dispatch_bundled_search_command(&keymap, &chord_event("/")),
            None,
            "an exclusive user binding hands / to the filter"
        );
        assert_eq!(
            dispatch_bundled_search_command(&keymap, &chord_event("n")),
            Some(BundledSearchCommand::Next)
        );

        let moved = resolved(&[UserKeyBindingEntry::new(
            BUNDLED_SEARCH_FIND_COMMAND_ID,
            UserKeyBinding::Chord("ctrl+s".into()),
        )]);
        assert_eq!(
            dispatch_bundled_search_command(&moved, &chord_event("ctrl+s")),
            Some(BundledSearchCommand::Find)
        );
        assert_eq!(
            dispatch_bundled_search_command(&moved, &chord_event("/")),
            None,
            "user bindings replace the shipped defaults"
        );
    }

    #[test]
    fn menu_and_help_views_carry_the_registration_order_and_labels() {
        let views = bundled_search_command_views(&resolved(&[]));
        assert_eq!(
            views
                .iter()
                .map(|view| (view.id, view.title, view.key_labels.clone()))
                .collect::<Vec<_>>(),
            [
                (
                    BUNDLED_SEARCH_FIND_COMMAND_ID,
                    "Search diff content",
                    vec!["/".to_owned()]
                ),
                (
                    BUNDLED_SEARCH_NEXT_COMMAND_ID,
                    "Next search match",
                    vec!["n".to_owned()]
                ),
                (
                    BUNDLED_SEARCH_PREVIOUS_COMMAND_ID,
                    "Previous search match",
                    vec!["N".to_owned()]
                ),
            ]
        );
    }

    #[test]
    fn the_search_prompt_opens_unattributed_with_the_live_query() {
        let options = bundled_search_prompt_options("needle");
        assert_eq!(options.prefix, "/");
        assert_eq!(options.placeholder, "search diff");
        assert_eq!(options.initial, "needle");
        assert!(options.on_change.is_none());

        let mut store = StatusLineStore::new();
        let id = store
            .open_prompt(
                bundled_search_prompt_options(""),
                bundled_search_prompt_request_options(),
                bundled_search_prompt_owner(),
            )
            .unwrap();
        let prompt = store.snapshot().prompt.unwrap();
        assert_eq!(prompt.prefix, "/");
        assert_eq!(prompt.placeholder, "search diff");
        assert!(prompt.attribution.is_none());
        assert_eq!(
            store.current_prompt_owner(),
            Some(StatusPromptOwner::Vendor)
        );
        drop(store.submit_prompt(id).unwrap());
    }

    #[test]
    fn the_status_item_is_host_owned_and_survives_extension_registry_turnover() {
        let item = bundled_search_status_item(Vec::new());
        assert_eq!(item.id, "workdeck:search.status");
        assert_eq!(item.priority, 1);
        assert!(!workdeck_extension_host::is_extension_status_item_id(
            &item.id
        ));
    }

    fn diff_file(patch: &str) -> DiffFile {
        let source = format!("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n{patch}");
        let mut document = workdeck_diff::parse_patch(
            &source,
            "test",
            "test",
            workdeck_core::ChangesetSource::Patch {
                label: "test".into(),
            },
        )
        .unwrap();
        document.files.remove(0)
    }

    #[test]
    fn the_highlight_runtime_serves_session_marks_in_the_wire_shape() {
        let session = Arc::new(Mutex::new(SearchSession::new(
            SearchSessionOptions::default(),
        )));
        let file = diff_file("@@ -1 +1 @@\n-old readConfig()\n+new readConfig()\n");
        let runtime = BundledSearchRuntime::new(Arc::clone(&session));

        let idle = runtime
            .highlight_file(
                BUNDLED_SEARCH_HIGHLIGHTER_ID,
                &file,
                &workdeck_extension_host::ExtensionRequestCancellation::default(),
            )
            .unwrap();
        assert_eq!(idle, serde_json::json!([]));

        session.lock().unwrap().search(
            "readConfig",
            &[BundledSearchRuntime::file_view(&file)],
            &SearchPosition::default(),
        );
        let marks = runtime
            .highlight_file(
                BUNDLED_SEARCH_HIGHLIGHTER_ID,
                &file,
                &workdeck_extension_host::ExtensionRequestCancellation::default(),
            )
            .unwrap();
        assert_eq!(
            marks,
            serde_json::json!([
                {"side": "old", "line": 1, "range": [4, 14], "tone": "current"},
                {"side": "new", "line": 1, "range": [4, 14], "tone": "match"},
            ])
        );
        assert!(
            runtime
                .highlight_file(
                    "other",
                    &file,
                    &workdeck_extension_host::ExtensionRequestCancellation::default(),
                )
                .is_err()
        );
        assert!(!runtime.request_pending());
    }
}
