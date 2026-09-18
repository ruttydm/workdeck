use std::sync::Arc;

use workdeck_extension_api::{ExtensionDiffFile, ExtensionKeyEvent};

use crate::{
    FileViewSelectionTarget, RegisteredFileView, SynchronousCallbackValue,
    SynchronousExtensionCallbackResult, call_extension_synchronously, registered_file_view_key,
    resolve_file_view_selection_target,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileViewModeContext {
    pub cwd: String,
    pub file: Arc<ExtensionDiffFile>,
}

/// The one native file-view mode a session can have active.
#[derive(Debug, Clone)]
pub struct ActiveFileViewMode {
    pub extension_id: String,
    pub view_id: String,
    pub view_key: String,
    pub file_id: String,
    pub registered: Arc<RegisteredFileView>,
    pub context: FileViewModeContext,
    pub review_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileViewModeSelection {
    pub file_id: String,
    pub view_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileViewModeActivation<'a> {
    Ready {
        registered: &'a RegisteredFileView,
        select: Option<FileViewModeSelection>,
    },
    Refused(String),
}

#[derive(Debug, Clone, Copy)]
pub struct FileViewModeActivationInputs<'a, F> {
    pub active_view_key: Option<&'a str>,
    pub extension_id: &'a str,
    pub file: Option<&'a F>,
    pub registered: Option<&'a RegisteredFileView>,
    pub unavailable_reason: Option<&'a str>,
    pub view_id: &'a str,
}

/// Resolve a mode activation and the atomic presentation selection it requires.
pub fn resolve_file_view_mode_activation<'a, F, E>(
    inputs: FileViewModeActivationInputs<'a, F>,
    file_id: impl FnOnce(&F) -> &str,
    matches: impl FnOnce(&RegisteredFileView, &F) -> Result<bool, E>,
) -> FileViewModeActivation<'a> {
    let Some(file) = inputs.file else {
        return FileViewModeActivation::Refused(format!(
            "Extension {} cannot enter a mode without a selected file",
            inputs.extension_id
        ));
    };
    let registered = match resolve_file_view_selection_target(
        inputs.extension_id,
        file,
        inputs.registered,
        inputs.unavailable_reason,
        inputs.view_id,
        matches,
    ) {
        FileViewSelectionTarget::Registered(registered) => registered,
        FileViewSelectionTarget::Refused(reason) => {
            return FileViewModeActivation::Refused(reason);
        }
    };
    if !registered.interactive_mode {
        return FileViewModeActivation::Refused(format!(
            "Extension {} file view \"{}\" has no interactive mode",
            inputs.extension_id, inputs.view_id
        ));
    }
    let view_key = registered_file_view_key(registered);
    let select =
        (inputs.active_view_key != Some(view_key.as_str())).then(|| FileViewModeSelection {
            file_id: file_id(file).to_owned(),
            view_key,
        });
    FileViewModeActivation::Ready { registered, select }
}

/// Whether a mode still describes the file, view, generation, and registration on screen.
#[must_use]
pub fn file_view_mode_still_valid(
    active: &ActiveFileViewMode,
    active_view_key: Option<&str>,
    review_generation: u64,
    selected_file_id: Option<&str>,
    views: &[Arc<RegisteredFileView>],
) -> bool {
    selected_file_id == Some(active.file_id.as_str())
        && active_view_key == Some(active.view_key.as_str())
        && active.review_generation == review_generation
        && views
            .iter()
            .any(|registered| Arc::ptr_eq(registered, &active.registered))
}

