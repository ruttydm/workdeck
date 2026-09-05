//! Host-owned file-presentation preparation metadata and row-failure reporting.
//!
//! This is the native Ratatui counterpart of Hunk's
//! `src/ui/fileViews/useFilePresentationRendering.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. React hook lifetime becomes
//! explicit controller ownership, while accepted layout generations remain
//! the boundary for warning deduplication and retirement.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use workdeck_extension_api::{FileViewRowFailure, ValidatedFileViewLayout};

/// Bound row-render warning metadata even if a large custom tree fails while scrolling.
pub const FILE_VIEW_RENDER_FAILURE_MAX_ENTRIES: usize = 256;

/// One selected, validated file presentation with its host-owned paint identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFileViewLayout {
    pub key: String,
    pub extension_id: String,
    pub view_id: String,
    pub registration_identity: u64,
    pub layout_generation: u64,
    pub validated: ValidatedFileViewLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReportedRowFailure {
    file_id: String,
    layout_generation: u64,
}

/// Deduplicate concrete row failures for exactly as long as their layout generation is active.
#[derive(Debug, Default)]
pub struct FilePresentationRenderingController {
    reported: BTreeMap<String, ReportedRowFailure>,
    insertion_order: VecDeque<String>,
}

impl FilePresentationRenderingController {
    /// Return the attributed warning only for the first occurrence in an active generation.
    #[must_use]
    pub fn report_row_failure(&mut self, failure: &FileViewRowFailure) -> Option<String> {
        let dedupe_key = row_failure_dedupe_key(failure);
        if self.reported.contains_key(&dedupe_key) {
            return None;
        }
        self.reported.insert(
            dedupe_key.clone(),
            ReportedRowFailure {
                file_id: failure.file_id.clone(),
                layout_generation: failure.layout_generation,
            },
        );
        self.insertion_order.push_back(dedupe_key);
        while self.reported.len() > FILE_VIEW_RENDER_FAILURE_MAX_ENTRIES {
            let Some(oldest) = self.insertion_order.pop_front() else {
                break;
            };
            self.reported.remove(&oldest);
        }
        Some(format!(
            "Extension {} file view \"{}\" row \"{}\" failed rendering {} • {}",
            failure.extension_id,
            failure.view_id,
            failure.row_id,
            failure.file_path,
            failure.message
        ))
    }

    /// Forget failures whose file/generation pair no longer belongs to a prepared layout.
    pub fn reconcile_active_layouts<'a>(
        &mut self,
        layouts: impl IntoIterator<Item = (&'a String, &'a ResolvedFileViewLayout)>,
    ) {
        let active = layouts
            .into_iter()
            .map(|(file_id, layout)| (file_id.clone(), layout.layout_generation))
            .collect::<BTreeSet<_>>();
        self.reported.retain(|_, failure| {
            active.contains(&(failure.file_id.clone(), failure.layout_generation))
        });
        self.insertion_order
            .retain(|key| self.reported.contains_key(key));
    }

    #[cfg(test)]
    fn remembered_len(&self) -> usize {
        self.reported.len()
    }
}

fn row_failure_dedupe_key(failure: &FileViewRowFailure) -> String {
    [
        failure.extension_id.as_str(),
        failure.view_id.as_str(),
        failure.file_id.as_str(),
        failure.row_id.as_str(),
        &failure.layout_generation.to_string(),
        failure.message.as_str(),
    ]
    .join("\0")
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_extension_api::ExtensionFileViewLayout;

    fn layout(generation: u64) -> ResolvedFileViewLayout {
        ResolvedFileViewLayout {
            key: "probe:preview".into(),
            extension_id: "probe".into(),
            view_id: "preview".into(),
            registration_identity: 1,
            layout_generation: generation,
            validated: ValidatedFileViewLayout {
                layout: ExtensionFileViewLayout {
                    rows: Vec::new(),
                    hunk_rows: Vec::new(),
                },
                row_heights: Vec::new(),
            },
        }
    }

    fn failure(row_index: usize, generation: u64) -> FileViewRowFailure {
        FileViewRowFailure {
            extension_id: "probe".into(),
            view_id: "preview".into(),
            file_id: "alpha".into(),
            file_path: "alpha.ts".into(),
            row_id: if row_index == usize::MAX {
                "row".into()
            } else {
                format!("row-{row_index}")
            },
            layout_generation: generation,
            message: "paint exploded".into(),
        }
    }

    #[test]
    fn deduplicates_row_warnings_within_one_generation_and_forgets_retired_generations() {
        let mut controller = FilePresentationRenderingController::default();
        let active = BTreeMap::from([("alpha".to_owned(), layout(1))]);
        controller.reconcile_active_layouts(&active);
        let failure = failure(usize::MAX, 1);

        assert_eq!(
            controller.report_row_failure(&failure).as_deref(),
            Some(
                "Extension probe file view \"preview\" row \"row\" failed rendering alpha.ts • paint exploded"
            )
        );
        assert_eq!(controller.report_row_failure(&failure), None);

        controller.reconcile_active_layouts(&BTreeMap::new());
        assert!(controller.report_row_failure(&failure).is_some());
    }

    #[test]
    fn bounds_remembered_row_failures_while_retaining_the_newest_entries() {
        let mut controller = FilePresentationRenderingController::default();
        let active = BTreeMap::from([("alpha".to_owned(), layout(1))]);
        controller.reconcile_active_layouts(&active);
        let mut warnings = Vec::new();

        for row_index in 0..=256 {
            warnings.extend(controller.report_row_failure(&failure(row_index, 1)));
        }
        assert_eq!(warnings.len(), 257);
        assert_eq!(controller.remembered_len(), 256);

        warnings.extend(controller.report_row_failure(&failure(0, 1)));
        warnings.extend(controller.report_row_failure(&failure(256, 1)));
        assert_eq!(warnings.len(), 258);
        assert_eq!(controller.remembered_len(), 256);
    }

    #[test]
    fn frozen_oracle_records_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/file-presentation-rendering.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["baselineOracle"]["passed"], 2);
        assert_eq!(oracle["baselineOracle"]["failed"], 0);
        assert_eq!(oracle["baselineOracle"]["assertions"], 5);
        assert_eq!(oracle["stableOracle"]["relationship"], "identical blobs");
        assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 2);
    }
}
