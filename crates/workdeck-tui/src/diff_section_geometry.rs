//! Cached file-section height and review-navigation geometry.
//!
//! This is a Rust reimplementation of Hunk's `src/ui/diff/diffSectionGeometry.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};
use workdeck_core::{DiffFile, LineRange};
use workdeck_diff::{DEFAULT_TAB_WIDTH, DiffRow, find_max_line_number, source_text_fingerprint};
use workdeck_review::{ExpandedSourceError, ExpandedSourceStatus, LayoutMode, PlannedFileViewRow};

use crate::{
    AppTheme, BuildDiffSectionRowPlanOptions, CodeRowLayoutOptions, DEFAULT_HUNK_GAP,
    PlannedHunkBounds, PlannedReviewRow, VerticalBounds, VisibleAgentNote,
    build_diff_section_row_plan, measure_agent_inline_note_height,
    measure_planned_rendered_row_height, planned_review_row_contributes_to_hunk_bounds,
    review_row_id,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSectionRowBounds {
    pub key: String,
    pub stable_key: String,
    pub stable_keys: Vec<String>,
    pub expanded_gap_key: Option<String>,
    pub bounds: VerticalBounds,
}

#[derive(Debug, Clone)]
enum OwnedSourceStatus {
    Pending,
    Loading,
    Loaded(String),
    Error(ExpandedSourceError),
}

impl OwnedSourceStatus {
    fn borrowed(&self) -> ExpandedSourceStatus<'_> {
        match self {
            Self::Pending => ExpandedSourceStatus::Pending,
            Self::Loading => ExpandedSourceStatus::Loading,
            Self::Loaded(text) => ExpandedSourceStatus::Loaded(text),
            Self::Error(error) => ExpandedSourceStatus::Error(*error),
        }
    }
}

impl From<ExpandedSourceStatus<'_>> for OwnedSourceStatus {
    fn from(status: ExpandedSourceStatus<'_>) -> Self {
        match status {
            ExpandedSourceStatus::Pending => Self::Pending,
            ExpandedSourceStatus::Loading => Self::Loading,
            ExpandedSourceStatus::Loaded(text) => Self::Loaded(text.into()),
            ExpandedSourceStatus::Error(error) => Self::Error(error),
        }
    }
}

#[derive(Debug, Clone)]
struct DiffSectionRowPlanSnapshot {
    expanded_keys: HashSet<String>,
    file: DiffFile,
    layout: LayoutMode,
    show_hunk_headers: bool,
    source_status: OwnedSourceStatus,
    tab_width: u16,
    hunk_gap: usize,
    theme: AppTheme,
    visible_agent_notes: Vec<VisibleAgentNote>,
}

impl DiffSectionRowPlanSnapshot {
    fn build(&self) -> Vec<PlannedReviewRow> {
        build_diff_section_row_plan(BuildDiffSectionRowPlanOptions {
            expanded_keys: &self.expanded_keys,
            file: Some(&self.file),
            highlighted_diff: None,
            layout: self.layout,
            show_hunk_headers: self.show_hunk_headers,
            source_line_spans: None,
            source_status: self.source_status.borrowed(),
            tab_width: self.tab_width,
            hunk_gap: self.hunk_gap,
            theme: &self.theme,
            visible_agent_notes: &self.visible_agent_notes,
        })
        .planned_rows
    }
}

#[derive(Debug)]
pub struct DiffSectionGeometry {
    pub body_height: usize,
    pub hunk_anchor_rows: HashMap<usize, usize>,
    pub hunk_bounds: HashMap<usize, PlannedHunkBounds>,
    pub line_number_digits: usize,
    pub file_view_rows: Option<Vec<PlannedFileViewRow>>,
    pub row_bounds: Vec<DiffSectionRowBounds>,
    pub row_bounds_by_key: HashMap<String, DiffSectionRowBounds>,
    pub row_bounds_by_stable_key: HashMap<String, DiffSectionRowBounds>,
    planned_rows: OnceLock<Vec<PlannedReviewRow>>,
    row_plan_snapshot: Option<DiffSectionRowPlanSnapshot>,
}

impl DiffSectionGeometry {
    #[must_use]
    pub fn planned_rows(&self) -> &[PlannedReviewRow] {
        self.planned_rows
            .get_or_init(|| {
                self.row_plan_snapshot
                    .as_ref()
                    .map_or_else(Vec::new, DiffSectionRowPlanSnapshot::build)
            })
            .as_slice()
    }

    #[must_use]
    pub fn planned_rows_are_initialized(&self) -> bool {
        self.planned_rows.get().is_some()
    }
}

pub struct DiffSectionGeometryOptions<'a> {
    pub file: &'a DiffFile,
    pub layout: LayoutMode,
    pub show_hunk_headers: bool,
    pub theme: &'a AppTheme,
    pub visible_agent_notes: &'a [VisibleAgentNote],
    pub width: usize,
    pub show_line_numbers: bool,
    /// Override the file-derived gutter width when an embedding renderer owns
    /// a wider fixed line-number lane.
    pub line_number_digits: Option<usize>,
    pub wrap_lines: bool,
    pub expanded_keys: &'a HashSet<String>,
    pub source_status: ExpandedSourceStatus<'a>,
    pub reserve_add_note_column: bool,
    pub tab_width: u16,
    pub hunk_gap: usize,
}

