//! Partial translation of Hunk's MIT-licensed review-conformance geometry corpus.
//! Other consumer families remain unmapped until independently exercised in Rust.

use serde_json::{Value, json};
use workdeck_core::{DiffFile, FileChangeKind, project_review_file, review_empty_diff_reason};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::{
    review_default_hunk_line_target, review_gap_address, review_gap_id, review_gap_source_for_file,
    review_hunk_ranges, review_leading_gap, review_trailing_gap,
};

fn fixture(id: &str) -> (DiffFile, Option<&'static str>, String) {
    let base: Vec<_> = (1..=12).map(|line| format!("line {line}")).collect();
    let mut edited = base.clone();
    let (name, context, expansion) = match id {
        "pure-insertion-hunk" => {
            edited.insert(6, "inserted".into());
            ("insertion.ts", 0, Some("before:0"))
        }
        "pure-deletion-hunk" => {
            edited.remove(5);
            ("deletion.ts", 0, Some("before:0"))
        }
        "hunk-with-leading-context" => ("context.ts", 3, None),
        "crlf-source" => ("crlf.ts", 0, Some("trailing:0")),
        "source-without-trailing-newline" => ("unterminated.ts", 0, Some("trailing:0")),
        "binary-rename-with-no-rows" => ("asset.png", 0, None),
        _ => panic!("untranslated geometry fixture: {id}"),
    };
    if matches!(name, "context.ts" | "crlf.ts" | "unterminated.ts") {
        edited[5] = "line six".into();
    }
    let encode = |lines: &[String]| {
        let separator = if name == "crlf.ts" { "\r\n" } else { "\n" };
        let mut text = lines.join(separator);
        if name != "unterminated.ts" {
            text.push_str(separator);
        }
        text
    };
    let before = encode(&base);
    let after = encode(&edited);
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "before",
            contents: &before,
            name,
        },
        FileSnapshot {
            cache_key: "after",
            contents: &after,
            name,
        },
        FileComparisonOptions {
            context_radius: context,
        },
    )
    .unwrap();
    if name == "asset.png" {
        file.flags.binary = true;
        file.previous_path = Some("old-asset.png".into());
        file.change_kind = FileChangeKind::Renamed;
        file.hunks.clear();
    }
    (file, expansion, after)
}

fn core_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
    let source = review_gap_source_for_file(file);
    let gaps = (0..file.hunks.len())
        .filter_map(|index| review_leading_gap(&source, index))
        .chain(review_trailing_gap(&source))
        .map(|gap| {
            json!({
                "gapId": review_gap_id(gap.position, gap.hunk_index),
                "oldRange": [gap.old_range.start, gap.old_range.end],
                "newRange": [gap.new_range.start, gap.new_range.end],
                "lineCount": gap.line_count,
            })
        })
        .collect::<Vec<_>>();
    let ranges = file
        .hunks
        .iter()
        .map(|hunk| {
            let (old, new) = review_hunk_ranges(hunk);
            json!({"oldRange": [old.start, old.end], "newRange": [new.start, new.end]})
        })
        .collect::<Vec<_>>();
    let targets = file
        .hunks
        .iter()
        .map(|hunk| {
            let target = review_default_hunk_line_target(hunk);
            json!({"side": target.side, "line": target.line})
        })
        .collect::<Vec<_>>();
    let mut value = json!({"path": file.path, "gaps": gaps,
        "hunkRanges": ranges, "defaultNoteTargets": targets});
    if file.hunks.is_empty() {
        let canonical = project_review_file(file, "/repo", 0);
        value["emptyDiffReason"] = json!(review_empty_diff_reason(
            canonical.change_kind,
            canonical.flags.binary,
            canonical.flags.too_large
        ));
    }
    if let Some(id) = expansion {
        let gap = review_gap_address(&source, id).expect("fixture gap exists");
        let lines = workdeck_review::normalized_review_source_lines(source_text);
        value["expandedRows"] = json!(
            (0..gap.line_count)
                .map(|offset| json!({
                    "oldLine": gap.old_range.start as usize + offset,
                    "newLine": gap.new_range.start as usize + offset,
                    "text": lines[gap.new_range.start as usize + offset - 1],
                }))
                .collect::<Vec<_>>()
        );
    }
    json!({"files": [value]})
}

#[test]
fn parsed_core_geometry_matches_both_pinned_conformance_oracles() {
    for encoded in [
        include_str!("../../../port/hunk/oracles/review-conformance-main.json"),
        include_str!("../../../port/hunk/oracles/review-conformance-stable.json"),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let mut count = 0;
        for case in oracle["results"].as_array().unwrap() {
            if case["group"] != "geometry" {
                continue;
            }
            let id = case["id"].as_str().unwrap();
            let (file, expansion, source) = fixture(id);
            let actual = core_projection(&file, expansion, &source);
            assert_eq!(actual, case["expected"], "{}: {id}", oracle["upstream"]);
            let captured = case["actual"]
                .as_array()
                .unwrap()
                .iter()
                .find(|consumer| consumer["consumer"] == "core review model")
                .expect("actual pinned core consumer output");
            assert_eq!(actual, captured["output"], "captured core: {id}");
            count += 1;
        }
        assert_eq!(count, 6);
    }
}