#[must_use]
pub fn file_view_mode_status_hint(active: &ActiveFileViewMode) -> String {
    format!(
        "{}:{} mode — Esc exits",
        active.extension_id, active.view_id
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileViewModeLifecyclePhase {
    OnEnter,
    OnExit,
}

impl FileViewModeLifecyclePhase {
    const fn label(self) -> &'static str {
        match self {
            Self::OnEnter => "onEnter",
            Self::OnExit => "onExit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeModeCallbackError {
    Failed(String),
    MustReturnSynchronously,
}

#[must_use]
pub fn file_view_mode_failure_message(
    extension_id: &str,
    view_id: &str,
    action: &str,
    detail: &str,
) -> String {
    format!(
        "Extension {} file view \"{}\" mode failed {action} • {detail}",
        extension_id, view_id
    )
}

fn format_file_view_mode_failure(
    active: &ActiveFileViewMode,
    action: &str,
    detail: &str,
) -> String {
    file_view_mode_failure_message(&active.extension_id, &active.view_id, action, detail)
}

/// Run one lifecycle callback and contain a child failure as a warning.
pub fn run_file_view_mode_lifecycle(
    active: &ActiveFileViewMode,
    phase: FileViewModeLifecyclePhase,
    has_callback: bool,
    callback: impl FnOnce(&ActiveFileViewMode) -> Result<(), NativeModeCallbackError>,
    mut notify: impl FnMut(String),
) -> bool {
    if !has_callback {
        return true;
    }
    let result = call_extension_synchronously(|| match callback(active) {
        Ok(()) => Ok(SynchronousCallbackValue::Returned(())),
        Err(NativeModeCallbackError::Failed(detail)) => Err(detail),
        Err(NativeModeCallbackError::MustReturnSynchronously) => {
            Ok(SynchronousCallbackValue::Thenable)
        }
    });
    match result {
        SynchronousExtensionCallbackResult::Returned(()) => true,
        SynchronousExtensionCallbackResult::Thenable => {
            let detail = format!("{} must return synchronously", phase.label());
            notify(format_file_view_mode_failure(
                active,
                phase.label(),
                &detail,
            ));
            false
        }
        SynchronousExtensionCallbackResult::Threw(detail) => {
            notify(format_file_view_mode_failure(
                active,
                phase.label(),
                &detail,
            ));
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileViewModeKeyResult {
    Handled,
    Pass,
    Exit,
}

/// Normalize one native mode key response; protocol failures exit the mode.
pub fn deliver_file_view_mode_key(
    active: &ActiveFileViewMode,
    key: &ExtensionKeyEvent,
    callback: impl FnOnce(
        &ActiveFileViewMode,
        &ExtensionKeyEvent,
    ) -> Result<Option<FileViewModeKeyResult>, NativeModeCallbackError>,
    mut notify: impl FnMut(String),
) -> FileViewModeKeyResult {
    let result = call_extension_synchronously(|| match callback(active, key) {
        Ok(result) => Ok(SynchronousCallbackValue::Returned(result)),
        Err(NativeModeCallbackError::Failed(detail)) => Err(detail),
        Err(NativeModeCallbackError::MustReturnSynchronously) => {
            Ok(SynchronousCallbackValue::Thenable)
        }
    });
    match result {
        SynchronousExtensionCallbackResult::Returned(Some(result)) => result,
        SynchronousExtensionCallbackResult::Returned(None) => FileViewModeKeyResult::Pass,
        SynchronousExtensionCallbackResult::Thenable => {
            notify(format_file_view_mode_failure(
                active,
                "onKey",
                "onKey must return synchronously",
            ));
            FileViewModeKeyResult::Exit
        }
        SynchronousExtensionCallbackResult::Threw(detail) => {
            notify(format_file_view_mode_failure(active, "onKey", &detail));
            FileViewModeKeyResult::Exit
        }
    }
}

#[cfg(test)]
mod tests {
    use workdeck_extension_api::ExtensionDiffStats;

    use super::*;

    fn registered(interactive_mode: bool) -> Arc<RegisteredFileView> {
        Arc::new(RegisteredFileView {
            extension_id: "preview".into(),
            view_id: "rendered".into(),
            interactive_mode,
        })
    }

    fn file(path: &str) -> ExtensionDiffFile {
        ExtensionDiffFile {
            id: "readme".into(),
            path: path.into(),
            previous_path: None,
            patch: String::new(),
            language: None,
            stats: ExtensionDiffStats {
                additions: 0,
                deletions: 0,
            },
            metadata: serde_json::json!({ "hunks": [] }),
            change_type: Some(workdeck_extension_api::ExtensionVcsFileChangeType::Change),
            stats_truncated: false,
            hunks: Vec::new(),
            agent: None,
            is_untracked: false,
            is_binary: false,
            is_too_large: false,
        }
    }

    fn active() -> ActiveFileViewMode {
        ActiveFileViewMode {
            extension_id: "preview".into(),
            view_id: "rendered".into(),
            view_key: "preview:rendered".into(),
            file_id: "readme".into(),
            registered: registered(true),
            context: FileViewModeContext {
                cwd: "/repo".into(),
                file: Arc::new(file("README.md")),
            },
            review_generation: 1,
        }
    }

    #[test]
    fn names_each_activation_refusal() {
        let file = file("README.md");
        let missing = resolve_file_view_mode_activation(
            FileViewModeActivationInputs {
                active_view_key: Some("preview:rendered"),
                extension_id: "preview",
                file: Some(&file),
                registered: None,
                unavailable_reason: None,
                view_id: "missing",
            },
            |file| &file.id,
            |_, _| Ok::<_, ()>(true),
        );
        assert_eq!(
            missing,
            FileViewModeActivation::Refused(
                "Extension preview targeted unknown file view \"missing\"".into()
            )
        );

        let plain = registered(false);
        assert_eq!(
            resolve_file_view_mode_activation(
                FileViewModeActivationInputs {
                    active_view_key: Some("preview:plain"),
                    extension_id: "preview",
                    file: Some(&file),
                    registered: Some(&plain),
                    unavailable_reason: None,
                    view_id: "rendered",
                },
                |file| &file.id,
                |_, _| Ok::<_, ()>(true),
            ),
            FileViewModeActivation::Refused(
                "Extension preview file view \"rendered\" has no interactive mode".into()
            )
        );

        let mode = registered(true);
        assert_eq!(
            resolve_file_view_mode_activation(
                FileViewModeActivationInputs {
                    active_view_key: Some("preview:rendered"),
                    extension_id: "preview",
                    file: None::<&ExtensionDiffFile>,
                    registered: Some(&mode),
                    unavailable_reason: None,
                    view_id: "rendered",
                },
                |file| &file.id,
                |_, _| Ok::<_, ()>(true),
            ),
            FileViewModeActivation::Refused(
                "Extension preview cannot enter a mode without a selected file".into()
            )
        );
        assert_eq!(
            resolve_file_view_mode_activation(
                FileViewModeActivationInputs {
                    active_view_key: None,
                    extension_id: "preview",
                    file: Some(&file),
                    registered: Some(&mode),
                    unavailable_reason: None,
                    view_id: "rendered",
                },
                |file| &file.id,
                |_, _| Ok::<_, ()>(false),
            ),
            FileViewModeActivation::Refused(
                "File view \"rendered\" does not match the selected file • using raw diff".into()
            )
        );
        assert_eq!(
            resolve_file_view_mode_activation(
                FileViewModeActivationInputs {
                    active_view_key: None,
                    extension_id: "preview",
                    file: Some(&file),
                    registered: Some(&mode),
                    unavailable_reason: None,
                    view_id: "rendered",
                },
                |file| &file.id,
                |_, _| Err("matcher exploded"),
            ),
            FileViewModeActivation::Refused(
                "Extension preview file view \"rendered\" failed matching the selected file".into()
            )
        );
        assert_eq!(
            resolve_file_view_mode_activation(
                FileViewModeActivationInputs {
                    active_view_key: None,
                    extension_id: "preview",
                    file: Some(&file),
                    registered: Some(&mode),
                    unavailable_reason: Some("File presentations are unavailable • using raw diff",),
                    view_id: "rendered",
                },
                |file| &file.id,
                |_, _| Ok::<_, ()>(true),
            ),
            FileViewModeActivation::Refused(
                "File presentations are unavailable • using raw diff".into()
            )
        );
    }

    #[test]
    fn enters_atomically_and_selects_the_view_when_needed() {
        let file = file("README.md");
        let registered = registered(true);
        assert_eq!(
            resolve_file_view_mode_activation(
                FileViewModeActivationInputs {
                    active_view_key: Some("preview:rendered"),
                    extension_id: "preview",
                    file: Some(&file),
                    registered: Some(&registered),
                    unavailable_reason: None,
                    view_id: "rendered",
                },
                |file| &file.id,
                |_, _| Ok::<_, ()>(true),
            ),
            FileViewModeActivation::Ready {
                registered: &registered,
                select: None,
            }
        );
        for active_view in [None, Some("preview:plain")] {
            assert_eq!(
                resolve_file_view_mode_activation(
                    FileViewModeActivationInputs {
                        active_view_key: active_view,
                        extension_id: "preview",
                        file: Some(&file),
                        registered: Some(&registered),
                        unavailable_reason: None,
                        view_id: "rendered",
                    },
                    |file| &file.id,
                    |_, _| Ok::<_, ()>(true),
                ),
                FileViewModeActivation::Ready {
                    registered: &registered,
                    select: Some(FileViewModeSelection {
                        file_id: "readme".into(),
                        view_key: "preview:rendered".into(),
                    }),
                }
            );
        }
    }

    #[test]
    fn remains_active_only_for_the_same_review_file_view_and_registration() {
        let active = active();
        let views = [Arc::clone(&active.registered)];
        assert!(file_view_mode_still_valid(
            &active,
            Some("preview:rendered"),
            1,
            Some("readme"),
            &views,
        ));
        assert!(!file_view_mode_still_valid(
            &active,
            Some("preview:rendered"),
            1,
            Some("other"),
            &views,
        ));
        assert!(!file_view_mode_still_valid(
            &active,
            None,
            1,
            Some("readme"),
            &views,
        ));
        assert!(!file_view_mode_still_valid(
            &active,
            Some("preview:rendered"),
            2,
            Some("readme"),
            &views,
        ));
        assert!(!file_view_mode_still_valid(
            &active,
            Some("preview:rendered"),
            1,
            Some("readme"),
            &[registered(true)],
        ));
    }

    #[test]
    fn status_hint_names_the_mode_and_escape_path() {
        assert_eq!(
            file_view_mode_status_hint(&active()),
            "preview:rendered mode — Esc exits"
        );
    }

    #[test]
    fn lifecycle_callbacks_contain_failures_and_protocol_violations() {
        let active = active();
        let mut warnings = Vec::new();
        let mut entered = Vec::new();
        assert!(run_file_view_mode_lifecycle(
            &active,
            FileViewModeLifecyclePhase::OnEnter,
            true,
            |active| {
                entered.push(active.context.file.id.clone());
                Ok(())
            },
            |warning| warnings.push(warning),
        ));
        assert_eq!(entered, ["readme"]);
        assert!(run_file_view_mode_lifecycle(
            &active,
            FileViewModeLifecyclePhase::OnExit,
            false,
            |_| Err(NativeModeCallbackError::Failed("not called".into())),
            |warning| warnings.push(warning),
        ));
        assert!(warnings.is_empty());

        assert!(!run_file_view_mode_lifecycle(
            &active,
            FileViewModeLifecyclePhase::OnExit,
            true,
            |_| Err(NativeModeCallbackError::Failed("teardown exploded".into())),
            |warning| warnings.push(warning),
        ));
        assert!(!run_file_view_mode_lifecycle(
            &active,
            FileViewModeLifecyclePhase::OnEnter,
            true,
            |_| Err(NativeModeCallbackError::MustReturnSynchronously),
            |warning| warnings.push(warning),
        ));
        assert_eq!(
            warnings,
            [
                "Extension preview file view \"rendered\" mode failed onExit • teardown exploded",
                "Extension preview file view \"rendered\" mode failed onEnter • onEnter must return synchronously",
            ]
        );
    }

    #[test]
    fn key_delivery_normalizes_answers_and_exits_on_failures() {
        let active = active();
        let key = ExtensionKeyEvent {
            name: "j".into(),
            sequence: "j".into(),
            ctrl: false,
            meta: false,
            option: false,
            shift: false,
        };
        let mut warnings = Vec::new();
        let mut seen = Vec::new();
        assert_eq!(
            deliver_file_view_mode_key(
                &active,
                &key,
                |_, key| {
                    seen.push(key.name.clone());
                    Ok(Some(FileViewModeKeyResult::Handled))
                },
                |warning| warnings.push(warning),
            ),
            FileViewModeKeyResult::Handled
        );
        assert_eq!(seen, ["j"]);
        assert_eq!(
            deliver_file_view_mode_key(
                &active,
                &key,
                |_, _| Ok(None),
                |warning| warnings.push(warning),
            ),
            FileViewModeKeyResult::Pass
        );
        assert_eq!(
            deliver_file_view_mode_key(
                &active,
                &key,
                |_, _| Err(NativeModeCallbackError::Failed("key exploded".into())),
                |warning| warnings.push(warning),
            ),
            FileViewModeKeyResult::Exit
        );
        assert_eq!(
            deliver_file_view_mode_key(
                &active,
                &key,
                |_, _| Err(NativeModeCallbackError::MustReturnSynchronously),
                |warning| warnings.push(warning),
            ),
            FileViewModeKeyResult::Exit
        );
        assert_eq!(
            warnings,
            [
                "Extension preview file view \"rendered\" mode failed onKey • key exploded",
                "Extension preview file view \"rendered\" mode failed onKey • onKey must return synchronously",
            ]
        );
    }
}