impl<'a> DiffSectionGeometryOptions<'a> {
    #[must_use]
    pub fn new(file: &'a DiffFile, layout: LayoutMode, theme: &'a AppTheme) -> Self {
        Self {
            file,
            layout,
            show_hunk_headers: true,
            theme,
            visible_agent_notes: &[],
            width: 0,
            show_line_numbers: true,
            line_number_digits: None,
            wrap_lines: false,
            expanded_keys: empty_expanded_keys(),
            source_status: ExpandedSourceStatus::Pending,
            reserve_add_note_column: false,
            tab_width: DEFAULT_TAB_WIDTH,
            hunk_gap: DEFAULT_HUNK_GAP,
        }
    }
}

fn empty_expanded_keys() -> &'static HashSet<String> {
    static EMPTY: std::sync::LazyLock<HashSet<String>> = std::sync::LazyLock::new(HashSet::new);
    &EMPTY
}

#[derive(Debug)]
struct SectionGeometryCacheEntry {
    key: String,
    geometry: Arc<DiffSectionGeometry>,
}

#[derive(Debug, Default)]
struct SectionGeometryCacheSlots {
    base: Option<SectionGeometryCacheEntry>,
    notes: Option<SectionGeometryCacheEntry>,
}

#[derive(Debug, Default)]
pub struct DiffSectionGeometryCache {
    files: HashMap<usize, SectionGeometryCacheSlots>,
}

fn file_object_key(file: &DiffFile) -> usize {
    std::ptr::from_ref(file).cast::<()>() as usize
}

fn json_range(range: Option<LineRange>) -> Option<Value> {
    range.map(|range| Value::Array(vec![Value::from(range.start), Value::from(range.end)]))
}

fn insert_optional(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        map.insert(key.into(), value);
    }
}

fn notes_cache_key(notes: &[VisibleAgentNote]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let mut encoded = String::new();
    for note in notes {
        let annotation = &note.annotation;
        let mut object = Map::new();
        insert_optional(
            &mut object,
            "author",
            annotation.author.clone().map(Value::String),
        );
        insert_optional(&mut object, "id", annotation.id.clone().map(Value::String));
        insert_optional(&mut object, "newRange", json_range(annotation.new_range));
        insert_optional(&mut object, "oldRange", json_range(annotation.old_range));
        insert_optional(
            &mut object,
            "rationale",
            annotation.rationale.clone().map(Value::String),
        );
        insert_optional(
            &mut object,
            "source",
            annotation.source.clone().map(Value::String),
        );
        object.insert("summary".into(), Value::String(annotation.summary.clone()));
        insert_optional(
            &mut object,
            "title",
            annotation.title.clone().map(Value::String),
        );
        if let Some(thread) = &note.thread {
            insert_optional(
                &mut object,
                "parentId",
                thread.parent_id.clone().map(Value::String),
            );
            object.insert("threadDepth".into(), Value::from(thread.depth));
        }
        if let Some(actions) = note.actions {
            object.insert(
                "actions".into(),
                serde_json::json!({
                    "edit": actions.edit,
                    "reply": actions.reply,
                    "delete": actions.delete,
                }),
            );
        }
        let value = Value::Object(object).to_string();
        encoded.push_str(&format!("{}:{value}", value.encode_utf16().count()));
    }
    format!(":notes:{encoded}")
}

fn expansion_cache_key(
    expanded_keys: &HashSet<String>,
    source_status: ExpandedSourceStatus<'_>,
) -> String {
    if expanded_keys.is_empty() {
        return String::new();
    }
    let mut keys = expanded_keys.iter().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    let status = match source_status {
        ExpandedSourceStatus::Pending => "pending".into(),
        ExpandedSourceStatus::Loading => "loading".into(),
        ExpandedSourceStatus::Loaded(text) => {
            format!("loaded:{}", source_text_fingerprint(text))
        }
        ExpandedSourceStatus::Error(_) => "error".into(),
    };
    format!(":{}:{status}", keys.join(","))
}

fn theme_cache_key(theme: &AppTheme) -> String {
    [
        theme.id.as_str(),
        theme.syntax_theme.as_deref().unwrap_or_default(),
        theme.background.as_str(),
        theme.panel_alt.as_str(),
        theme.context_bg.as_str(),
        theme.added_bg.as_str(),
        theme.removed_bg.as_str(),
        theme.line_number_bg.as_str(),
        theme.line_number_fg.as_str(),
    ]
    .join(":")
}

fn geometry_cache_key(options: &DiffSectionGeometryOptions<'_>) -> String {
    let file_id = if options.file.runtime_id.is_empty() {
        &options.file.key
    } else {
        &options.file.runtime_id
    };
    let layout = match options.layout {
        LayoutMode::Split => "split",
        LayoutMode::Stack => "stack",
        LayoutMode::Auto => "auto",
    };
    format!(
        "{file_id}:{layout}:{}:{}:{}:{}:lineDigits:{:?}:{}:{}:tabs:{}:hunkGap:{}{}{}",
        usize::from(options.show_hunk_headers),
        theme_cache_key(options.theme),
        options.width,
        usize::from(options.show_line_numbers),
        options.line_number_digits,
        usize::from(options.wrap_lines),
        usize::from(options.reserve_add_note_column),
        options.tab_width,
        options.hunk_gap,
        expansion_cache_key(options.expanded_keys, options.source_status),
        notes_cache_key(options.visible_agent_notes),
    )
}

