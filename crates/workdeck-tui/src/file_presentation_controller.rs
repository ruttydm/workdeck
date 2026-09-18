//! Host projection for file-presentation selection, menus, and bulk actions.
//!
//! This is part of the native Ratatui counterpart of Hunk's
//! `src/ui/fileViews/useFilePresentationController.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Selection and mode ownership
//! live in the extension host; this module derives the user-facing menu from
//! the same currently presented (rather than merely stored) choice.

use crate::MenuEntry;

/// One registration after the host has evaluated its matcher for the selected file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePresentationMenuCandidate {
    pub key: String,
    pub title: String,
    pub matches: bool,
}

/// The still-applicable apply-to-all operation behind the selected presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePresentationBulkTarget {
    pub key: String,
    pub title: String,
    pub file_ids: Vec<String>,
}

/// Complete dynamic contribution to Workdeck's View menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilePresentationMenuProjection {
    pub entries: Vec<MenuEntry>,
    pub bulk_target: Option<FilePresentationBulkTarget>,
}

/// Derive menu rows from the presentation actually visible for the selected file.
#[must_use]
pub fn plan_file_presentation_menu(
    selected_file_id: Option<&str>,
    presented_key: Option<&str>,
    unavailable_reason: Option<&str>,
    candidates: &[FilePresentationMenuCandidate],
    bulk_target: Option<FilePresentationBulkTarget>,
) -> FilePresentationMenuProjection {
    if selected_file_id.is_none() {
        return FilePresentationMenuProjection::default();
    }
    let mut entries = vec![MenuEntry::Item {
        label: "File presentation: Raw diff".into(),
        command_id: Some("workdeck.view.filePresentation.raw".into()),
        hint: None,
        checked: Some(presented_key.is_none()),
    }];
    if unavailable_reason.is_none() {
        entries.extend(
            candidates
                .iter()
                .filter(|candidate| candidate.matches)
                .map(|candidate| MenuEntry::Item {
                    label: format!("File presentation: {}", candidate.title),
                    command_id: Some(format!("workdeck.view.filePresentation.{}", candidate.key)),
                    hint: None,
                    checked: Some(presented_key == Some(candidate.key.as_str())),
                }),
        );
    }
    FilePresentationMenuProjection {
        entries,
        bulk_target: if unavailable_reason.is_none() {
            bulk_target
        } else {
            None
        },
    }
}

/// A requested handoff may install only when outgoing teardown did not install
/// a successor of its own.
#[must_use]
pub(crate) const fn requested_mode_may_enter(current_activation_id: Option<u64>) -> bool {
    current_activation_id.is_none()
}

/// Callback completion and `exit` routing belong to the activation that began
/// them, never to a replacement installed re-entrantly during the callback.
#[must_use]
pub(crate) const fn activation_still_owns_mode(
    current_activation_id: Option<u64>,
    expected_activation_id: u64,
) -> bool {
    matches!(current_activation_id, Some(current) if current == expected_activation_id)
}

/// Matcher failures are contained as an unavailable presentation, exactly like
/// an explicit `false` answer.
pub(crate) fn contain_file_view_match<E>(result: Result<bool, E>) -> bool {
    result.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<FilePresentationMenuCandidate> {
        vec![
            FilePresentationMenuCandidate {
                key: "probe:preview".into(),
                title: "Preview".into(),
                matches: true,
            },
            FilePresentationMenuCandidate {
                key: "probe:nope".into(),
                title: "Nope".into(),
                matches: false,
            },
            FilePresentationMenuCandidate {
                key: "probe:raw".into(),
                title: "Extension raw".into(),
                matches: true,
            },
        ]
    }

    #[test]
    fn frozen_hunk_controller_oracle_records_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/file-presentation-controller.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"]["passed"], 10);
        assert_eq!(oracle["stableOracle"]["passed"], 10);
        assert_eq!(oracle["baselineOracle"]["assertions"], 42);
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 10);
    }

    fn labels(projection: &FilePresentationMenuProjection) -> Vec<&str> {
        projection
            .entries
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Item { label, .. } => Some(label.as_str()),
                MenuEntry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn raw_remains_implicit_when_an_extension_view_is_literally_named_raw() {
        let raw = plan_file_presentation_menu(Some("alpha"), None, None, &candidates(), None);
        assert!(matches!(
            &raw.entries[0],
            MenuEntry::Item {
                checked: Some(true),
                ..
            }
        ));
        assert_eq!(labels(&raw)[2], "File presentation: Extension raw");

        let extension_raw = plan_file_presentation_menu(
            Some("alpha"),
            Some("probe:raw"),
            None,
            &candidates(),
            None,
        );
        assert!(matches!(
            &extension_raw.entries[0],
            MenuEntry::Item {
                checked: Some(false),
                ..
            }
        ));
        assert!(matches!(
            &extension_raw.entries[2],
            MenuEntry::Item {
                checked: Some(true),
                ..
            }
        ));
    }

    #[test]
    fn unavailable_draft_masks_but_does_not_rewrite_the_stored_choice() {
        let projection = plan_file_presentation_menu(
            Some("alpha"),
            None,
            Some("drafting requires raw"),
            &candidates(),
            Some(FilePresentationBulkTarget {
                key: "probe:preview".into(),
                title: "Preview".into(),
                file_ids: vec!["alpha".into(), "beta".into()],
            }),
        );
        assert_eq!(labels(&projection), ["File presentation: Raw diff"]);
        assert!(projection.bulk_target.is_none());
    }

    #[test]
    fn menu_omits_nonmatching_or_throwing_views_and_retains_registration_order() {
        let projection = plan_file_presentation_menu(
            Some("alpha"),
            Some("probe:preview"),
            None,
            &candidates(),
            None,
        );
        assert_eq!(
            labels(&projection),
            [
                "File presentation: Raw diff",
                "File presentation: Preview",
                "File presentation: Extension raw"
            ]
        );
        assert!(!contain_file_view_match(Ok::<_, &str>(false)));
        assert!(!contain_file_view_match(Err::<bool, _>("matcher exploded")));
        assert!(contain_file_view_match(Ok::<_, &str>(true)));
    }

    #[test]
    fn no_selected_file_contributes_no_presentation_commands() {
        assert_eq!(
            plan_file_presentation_menu(None, None, None, &candidates(), None),
            FilePresentationMenuProjection::default()
        );
    }

    #[test]
    fn reentrant_successor_from_outgoing_exit_wins_over_requested_mode() {
        assert!(requested_mode_may_enter(None));
        assert!(!requested_mode_may_enter(Some(3)));
    }

    #[test]
    fn failed_or_exiting_activation_cannot_retire_its_reentrant_replacement() {
        assert!(activation_still_owns_mode(Some(4), 4));
        assert!(!activation_still_owns_mode(Some(5), 4));
        assert!(!activation_still_owns_mode(None, 4));
    }
}
