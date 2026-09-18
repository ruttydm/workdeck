//! Live review guards for extension-driven navigation.

use std::fmt;

use workdeck_core::ReviewSide;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavigableFile<'a> {
    pub id: &'a str,
    pub hunk_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedFileTarget {
    pub file_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedHunkTarget {
    pub file_index: usize,
    pub hunk_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardedLineTarget {
    pub file_index: usize,
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionNavigationWarning(pub String);

impl fmt::Display for ExtensionNavigationWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn ensure_live(
    extension_id: &str,
    method: &str,
    live: bool,
) -> Result<(), ExtensionNavigationWarning> {
    if live {
        Ok(())
    } else {
        Err(ExtensionNavigationWarning(format!(
            "Extension {extension_id} {method} ignored — the review session was reloaded"
        )))
    }
}

#[must_use]
pub fn extension_navigation_reloaded_warning(
    extension_id: &str,
    method: &str,
) -> ExtensionNavigationWarning {
    ensure_live(extension_id, method, false).unwrap_err()
}

fn visible_file_index(
    extension_id: &str,
    method: &str,
    files: &[NavigableFile<'_>],
    file_id: &str,
) -> Result<usize, ExtensionNavigationWarning> {
    files
        .iter()
        .position(|file| file.id == file_id)
        .ok_or_else(|| {
            ExtensionNavigationWarning(format!(
                "Extension {extension_id} {method} targeted unknown file id \"{file_id}\""
            ))
        })
}

pub fn guard_extension_select_file(
    extension_id: &str,
    files: &[NavigableFile<'_>],
    live: bool,
    file_id: &str,
) -> Result<GuardedFileTarget, ExtensionNavigationWarning> {
    ensure_live(extension_id, "selectFile", live)?;
    Ok(GuardedFileTarget {
        file_index: visible_file_index(extension_id, "selectFile", files, file_id)?,
    })
}

/// Validate and clamp the untrusted numeric index before it reaches review state.
///
/// `None` represents a non-numeric JavaScript value at the frozen compatibility boundary.
pub fn guard_extension_select_hunk(
    extension_id: &str,
    files: &[NavigableFile<'_>],
    live: bool,
    file_id: &str,
    hunk_index: Option<f64>,
) -> Result<GuardedHunkTarget, ExtensionNavigationWarning> {
    ensure_live(extension_id, "selectHunk", live)?;
    let file_index = visible_file_index(extension_id, "selectHunk", files, file_id)?;
    let Some(hunk_index) = hunk_index.filter(|index| index.is_finite()) else {
        return Err(ExtensionNavigationWarning(format!(
            "Extension {extension_id} selectHunk received an invalid hunk index for \"{file_id}\""
        )));
    };
    let last = files[file_index].hunk_count.saturating_sub(1);
    let hunk_index = hunk_index.floor().max(0.0);
    Ok(GuardedHunkTarget {
        file_index,
        hunk_index: if hunk_index >= last as f64 {
            last
        } else {
            hunk_index as usize
        },
    })
}

/// Validate the public one-based source address before it reaches review state.
///
/// The string and optional number retain Hunk's invalid dynamic-input cases even though native
/// JSON-RPC actions deserialize directly into the narrower enum and integer types.
pub fn guard_extension_reveal_line(
    extension_id: &str,
    files: &[NavigableFile<'_>],
    live: bool,
    file_id: &str,
    side: &str,
    line: Option<f64>,
) -> Result<GuardedLineTarget, ExtensionNavigationWarning> {
    ensure_live(extension_id, "revealLine", live)?;
    let file_index = visible_file_index(extension_id, "revealLine", files, file_id)?;
    let side = match side {
        "old" => ReviewSide::Old,
        "new" => ReviewSide::New,
        _ => {
            return Err(ExtensionNavigationWarning(format!(
                "Extension {extension_id} revealLine received an invalid side for \"{file_id}\""
            )));
        }
    };
    let Some(line) = line.filter(|line| line.is_finite()) else {
        return Err(invalid_line_warning(extension_id, file_id));
    };
    if line < 1.0 || line.fract() != 0.0 || line > f64::from(u32::MAX) {
        return Err(invalid_line_warning(extension_id, file_id));
    }
    Ok(GuardedLineTarget {
        file_index,
        side,
        line: line as u32,
    })
}

fn invalid_line_warning(extension_id: &str, file_id: &str) -> ExtensionNavigationWarning {
    ExtensionNavigationWarning(format!(
        "Extension {extension_id} revealLine received an invalid line number for \"{file_id}\""
    ))
}

#[must_use]
pub fn extension_navigation_callback_warning(
    extension_id: &str,
    method: &str,
    error: impl fmt::Display,
) -> ExtensionNavigationWarning {
    ExtensionNavigationWarning(format!(
        "Extension {extension_id} failed {method} • {error}"
    ))
}

#[must_use]
pub fn extension_reveal_line_missing_warning(
    extension_id: &str,
    file_id: &str,
    side: ReviewSide,
    line: u32,
) -> ExtensionNavigationWarning {
    let side = match side {
        ReviewSide::Old => "old",
        ReviewSide::New => "new",
    };
    ExtensionNavigationWarning(format!(
        "Extension {extension_id} revealLine found no {side} line {line} in \"{file_id}\""
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files<'a>() -> [NavigableFile<'a>; 1] {
        [NavigableFile {
            id: "a",
            hunk_count: 3,
        }]
    }

    #[test]
    fn frozen_hunk_extension_navigation_oracle_records_both_pinned_baselines() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-navigation.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["stablePresence"], "identical");
        assert_eq!(oracle["executedOracle"]["passed"], 13);
        assert_eq!(oracle["executedOracle"]["expectations"], 22);
    }

    #[test]
    fn visible_file_selection_routes_and_unknown_ids_warn() {
        assert_eq!(
            guard_extension_select_file("triage", &files(), true, "a").unwrap(),
            GuardedFileTarget { file_index: 0 }
        );
        assert_eq!(
            guard_extension_select_file("triage", &files(), true, "hidden")
                .unwrap_err()
                .0,
            "Extension triage selectFile targeted unknown file id \"hidden\""
        );
    }

    #[test]
    fn hunk_indexes_floor_clamp_and_reject_dynamic_garbage() {
        let targets = [Some(99.0), Some(-5.0), Some(1.7)]
            .into_iter()
            .map(|index| {
                guard_extension_select_hunk("triage", &files(), true, "a", index)
                    .unwrap()
                    .hunk_index
            })
            .collect::<Vec<_>>();
        assert_eq!(targets, [2, 0, 1]);
        for invalid in [None, Some(f64::NAN), Some(f64::INFINITY)] {
            assert_eq!(
                guard_extension_select_hunk("triage", &files(), true, "a", invalid)
                    .unwrap_err()
                    .0,
                "Extension triage selectHunk received an invalid hunk index for \"a\""
            );
        }
    }

    #[test]
    fn host_callback_failures_are_attributed_without_unwinding() {
        assert_eq!(
            extension_navigation_callback_warning("triage", "selectFile", "controller unavailable")
                .0,
            "Extension triage failed selectFile • controller unavailable"
        );
    }

    #[test]
    fn a_retired_review_refuses_every_navigation_method() {
        for (method, warning) in [
            (
                "selectFile",
                guard_extension_select_file("triage", &files(), false, "a").unwrap_err(),
            ),
            (
                "selectHunk",
                guard_extension_select_hunk("triage", &files(), false, "a", Some(0.0)).unwrap_err(),
            ),
            (
                "revealLine",
                guard_extension_reveal_line("triage", &files(), false, "a", "new", Some(1.0))
                    .unwrap_err(),
            ),
        ] {
            assert_eq!(
                warning.0,
                format!("Extension triage {method} ignored — the review session was reloaded")
            );
        }
    }

    #[test]
    fn source_line_reveal_routes_only_visible_typed_one_based_lines() {
        assert_eq!(
            guard_extension_reveal_line("triage", &files(), true, "a", "old", Some(211.0)).unwrap(),
            GuardedLineTarget {
                file_index: 0,
                side: ReviewSide::Old,
                line: 211,
            }
        );
        assert_eq!(
            guard_extension_reveal_line("triage", &files(), true, "hidden", "new", Some(4.0))
                .unwrap_err()
                .0,
            "Extension triage revealLine targeted unknown file id \"hidden\""
        );
        assert_eq!(
            guard_extension_reveal_line("triage", &files(), true, "a", "both", Some(4.0))
                .unwrap_err()
                .0,
            "Extension triage revealLine received an invalid side for \"a\""
        );
        for invalid in [Some(0.0), Some(-3.0), Some(2.5), Some(f64::NAN), None] {
            assert_eq!(
                guard_extension_reveal_line("triage", &files(), true, "a", "new", invalid)
                    .unwrap_err()
                    .0,
                "Extension triage revealLine received an invalid line number for \"a\""
            );
        }
    }

    #[test]
    fn hunk_fallback_is_quiet_while_a_missing_line_warns() {
        assert_eq!(
            extension_reveal_line_missing_warning("triage", "a", ReviewSide::New, 9001).0,
            "Extension triage revealLine found no new line 9001 in \"a\""
        );
    }

    #[test]
    fn callers_validate_against_the_live_file_slice_each_time() {
        let before = files();
        let after = [NavigableFile {
            id: "b",
            hunk_count: 1,
        }];
        assert!(guard_extension_select_file("triage", &before, true, "a").is_ok());
        assert!(guard_extension_select_file("triage", &after, true, "a").is_err());
        assert!(guard_extension_select_file("triage", &after, true, "b").is_ok());
    }
}