fn planned_row_stable_keys(row: &PlannedReviewRow) -> (&str, Vec<String>) {
    match row {
        PlannedReviewRow::DiffRow {
            stable_key,
            stable_alias_keys,
            ..
        } => {
            let mut keys = Vec::with_capacity(stable_alias_keys.len() + 1);
            keys.push(stable_key.clone());
            keys.extend(stable_alias_keys.iter().cloned());
            (stable_key, keys)
        }
        PlannedReviewRow::InlineNote { stable_key, .. }
        | PlannedReviewRow::HunkGap { stable_key, .. } => (stable_key, vec![stable_key.clone()]),
    }
}

fn planned_row_expanded_gap_key(row: &PlannedReviewRow) -> Option<String> {
    let PlannedReviewRow::DiffRow { row, .. } = row else {
        return None;
    };
    match row {
        DiffRow::SplitLine {
            expanded_gap_key, ..
        }
        | DiffRow::StackLine {
            expanded_gap_key, ..
        } => expanded_gap_key.clone(),
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => None,
    }
}

fn measure_planned_diff_section_row_height(
    row: &PlannedReviewRow,
    options: &DiffSectionGeometryOptions<'_>,
    line_number_digits: usize,
) -> usize {
    match row {
        PlannedReviewRow::InlineNote {
            annotation,
            anchor_side,
            note,
            ..
        } => measure_agent_inline_note_height(
            annotation,
            *anchor_side,
            options.layout,
            options.width,
            note.thread.as_ref().map_or(0, |thread| thread.depth),
        ),
        PlannedReviewRow::HunkGap { height, .. } => *height,
        PlannedReviewRow::DiffRow { .. } => measure_planned_rendered_row_height(
            row,
            CodeRowLayoutOptions {
                width: options.width,
                line_number_digits,
                show_line_numbers: options.show_line_numbers,
                wrap_lines: options.wrap_lines,
                reserve_add_note_column: options.reserve_add_note_column,
                show_add_note_badge: false,
            },
            options.show_hunk_headers,
        ),
    }
}

fn empty_geometry(file: &DiffFile) -> Arc<DiffSectionGeometry> {
    let planned_rows = OnceLock::new();
    let _ = planned_rows.set(Vec::new());
    Arc::new(DiffSectionGeometry {
        body_height: 1,
        hunk_anchor_rows: HashMap::new(),
        hunk_bounds: HashMap::new(),
        line_number_digits: find_max_line_number(file).to_string().len(),
        file_view_rows: None,
        row_bounds: Vec::new(),
        row_bounds_by_key: HashMap::new(),
        row_bounds_by_stable_key: HashMap::new(),
        planned_rows,
        row_plan_snapshot: None,
    })
}

fn measure_uncached(options: &DiffSectionGeometryOptions<'_>) -> Arc<DiffSectionGeometry> {
    let section_row_plan = build_diff_section_row_plan(BuildDiffSectionRowPlanOptions {
        expanded_keys: options.expanded_keys,
        file: Some(options.file),
        highlighted_diff: None,
        layout: options.layout,
        show_hunk_headers: options.show_hunk_headers,
        source_line_spans: None,
        source_status: options.source_status,
        tab_width: options.tab_width,
        hunk_gap: options.hunk_gap,
        theme: options.theme,
        visible_agent_notes: options.visible_agent_notes,
    });
    let line_number_digits = options
        .line_number_digits
        .unwrap_or(section_row_plan.line_number_digits)
        .max(1);
    let mut hunk_anchor_rows = HashMap::new();
    let mut hunk_bounds = HashMap::<usize, PlannedHunkBounds>::new();
    let mut row_bounds = Vec::with_capacity(section_row_plan.planned_rows.len());
    let mut row_bounds_by_key = HashMap::new();
    let mut row_bounds_by_stable_key = HashMap::new();
    let mut body_height = 0;

    for row in &section_row_plan.planned_rows {
        if matches!(
            row,
            PlannedReviewRow::DiffRow {
                anchor_id: Some(_),
                ..
            }
        ) {
            hunk_anchor_rows
                .entry(row.hunk_index())
                .or_insert(body_height);
        }
        let height = measure_planned_diff_section_row_height(row, options, line_number_digits);
        let (stable_key, stable_keys) = planned_row_stable_keys(row);
        let entry = DiffSectionRowBounds {
            key: row.key().into(),
            stable_key: stable_key.into(),
            stable_keys,
            expanded_gap_key: planned_row_expanded_gap_key(row),
            bounds: VerticalBounds {
                top: body_height,
                height,
            },
        };
        row_bounds.push(entry.clone());
        row_bounds_by_key.insert(entry.key.clone(), entry.clone());
        for key in &entry.stable_keys {
            row_bounds_by_stable_key
                .entry(key.clone())
                .or_insert_with(|| entry.clone());
        }
        if height > 0 && planned_review_row_contributes_to_hunk_bounds(row) {
            let row_id = review_row_id(row.key());
            if let Some(bounds) = hunk_bounds.get_mut(&row.hunk_index()) {
                bounds.end_row_id = row_id;
                bounds.height = bounds.height.saturating_add(height);
            } else {
                hunk_bounds.insert(
                    row.hunk_index(),
                    PlannedHunkBounds {
                        top: body_height,
                        height,
                        start_row_id: row_id.clone(),
                        end_row_id: row_id,
                    },
                );
            }
        }
        body_height = body_height.saturating_add(height);
    }

    Arc::new(DiffSectionGeometry {
        body_height,
        hunk_anchor_rows,
        hunk_bounds,
        line_number_digits,
        file_view_rows: None,
        row_bounds,
        row_bounds_by_key,
        row_bounds_by_stable_key,
        planned_rows: OnceLock::new(),
        row_plan_snapshot: Some(DiffSectionRowPlanSnapshot {
            expanded_keys: options.expanded_keys.clone(),
            file: options.file.clone(),
            layout: options.layout,
            show_hunk_headers: options.show_hunk_headers,
            source_status: options.source_status.into(),
            tab_width: options.tab_width,
            hunk_gap: options.hunk_gap,
            theme: options.theme.clone(),
            visible_agent_notes: options.visible_agent_notes.to_vec(),
        }),
    })
}

impl DiffSectionGeometryCache {
    /// Measure one file section from the same canonical row plan used for rendering.
    #[must_use]
    pub fn measure(&mut self, options: DiffSectionGeometryOptions<'_>) -> Arc<DiffSectionGeometry> {
        assert_ne!(
            options.layout,
            LayoutMode::Auto,
            "section geometry requires a resolved layout"
        );
        if options.file.hunks.is_empty() {
            return empty_geometry(options.file);
        }
        let key = geometry_cache_key(&options);
        let slots = self.files.entry(file_object_key(options.file)).or_default();
        let slot = if options.visible_agent_notes.is_empty() {
            &mut slots.base
        } else {
            &mut slots.notes
        };
        if let Some(entry) = slot
            && entry.key == key
        {
            return Arc::clone(&entry.geometry);
        }
        let geometry = measure_uncached(&options);
        *slot = Some(SectionGeometryCacheEntry {
            key,
            geometry: Arc::clone(&geometry),
        });
        geometry
    }
}

/// Estimate the number of diff-body rows for one file section in the windowed path.
#[must_use]
pub fn estimate_diff_section_body_rows(
    cache: &mut DiffSectionGeometryCache,
    file: &DiffFile,
    layout: LayoutMode,
    show_hunk_headers: bool,
    theme: &AppTheme,
) -> usize {
    let mut options = DiffSectionGeometryOptions::new(file, layout, theme);
    options.show_hunk_headers = show_hunk_headers;
    cache.measure(options).body_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{AgentAnnotation, FileSourceSnapshots, SourceOrigin, SourceSnapshot};
    use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
    use workdeck_review::{ReviewNoteAnchorInput, ReviewPreferredLine, resolve_review_note_anchor};

    use crate::{
        VisibleAgentNoteActions, VisibleAgentNoteDraft, VisibleAgentNoteThread, VisibleNoteSource,
        resolve_theme,
    };

    const DEFAULT_BEFORE: &str = concat!(
        "const alpha = 1;\n",
        "const beta = 2;\n",
        "const gamma = 3;\n",
        "const stable = true;\n",
    );
    const DEFAULT_AFTER: &str = concat!(
        "const alpha = 10;\n",
        "const beta = 2;\n",
        "const gamma = 30;\n",
        "const stable = true;\n",
    );

    fn test_file(before: &str, after: &str, id: &str, path: &str) -> DiffFile {
        let mut file = diff_from_file_snapshots(
            FileSnapshot {
                cache_key: "before",
                contents: before,
                name: path,
            },
            FileSnapshot {
                cache_key: "after",
                contents: after,
                name: path,
            },
            FileComparisonOptions { context_radius: 0 },
        )
        .expect("test snapshots differ");
        file.runtime_id = id.into();
        file.flags.partial = false;
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                before.into(),
                SourceOrigin::File { path: path.into() },
                true,
            )),
            new: Some(SourceSnapshot::new(
                after.into(),
                SourceOrigin::File { path: path.into() },
                true,
            )),
        });
        file
    }

    fn default_file() -> DiffFile {
        test_file(DEFAULT_BEFORE, DEFAULT_AFTER, "example", "example.ts")
    }

    fn annotation(summary: &str, rationale: Option<&str>, line: u32) -> AgentAnnotation {
        AgentAnnotation {
            id: Some("annotation:example:0".into()),
            old_range: None,
            new_range: Some(LineRange {
                start: line,
                end: line,
            }),
            summary: summary.into(),
            rationale: rationale.map(str::to_owned),
            markup: None,
            tags: Vec::new(),
            confidence: None,
            source: None,
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: false,
        }
    }

    fn note(
        file: &DiffFile,
        summary: &str,
        rationale: Option<&str>,
        line: u32,
    ) -> VisibleAgentNote {
        let annotation = annotation(summary, rationale, line);
        let preferred = ReviewPreferredLine {
            side: workdeck_core::ReviewSide::New,
            line,
        };
        VisibleAgentNote {
            id: "annotation:example:0".into(),
            anchor: resolve_review_note_anchor(
                &file.hunks,
                ReviewNoteAnchorInput {
                    old_range: annotation.old_range,
                    new_range: annotation.new_range,
                    preferred: Some(preferred),
                    fallback_owner_hunk_index: None,
                },
            ),
            annotation,
            source: Some(VisibleNoteSource::Agent),
            editable: false,
            thread: None,
            actions: None,
            draft: None,
        }
    }

    fn measure(
        cache: &mut DiffSectionGeometryCache,
        file: &DiffFile,
        layout: LayoutMode,
        notes: &[VisibleAgentNote],
        width: usize,
        wrap_lines: bool,
    ) -> Arc<DiffSectionGeometry> {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut options = DiffSectionGeometryOptions::new(file, layout, &theme);
        options.visible_agent_notes = notes;
        options.width = width;
        options.wrap_lines = wrap_lines;
        cache.measure(options)
    }

    fn oracle() -> Value {
        serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/diff-section-geometry.json"
        ))
        .expect("frozen section geometry oracle is valid JSON")
    }

    fn sorted_anchors(geometry: &DiffSectionGeometry) -> Vec<[usize; 2]> {
        let mut anchors = geometry
            .hunk_anchor_rows
            .iter()
            .map(|(hunk, top)| [*hunk, *top])
            .collect::<Vec<_>>();
        anchors.sort_unstable();
        anchors
    }

    fn sorted_hunk_bounds(geometry: &DiffSectionGeometry) -> Vec<[usize; 3]> {
        let mut bounds = geometry
            .hunk_bounds
            .iter()
            .map(|(hunk, bounds)| [*hunk, bounds.top, bounds.height])
            .collect::<Vec<_>>();
        bounds.sort_unstable();
        bounds
    }

    #[test]
    fn frozen_baseline_projection_vectors_match_native_geometry() {
        let expected = &oracle()["projectionVectors"];
        let file = default_file();
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = DiffSectionGeometryCache::default();
        let mut split_options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        split_options.width = 120;
        let split = cache.measure(split_options);
        assert_eq!(split.body_height, expected["split"]["bodyHeight"]);
        assert_eq!(
            split.line_number_digits,
            expected["split"]["lineNumberDigits"]
        );
        assert_eq!(
            serde_json::to_value(sorted_anchors(&split)).unwrap(),
            expected["split"]["hunkAnchorRows"]
        );
        assert_eq!(
            serde_json::to_value(sorted_hunk_bounds(&split)).unwrap(),
            expected["split"]["hunkBounds"]
        );
        assert_eq!(
            serde_json::to_value(
                split
                    .row_bounds
                    .iter()
                    .map(|row| row.key.as_str())
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            expected["split"]["rowKeys"]
        );

        let mut stack_options = DiffSectionGeometryOptions::new(&file, LayoutMode::Stack, &theme);
        stack_options.width = 120;
        let stack = cache.measure(stack_options);
        assert_eq!(stack.body_height, expected["stack"]["bodyHeight"]);
        assert_eq!(
            stack.line_number_digits,
            expected["stack"]["lineNumberDigits"]
        );
        assert_eq!(
            serde_json::to_value(sorted_anchors(&stack)).unwrap(),
            expected["stack"]["hunkAnchorRows"]
        );
        assert_eq!(
            serde_json::to_value(sorted_hunk_bounds(&stack)).unwrap(),
            expected["stack"]["hunkBounds"]
        );

        let mut gap_options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        gap_options.width = 120;
        gap_options.hunk_gap = 2;
        let gap = cache.measure(gap_options);
        let gap_row = gap
            .row_bounds
            .iter()
            .find(|row| row.key.starts_with("hunk-gap:"))
            .expect("main baseline inserts its configured gap");
        assert_eq!(gap.body_height, expected["hunkGap2"]["bodyHeight"]);
        assert_eq!(gap_row.key, expected["hunkGap2"]["gapKey"]);
        assert_eq!(gap_row.bounds.top, expected["hunkGap2"]["gapTop"]);
        assert_eq!(gap_row.bounds.height, expected["hunkGap2"]["gapHeight"]);
        assert_eq!(
            gap.hunk_anchor_rows[&1],
            expected["hunkGap2"]["secondHunkAnchor"]
        );

        let notes = vec![note(
            &file,
            "Explain the change",
            Some("Keep note height in section geometry."),
            1,
        )];
        let noted = measure(&mut cache, &file, LayoutMode::Split, &notes, 120, false);
        let note_row = noted
            .row_bounds
            .iter()
            .find(|row| row.key.starts_with("inline-note:"))
            .expect("note vector includes one inline note");
        assert_eq!(noted.body_height, expected["inlineNote"]["bodyHeight"]);
        assert_eq!(note_row.bounds.top, expected["inlineNote"]["noteTop"]);
        assert_eq!(note_row.bounds.height, expected["inlineNote"]["noteHeight"]);
        assert_eq!(
            noted.hunk_anchor_rows[&1],
            expected["inlineNote"]["secondHunkAnchor"]
        );
    }

    #[test]
    fn measures_split_and_stack_layouts_from_the_render_plan() {
        let file = default_file();
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = DiffSectionGeometryCache::default();
        let split = cache.measure(DiffSectionGeometryOptions::new(
            &file,
            LayoutMode::Split,
            &theme,
        ));
        let stack = cache.measure(DiffSectionGeometryOptions::new(
            &file,
            LayoutMode::Stack,
            &theme,
        ));
        assert_eq!(split.body_height, 6);
        assert_eq!(stack.body_height, 8);
        assert_eq!(split.hunk_bounds[&0].height, 2);
        assert_eq!(stack.hunk_bounds[&0].height, 3);
    }

    #[test]
    fn reuses_no_note_geometry_for_the_same_file_and_layout_inputs() {
        let file = default_file();
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = DiffSectionGeometryCache::default();
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.width = 120;
        let first = cache.measure(options);
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.width = 120;
        let second = cache.measure(options);
        assert!(Arc::ptr_eq(&first, &second));

        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.width = 121;
        assert!(!Arc::ptr_eq(&first, &cache.measure(options)));
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.width = 120;
        options.reserve_add_note_column = true;
        assert!(!Arc::ptr_eq(&first, &cache.measure(options)));
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.width = 120;
        options.tab_width = 8;
        assert!(!Arc::ptr_eq(&first, &cache.measure(options)));
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.width = 120;
        options.hunk_gap = 2;
        let different_hunk_gap = cache.measure(options);
        assert!(!Arc::ptr_eq(&first, &different_hunk_gap));
        assert!(different_hunk_gap.body_height > first.body_height);
    }

    #[test]
    fn caches_planned_rows_after_the_lazy_geometry_property_is_first_read() {
        let file = default_file();
        let mut cache = DiffSectionGeometryCache::default();
        let geometry = measure(&mut cache, &file, LayoutMode::Split, &[], 120, false);
        assert!(!geometry.planned_rows_are_initialized());
        let first = geometry.planned_rows().as_ptr();
        assert_eq!(geometry.planned_rows().len(), geometry.row_bounds.len());
        assert_eq!(geometry.planned_rows().as_ptr(), first);
    }

    #[test]
    fn keeps_lazy_planned_rows_aligned_after_caller_owned_inputs_mutate() {
        let before = (1..=30)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before.replace("line 5\n", "line 5 modified\n");
        let file = test_file(&before, &after, "snapshot", "snapshot.txt");
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut expanded_keys = HashSet::from(["trailing:0".into()]);
        let mut notes = vec![note(&file, "Original note.", None, 5)];
        notes[0].id = "original-note".into();
        notes[0].annotation.id = Some("original-note".into());
        let mut cache = DiffSectionGeometryCache::default();
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.visible_agent_notes = &notes;
        options.width = 120;
        options.expanded_keys = &expanded_keys;
        options.source_status = ExpandedSourceStatus::Loaded(&after);
        let geometry = cache.measure(options);

        expanded_keys.clear();
        notes[0].annotation.new_range = Some(LineRange { start: 1, end: 1 });
        notes[0].annotation.summary = "Mutated after geometry measurement.".into();
        notes.push(note(&file, "Added after geometry measurement.", None, 1));
        assert_eq!(
            geometry
                .planned_rows()
                .iter()
                .map(|row| row.key())
                .collect::<Vec<_>>(),
            geometry
                .row_bounds
                .iter()
                .map(|bounds| bounds.key.as_str())
                .collect::<Vec<_>>()
        );
        let summary = geometry.planned_rows().iter().find_map(|row| match row {
            PlannedReviewRow::InlineNote { annotation, .. } => Some(annotation.summary.as_str()),
            _ => None,
        });
        assert_eq!(summary, Some("Original note."));
    }

    #[test]
    fn replaces_stale_width_variants_while_retaining_base_and_note_slots() {
        let file = default_file();
        let notes = vec![note(
            &file,
            "Explain",
            Some("Keep a note-aware geometry slot."),
            1,
        )];
        let mut cache = DiffSectionGeometryCache::default();
        let width100 = measure(&mut cache, &file, LayoutMode::Split, &[], 100, false);
        let note_width100 = measure(&mut cache, &file, LayoutMode::Split, &notes, 100, false);
        let width101 = measure(&mut cache, &file, LayoutMode::Split, &[], 101, false);
        assert!(!Arc::ptr_eq(&width101, &width100));
        let width100_again = measure(&mut cache, &file, LayoutMode::Split, &[], 100, false);
        assert!(!Arc::ptr_eq(&width100_again, &width100));
        let note_again = measure(&mut cache, &file, LayoutMode::Split, &notes, 100, false);
        assert!(Arc::ptr_eq(&note_again, &note_width100));
    }

    #[test]
    fn accounts_for_visible_inline_notes_without_moving_the_hunk_anchor() {
        let file = default_file();
        let notes = vec![note(
            &file,
            "Explain the change",
            Some("Keep note height in section geometry."),
            1,
        )];
        let mut cache = DiffSectionGeometryCache::default();
        let base = measure(&mut cache, &file, LayoutMode::Split, &[], 120, false);
        let noted = measure(&mut cache, &file, LayoutMode::Split, &notes, 120, false);
        assert_eq!(base.body_height, 6);
        assert_eq!(noted.body_height, 11);
        assert_eq!(noted.hunk_anchor_rows[&0], base.hunk_anchor_rows[&0]);
        assert!(
            noted
                .row_bounds
                .iter()
                .any(|row| row.key.starts_with("inline-note:"))
        );
    }

    #[test]
    fn reuses_note_geometry_across_equivalent_arrays_and_invalidates_content() {
        let file = default_file();
        let notes = vec![note(
            &file,
            "Explain",
            Some("Keep note height in section geometry."),
            1,
        )];
        let same = notes.clone();
        let changed = vec![note(
            &file,
            "Explain this change with enough words to wrap onto another note line.",
            Some("Keep note height in section geometry."),
            1,
        )];
        let mut cache = DiffSectionGeometryCache::default();
        let first = measure(&mut cache, &file, LayoutMode::Split, &notes, 120, false);
        let equivalent = measure(&mut cache, &file, LayoutMode::Split, &same, 120, false);
        assert!(Arc::ptr_eq(&first, &equivalent));
        let changed = measure(&mut cache, &file, LayoutMode::Split, &changed, 120, false);
        assert!(!Arc::ptr_eq(&first, &changed));
        assert!(changed.body_height > first.body_height);
    }

    #[test]
    fn wraps_long_rows_into_taller_section_geometry() {
        let file = test_file(
            "const alpha = 1;\nconst beta = 2;\n",
            concat!(
                "const alpha = 1;\n",
                "const beta = 'this is a deliberately long line that should wrap in a narrow viewport';\n"
            ),
            "wrapped",
            "wrapped.ts",
        );
        let mut cache = DiffSectionGeometryCache::default();
        let nowrap = measure(&mut cache, &file, LayoutMode::Stack, &[], 32, false);
        let wrapped = measure(&mut cache, &file, LayoutMode::Stack, &[], 32, true);
        assert_eq!(nowrap.body_height, 4);
        assert_eq!(wrapped.body_height, 7);
        assert!(wrapped.hunk_bounds[&0].height > nowrap.hunk_bounds[&0].height);
    }

    #[test]
    fn explicit_line_number_width_is_part_of_geometry_cache_identity() {
        let file = default_file();
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = DiffSectionGeometryCache::default();
        let natural = cache.measure(DiffSectionGeometryOptions::new(
            &file,
            LayoutMode::Stack,
            &theme,
        ));
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Stack, &theme);
        options.line_number_digits = Some(4);
        let fixed = cache.measure(options);

        assert_eq!(fixed.line_number_digits, 4);
        assert!(!Arc::ptr_eq(&natural, &fixed));
    }

    #[test]
    fn returns_one_row_placeholder_for_files_without_visible_hunks() {
        let mut file = default_file();
        file.hunks.clear();
        let mut cache = DiffSectionGeometryCache::default();
        let geometry = measure(&mut cache, &file, LayoutMode::Split, &[], 0, false);
        assert_eq!(geometry.body_height, 1);
        assert!(geometry.hunk_bounds.is_empty());
        assert!(geometry.row_bounds.is_empty());
    }

    #[test]
    fn measures_a_header_only_hunk_stream_without_line_rows() {
        let mut file = test_file(
            "const alpha = 1;\n",
            "const alpha = 2;\n",
            "header-only",
            "header-only.ts",
        );
        file.flags.partial = true;
        file.hunks[0].lines.clear();
        file.hunks[0].old_count = 0;
        file.hunks[0].new_count = 0;
        let mut cache = DiffSectionGeometryCache::default();
        let geometry = measure(&mut cache, &file, LayoutMode::Split, &[], 0, false);
        assert_eq!(geometry.body_height, 1);
        assert_eq!(geometry.hunk_anchor_rows[&0], 0);
        assert_eq!(geometry.hunk_bounds[&0].height, 1);
        assert_eq!(geometry.row_bounds.len(), 1);
        assert!(geometry.row_bounds[0].key.contains(":header:"));
    }

    #[test]
    fn expanding_trailing_gap_grows_body_without_stretching_hunk_bounds() {
        let before = (1..=30)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before.replace("line 5\n", "line 5 modified\n");
        let file = test_file(&before, &after, "expand", "expand.txt");
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = DiffSectionGeometryCache::default();
        let collapsed = cache.measure(DiffSectionGeometryOptions::new(
            &file,
            LayoutMode::Split,
            &theme,
        ));
        let keys = HashSet::from(["trailing:0".into()]);
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.expanded_keys = &keys;
        options.source_status = ExpandedSourceStatus::Loaded(&after);
        let expanded = cache.measure(options);
        let synthesized = expanded.row_bounds.len() - collapsed.row_bounds.len();
        assert_eq!(synthesized, 25);
        assert_eq!(expanded.body_height, collapsed.body_height + synthesized);
        assert_eq!(
            expanded.hunk_anchor_rows[&0],
            collapsed.hunk_anchor_rows[&0]
        );
        assert_eq!(
            expanded.hunk_bounds[&0].height,
            collapsed.hunk_bounds[&0].height
        );
    }

    #[test]
    fn expanding_leading_gap_shifts_anchor_but_not_following_hunk_bounds() {
        let before = (1..=40)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before.replace("line 35\n", "line 35 modified\n");
        let file = test_file(&before, &after, "expand-leading", "leading.txt");
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = DiffSectionGeometryCache::default();
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.show_hunk_headers = false;
        let collapsed = cache.measure(options);
        let keys = HashSet::from(["before:0".into()]);
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Split, &theme);
        options.show_hunk_headers = false;
        options.expanded_keys = &keys;
        options.source_status = ExpandedSourceStatus::Loaded(&after);
        let expanded = cache.measure(options);
        let synthesized = expanded.row_bounds.len() - collapsed.row_bounds.len();
        assert!(synthesized > 0);
        assert_eq!(expanded.body_height, collapsed.body_height + synthesized);
        assert_eq!(
            expanded.hunk_bounds[&0].height,
            collapsed.hunk_bounds[&0].height
        );
        assert_eq!(
            expanded.hunk_anchor_rows[&0],
            collapsed.hunk_anchor_rows[&0] + synthesized
        );
    }

    #[test]
    fn expanded_context_uses_expanded_line_number_width_for_wrapping() {
        let mut before_lines = vec!["x".to_owned(); 1_000];
        before_lines[4] = "old".into();
        before_lines[999] = "abcdefghij".into();
        let mut after_lines = before_lines.clone();
        after_lines[4] = "new".into();
        let before = format!("{}\n", before_lines.join("\n"));
        let after = format!("{}\n", after_lines.join("\n"));
        let file = test_file(
            &before,
            &after,
            "large-expanded-gutter",
            "large-expanded-gutter.txt",
        );
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let keys = HashSet::from(["trailing:0".into()]);
        let mut cache = DiffSectionGeometryCache::default();
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Stack, &theme);
        options.width = 20;
        options.expanded_keys = &keys;
        options.source_status = ExpandedSourceStatus::Loaded(&after);
        let nowrap = cache.measure(options);
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Stack, &theme);
        options.width = 20;
        options.wrap_lines = true;
        options.expanded_keys = &keys;
        options.source_status = ExpandedSourceStatus::Loaded(&after);
        let wrapped = cache.measure(options);
        assert_eq!(wrapped.body_height, nowrap.body_height + 1);
        assert_eq!(wrapped.line_number_digits, 4);
    }

    #[test]
    fn same_length_source_edits_invalidate_note_aware_expanded_geometry() {
        let before_lines = (1..=30)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>();
        let mut after_lines = before_lines.clone();
        after_lines[4] = "line 5 modified".into();
        let before = format!("{}\n", before_lines.join("\n"));
        let after = format!("{}\n", after_lines.join("\n"));
        let file = test_file(
            &before,
            &after,
            "same-length-source",
            "same-length-source.txt",
        );
        let notes = vec![note(
            &file,
            "Changed line",
            Some("Forces note-aware geometry caching."),
            5,
        )];
        let keys = HashSet::from(["trailing:0".into()]);
        let mut short_lines = after_lines.clone();
        let mut long_lines = after_lines;
        let short = "short";
        let long = "this is a deliberately long expanded source line";
        short_lines[0].push_str(&"x".repeat(long.len() - short.len()));
        short_lines[8] = short.into();
        long_lines[8] = long.into();
        let short_source = format!("{}\n", short_lines.join("\n"));
        let long_source = format!("{}\n", long_lines.join("\n"));
        assert_eq!(short_source.len(), long_source.len());
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut cache = DiffSectionGeometryCache::default();
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Stack, &theme);
        options.visible_agent_notes = &notes;
        options.width = 24;
        options.wrap_lines = true;
        options.expanded_keys = &keys;
        options.source_status = ExpandedSourceStatus::Loaded(&short_source);
        let short_geometry = cache.measure(options);
        let mut options = DiffSectionGeometryOptions::new(&file, LayoutMode::Stack, &theme);
        options.visible_agent_notes = &notes;
        options.width = 24;
        options.wrap_lines = true;
        options.expanded_keys = &keys;
        options.source_status = ExpandedSourceStatus::Loaded(&long_source);
        let long_geometry = cache.measure(options);
        assert!(long_geometry.body_height > short_geometry.body_height);
    }

    #[test]
    fn note_cache_identity_covers_threads_actions_and_drafts_do_not_escape_snapshot() {
        let file = default_file();
        let mut threaded = note(&file, "Threaded", None, 1);
        threaded.thread = Some(VisibleAgentNoteThread {
            note_id: threaded.id.clone(),
            parent_id: Some("root".into()),
            depth: 1,
            has_next_sibling: None,
            ancestor_has_next_sibling: Vec::new(),
        });
        threaded.actions = Some(VisibleAgentNoteActions {
            edit: true,
            reply: true,
            delete: false,
        });
        threaded.draft = Some(VisibleAgentNoteDraft {
            body: "ignored by saved-card geometry".into(),
            focused: false,
        });
        let mut changed_depth = threaded.clone();
        changed_depth.thread.as_mut().unwrap().depth = 2;
        let mut cache = DiffSectionGeometryCache::default();
        let first = measure(
            &mut cache,
            &file,
            LayoutMode::Split,
            &[threaded],
            120,
            false,
        );
        let changed = measure(
            &mut cache,
            &file,
            LayoutMode::Split,
            &[changed_depth],
            120,
            false,
        );
        assert!(!Arc::ptr_eq(&first, &changed));
    }
}
