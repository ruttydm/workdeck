//! Strict parsing of Workdeck-owned registration and snapshot payloads.

use serde_json::{Map, Value};
use workdeck_core::{ReviewNoteSource, is_review_sha256_digest};

use crate::{
    BrokerStringOptions, MAX_REGISTRATION_FILES, MAX_REGISTRATION_HUNKS_PER_FILE,
    MAX_REGISTRATION_PATCH_BYTES, MAX_SNAPSHOT_LIVE_COMMENTS, MAX_SNAPSHOT_REVIEW_NOTES,
    SessionFileSummary, SessionLiveCommentSummary, SessionReviewFile, SessionReviewHunk,
    SessionReviewNoteSummary, WorkdeckExperimentalFeature, WorkdeckSessionInfo,
    WorkdeckSessionInputKind, WorkdeckSessionRegistration, WorkdeckSessionSnapshot,
    WorkdeckSessionState, parse_broker_string, parse_exact_broker_record,
    parse_nonnegative_broker_integer, parse_optional_broker_string, parse_positive_broker_integer,
    parse_required_broker_string, parse_session_registration_envelope,
    parse_session_snapshot_envelope, parse_workdeck_review_publication_address,
    parse_workdeck_review_resource_catalog,
};

fn exact<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Option<&'a Map<String, Value>> {
    parse_exact_broker_record(value, required, optional).ok()
}

fn parse_experimental_features(value: Option<&Value>) -> Option<Vec<WorkdeckExperimentalFeature>> {
    let Some(value) = value else {
        return Some(Vec::new());
    };
    let features = value.as_array()?;
    let mut parsed = Vec::new();
    for feature in features {
        let feature = match feature.as_str()? {
            "stml" => WorkdeckExperimentalFeature::Stml,
            _ => return None,
        };
        if !parsed.contains(&feature) {
            parsed.push(feature);
        }
    }
    Some(parsed)
}

fn parse_optional_range(value: Option<&Value>) -> Option<Option<[u64; 2]>> {
    let Some(value) = value else {
        return Some(None);
    };
    let range = value.as_array()?;
    if range.len() != 2 {
        return None;
    }
    Some(Some([
        parse_nonnegative_broker_integer(&range[0])?,
        parse_nonnegative_broker_integer(&range[1])?,
    ]))
}

fn parse_session_review_hunk(value: &Value) -> Option<SessionReviewHunk> {
    let record = exact(value, &["index", "header"], &["oldRange", "newRange"])?;
    Some(SessionReviewHunk {
        index: parse_nonnegative_broker_integer(&record["index"])?,
        header: parse_required_broker_string(&record["header"])?,
        old_range: parse_optional_range(record.get("oldRange"))?,
        new_range: parse_optional_range(record.get("newRange"))?,
    })
}

fn parse_session_review_file(value: &Value) -> Option<SessionReviewFile> {
    let record = exact(
        value,
        &["id", "path", "additions", "deletions", "hunks"],
        &["previousPath", "patch", "hunkCount"],
    )?;
    let hunk_values = record["hunks"].as_array()?;
    if hunk_values.len() > MAX_REGISTRATION_HUNKS_PER_FILE {
        return None;
    }
    if record.get("hunkCount").is_some_and(|count| {
        parse_nonnegative_broker_integer(count) != u64::try_from(hunk_values.len()).ok()
    }) {
        return None;
    }
    let hunks = hunk_values
        .iter()
        .map(parse_session_review_hunk)
        .collect::<Option<Vec<_>>>()?;
    let patch = record
        .get("patch")
        .map(|patch| {
            parse_broker_string(
                patch,
                BrokerStringOptions {
                    min_bytes: 1,
                    max_bytes: MAX_REGISTRATION_PATCH_BYTES,
                },
            )
            .map(str::to_owned)
        })
        .transpose()
        .ok()?;
    Some(SessionReviewFile {
        summary: SessionFileSummary {
            id: parse_required_broker_string(&record["id"])?,
            path: parse_required_broker_string(&record["path"])?,
            previous_path: parse_optional_broker_string(record.get("previousPath")).ok()?,
            additions: parse_nonnegative_broker_integer(&record["additions"])?,
            deletions: parse_nonnegative_broker_integer(&record["deletions"])?,
            hunk_count: u64::try_from(hunks.len()).ok()?,
        },
        patch,
        hunks,
    })
}

fn parse_review_input_kind(value: &Value) -> Option<WorkdeckSessionInputKind> {
    Some(match value.as_str()? {
        "vcs" => WorkdeckSessionInputKind::Vcs,
        "show" => WorkdeckSessionInputKind::Show,
        "stash-show" => WorkdeckSessionInputKind::StashShow,
        "diff" => WorkdeckSessionInputKind::Diff,
        "patch" => WorkdeckSessionInputKind::Patch,
        "difftool" => WorkdeckSessionInputKind::Difftool,
        _ => return None,
    })
}

fn parse_session_live_comment(value: &Value) -> Option<SessionLiveCommentSummary> {
    let record = exact(
        value,
        &[
            "commentId",
            "filePath",
            "hunkIndex",
            "summary",
            "createdAt",
            "line",
            "side",
        ],
        &["rationale", "author"],
    )?;
    Some(SessionLiveCommentSummary {
        comment_id: parse_required_broker_string(&record["commentId"])?,
        file_path: parse_required_broker_string(&record["filePath"])?,
        hunk_index: parse_nonnegative_broker_integer(&record["hunkIndex"])?,
        side: serde_json::from_value(record["side"].clone()).ok()?,
        line: parse_positive_broker_integer(&record["line"])?,
        summary: parse_required_broker_string(&record["summary"])?,
        rationale: parse_optional_broker_string(record.get("rationale")).ok()?,
        author: parse_optional_broker_string(record.get("author")).ok()?,
        created_at: parse_required_broker_string(&record["createdAt"])?,
    })
}

fn parse_review_note_source(value: &Value) -> Option<ReviewNoteSource> {
    Some(match value.as_str()? {
        "ai" => ReviewNoteSource::Ai,
        "agent" => ReviewNoteSource::Agent,
        "user" => ReviewNoteSource::User,
        _ => return None,
    })
}

fn parse_session_review_note(value: &Value) -> Option<SessionReviewNoteSummary> {
    let record = exact(
        value,
        &["noteId", "source", "filePath", "body", "createdAt"],
        &[
            "parentId",
            "hunkIndex",
            "oldRange",
            "newRange",
            "title",
            "author",
            "updatedAt",
            "editable",
        ],
    )?;
    let source = parse_review_note_source(&record["source"])?;
    let hunk_index = match record.get("hunkIndex") {
        Some(value) => Some(parse_nonnegative_broker_integer(value)?),
        None => None,
    };
    let editable = match record.get("editable") {
        Some(value) => value.as_bool()?,
        None => source == ReviewNoteSource::User,
    };
    Some(SessionReviewNoteSummary {
        note_id: parse_required_broker_string(&record["noteId"])?,
        parent_id: parse_optional_broker_string(record.get("parentId")).ok()?,
        source,
        file_path: parse_required_broker_string(&record["filePath"])?,
        hunk_index,
        old_range: parse_optional_range(record.get("oldRange"))?,
        new_range: parse_optional_range(record.get("newRange"))?,
        body: parse_required_broker_string(&record["body"])?,
        title: parse_optional_broker_string(record.get("title")).ok()?,
        author: parse_optional_broker_string(record.get("author")).ok()?,
        created_at: parse_required_broker_string(&record["createdAt"])?,
        updated_at: parse_optional_broker_string(record.get("updatedAt")).ok()?,
        editable,
    })
}

fn parse_workdeck_session_info(value: &Value) -> Option<WorkdeckSessionInfo> {
    let record = exact(
        value,
        &["inputKind", "title", "sourceLabel", "files"],
        &[
            "experimentalFeatures",
            "reviewCatalog",
            "reviewCapabilityDigest",
        ],
    )?;
    let file_values = record["files"].as_array()?;
    if file_values.len() > MAX_REGISTRATION_FILES {
        return None;
    }
    let files = file_values
        .iter()
        .map(parse_session_review_file)
        .collect::<Option<Vec<_>>>()?;
    let review_catalog = match record.get("reviewCatalog") {
        Some(value) => Some(parse_workdeck_review_resource_catalog(value)?),
        None => None,
    };
    let review_capability_digest = match record.get("reviewCapabilityDigest") {
        Some(value) => {
            let digest = value.as_str()?;
            if !is_review_sha256_digest(digest) {
                return None;
            }
            Some(digest.to_owned())
        }
        None => None,
    };
    Some(WorkdeckSessionInfo {
        input_kind: parse_review_input_kind(&record["inputKind"])?,
        title: parse_required_broker_string(&record["title"])?,
        source_label: parse_required_broker_string(&record["sourceLabel"])?,
        experimental_features: Some(parse_experimental_features(
            record.get("experimentalFeatures"),
        )?),
        files,
        review_catalog,
        review_capability_digest,
    })
}

fn parse_workdeck_session_state(value: &Value) -> Option<WorkdeckSessionState> {
    let record = exact(
        value,
        &["liveComments", "selectedHunkIndex", "showAgentNotes"],
        &[
            "selectedFileId",
            "selectedFilePath",
            "selectedHunkOldRange",
            "selectedHunkNewRange",
            "noteMarkupWidth",
            "liveCommentCount",
            "reviewNoteCount",
            "reviewNotes",
            "reviewPublication",
        ],
    )?;
    let live_values = record["liveComments"].as_array()?;
    if live_values.len() > MAX_SNAPSHOT_LIVE_COMMENTS {
        return None;
    }
    let review_values: &[Value] = match record.get("reviewNotes") {
        Some(value) => value.as_array()?.as_slice(),
        None => &[],
    };
    if review_values.len() > MAX_SNAPSHOT_REVIEW_NOTES {
        return None;
    }
    let live_comments = live_values
        .iter()
        .map(parse_session_live_comment)
        .collect::<Option<Vec<_>>>()?;
    let review_notes = review_values
        .iter()
        .map(parse_session_review_note)
        .collect::<Option<Vec<_>>>()?;
    let live_comment_count = u64::try_from(live_comments.len()).ok()?;
    let review_note_count = u64::try_from(review_notes.len()).ok()?;
    if record
        .get("liveCommentCount")
        .is_some_and(|value| parse_nonnegative_broker_integer(value) != Some(live_comment_count))
        || record
            .get("reviewNoteCount")
            .is_some_and(|value| parse_nonnegative_broker_integer(value) != Some(review_note_count))
    {
        return None;
    }
    let review_publication = match record.get("reviewPublication") {
        Some(value) => Some(parse_workdeck_review_publication_address(value)?),
        None => None,
    };
    Some(WorkdeckSessionState {
        selected_file_id: parse_optional_broker_string(record.get("selectedFileId")).ok()?,
        selected_file_path: parse_optional_broker_string(record.get("selectedFilePath")).ok()?,
        selected_hunk_index: parse_nonnegative_broker_integer(&record["selectedHunkIndex"])?,
        selected_hunk_old_range: parse_optional_range(record.get("selectedHunkOldRange"))?,
        selected_hunk_new_range: parse_optional_range(record.get("selectedHunkNewRange"))?,
        show_agent_notes: record["showAgentNotes"].as_bool()?,
        note_markup_width: match record.get("noteMarkupWidth") {
            Some(value) => Some(parse_nonnegative_broker_integer(value)?),
            None => None,
        },
        live_comment_count,
        live_comments,
        review_note_count: Some(review_note_count),
        review_notes: Some(review_notes),
        review_publication,
    })
}

/// Parse one Workdeck session registration from the broker WebSocket wire format.
#[must_use]
pub fn parse_workdeck_session_registration(value: &Value) -> Option<WorkdeckSessionRegistration> {
    parse_session_registration_envelope(value, parse_workdeck_session_info)
}

/// Parse one Workdeck session snapshot from the broker WebSocket wire format.
#[must_use]
pub fn parse_workdeck_session_snapshot(value: &Value) -> Option<WorkdeckSessionSnapshot> {
    parse_session_snapshot_envelope(value, parse_workdeck_session_state)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use workdeck_core::ReviewSide;

    use crate::SESSION_BROKER_REGISTRATION_VERSION;

    use super::*;

    fn registration(files: Vec<Value>) -> Value {
        json!({
            "registrationVersion": SESSION_BROKER_REGISTRATION_VERSION,
            "sessionId": "session-1",
            "pid": 123,
            "cwd": "/repo",
            "launchedAt": "2026-03-22T00:00:00.000Z",
            "info": {
                "inputKind": "vcs",
                "title": "repo working tree",
                "sourceLabel": "/repo",
                "files": files,
            },
        })
    }

    fn review_file(id: &str, path: &str) -> Value {
        json!({
            "id": id,
            "path": path,
            "additions": 1,
            "deletions": 0,
            "hunks": [{"index": 0, "header": "@@ -1 +1 @@"}],
        })
    }

    fn valid_comment(id: &str) -> Value {
        json!({
            "commentId": id,
            "filePath": "src/example.ts",
            "hunkIndex": 0,
            "side": "new",
            "line": 4,
            "summary": "Review note",
            "createdAt": "2026-03-22T00:00:00.000Z",
        })
    }

    fn snapshot(state: Value) -> Value {
        json!({
            "updatedAt": "2026-03-22T00:00:00.000Z",
            "state": state,
        })
    }

    #[test]
    fn snapshot_rejects_malformed_comments_instead_of_filtering_them() {
        let parsed = parse_workdeck_session_snapshot(&snapshot(json!({
            "selectedFileId": "file-1",
            "selectedFilePath": "src/example.ts",
            "selectedHunkIndex": 0,
            "showAgentNotes": true,
            "liveCommentCount": 5,
            "liveComments": [
                valid_comment("comment-1"),
                {"filePath": "src/example.ts", "summary": "Missing id and line."},
            ],
        })));
        assert_eq!(parsed, None);
    }

    #[test]
    fn snapshot_carries_note_markup_width_and_rejects_invalid_values() {
        let parse = |width: Option<Value>| {
            let mut state = json!({
                "selectedHunkIndex": 0,
                "showAgentNotes": true,
                "liveComments": [],
            });
            if let Some(width) = width {
                state["noteMarkupWidth"] = width;
            }
            parse_workdeck_session_snapshot(&snapshot(state))
        };
        assert_eq!(
            parse(Some(json!(112))).unwrap().state.note_markup_width,
            Some(112)
        );
        assert_eq!(parse(Some(json!("wide"))), None);
        assert_eq!(parse(None).unwrap().state.note_markup_width, None);
    }

    #[test]
    fn registration_parses_nested_app_info_and_defaults_features() {
        let parsed = parse_workdeck_session_registration(&registration(Vec::new())).unwrap();
        assert_eq!(parsed.info.input_kind, WorkdeckSessionInputKind::Vcs);
        assert_eq!(parsed.info.title, "repo working tree");
        assert_eq!(parsed.info.source_label, "/repo");
        assert_eq!(parsed.info.experimental_features, Some(Vec::new()));
        assert!(parsed.info.files.is_empty());
    }

    #[test]
    fn registration_rejects_unknown_or_malformed_experimental_features() {
        let mut value = registration(Vec::new());
        value["info"]["experimentalFeatures"] = json!(["stml", "future-feature", "stml", 42]);
        assert_eq!(parse_workdeck_session_registration(&value), None);
    }

    #[test]
    fn registration_accepts_verified_review_catalog_and_capability_digest() {
        let mut value = registration(Vec::new());
        value["info"]["reviewCatalog"] = json!({
            "generation": "generation:p1:2",
            "fileKeysByRuntimeId": {},
            "resources": [],
        });
        value["info"]["reviewCapabilityDigest"] = json!("a".repeat(64));

        let parsed = parse_workdeck_session_registration(&value).unwrap();
        assert_eq!(
            parsed.info.review_catalog.unwrap().generation,
            "generation:p1:2"
        );
        assert_eq!(
            parsed.info.review_capability_digest.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn rejects_registrations_above_the_file_cap() {
        let files = (0..=MAX_REGISTRATION_FILES)
            .map(|index| review_file(&format!("file-{index}"), &format!("src/file-{index}.ts")))
            .collect();
        assert_eq!(
            parse_workdeck_session_registration(&registration(files)),
            None
        );
    }

    #[test]
    fn rejects_files_above_the_per_file_hunk_cap() {
        let hunks = (0..=MAX_REGISTRATION_HUNKS_PER_FILE)
            .map(|index| json!({"index": index, "header": format!("@@ hunk {index} @@")}))
            .collect::<Vec<_>>();
        let mut file = review_file("file-1", "src/example.ts");
        file["hunks"] = Value::Array(hunks);
        assert_eq!(
            parse_workdeck_session_registration(&registration(vec![file])),
            None
        );
    }

    #[test]
    fn accepts_legacy_patches_beyond_generic_strings_through_the_exact_cap() {
        for size in [4_097, MAX_REGISTRATION_PATCH_BYTES] {
            let mut file = review_file("file-1", "src/example.ts");
            file["patch"] = json!("x".repeat(size));
            assert_eq!(
                parse_workdeck_session_registration(&registration(vec![file]))
                    .unwrap()
                    .info
                    .files[0]
                    .patch
                    .as_ref()
                    .unwrap()
                    .len(),
                size
            );
        }
    }

    #[test]
    fn rejects_a_legacy_patch_one_byte_above_its_cap() {
        let mut file = review_file("file-1", "src/example.ts");
        file["patch"] = json!("x".repeat(MAX_REGISTRATION_PATCH_BYTES + 1));
        assert_eq!(
            parse_workdeck_session_registration(&registration(vec![file])),
            None
        );
    }

    #[test]
    fn accepts_zero_based_pure_add_delete_and_new_file_hunk_ranges() {
        let ranges = [([0, 0], [1, 3]), ([4, 2], [0, 0]), ([0, 0], [0, 4])];
        let files = ranges
            .iter()
            .enumerate()
            .map(|(index, (old, new))| {
                let mut file =
                    review_file(&format!("file-{index}"), &format!("src/file-{index}.ts"));
                file["hunks"] = json!([{
                    "index": 0,
                    "header": "@@",
                    "oldRange": old,
                    "newRange": new,
                }]);
                file
            })
            .collect();
        let files = parse_workdeck_session_registration(&registration(files))
            .unwrap()
            .info
            .files;
        for (file, (old, new)) in files.iter().zip(ranges) {
            assert_eq!(file.hunks[0].old_range, Some(old));
            assert_eq!(file.hunks[0].new_range, Some(new));
        }
    }

    #[test]
    fn accepts_zero_based_selected_and_review_note_ranges() {
        let parsed = parse_workdeck_session_snapshot(&snapshot(json!({
            "selectedHunkIndex": 0,
            "selectedHunkOldRange": [0, 0],
            "selectedHunkNewRange": [0, 3],
            "showAgentNotes": true,
            "liveComments": [],
            "reviewNotes": [{
                "noteId": "note-1",
                "parentId": "note-root",
                "source": "user",
                "filePath": "new-file.ts",
                "oldRange": [0, 0],
                "newRange": [0, 3],
                "body": "New file",
                "createdAt": "2026-03-22T00:00:00.000Z",
            }],
        })))
        .unwrap();
        assert_eq!(parsed.state.selected_hunk_old_range, Some([0, 0]));
        assert_eq!(parsed.state.selected_hunk_new_range, Some([0, 3]));
        let note = &parsed.state.review_notes.unwrap()[0];
        assert_eq!(note.parent_id.as_deref(), Some("note-root"));
        assert_eq!(note.old_range, Some([0, 0]));
        assert_eq!(note.new_range, Some([0, 3]));
    }

    #[test]
    fn rejects_snapshots_above_the_live_comment_cap() {
        let comments = (0..=MAX_SNAPSHOT_LIVE_COMMENTS)
            .map(|index| valid_comment(&format!("comment-{index}")))
            .collect::<Vec<_>>();
        assert_eq!(
            parse_workdeck_session_snapshot(&snapshot(json!({
                "selectedHunkIndex": 0,
                "showAgentNotes": true,
                "liveComments": comments,
            }))),
            None
        );
    }

    #[test]
    fn rejects_snapshots_above_the_review_note_cap() {
        let notes = (0..=MAX_SNAPSHOT_REVIEW_NOTES)
            .map(|index| {
                json!({
                    "noteId": format!("note-{index}"),
                    "source": "user",
                    "filePath": "src/example.ts",
                    "body": "Looks good",
                    "createdAt": "2026-03-22T00:00:00.000Z",
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            parse_workdeck_session_snapshot(&snapshot(json!({
                "selectedHunkIndex": 0,
                "showAgentNotes": true,
                "liveComments": [],
                "reviewNotes": notes,
            }))),
            None
        );
    }

    #[test]
    fn enforces_asserted_counts_deduplicates_features_and_defaults_editability() {
        let mut value = registration(vec![review_file("file-1", "src/example.ts")]);
        value["info"]["experimentalFeatures"] = json!(["stml", "stml"]);
        let parsed = parse_workdeck_session_registration(&value).unwrap();
        assert_eq!(
            parsed.info.experimental_features,
            Some(vec![WorkdeckExperimentalFeature::Stml])
        );

        let valid = parse_workdeck_session_snapshot(&snapshot(json!({
            "selectedHunkIndex": 0,
            "showAgentNotes": false,
            "liveCommentCount": 0,
            "liveComments": [],
            "reviewNoteCount": 1,
            "reviewNotes": [{
                "noteId": "note-1",
                "source": "user",
                "filePath": "src/example.ts",
                "body": "Looks good",
                "createdAt": "now",
            }],
        })))
        .unwrap();
        assert!(valid.state.review_notes.unwrap()[0].editable);

        let invalid = snapshot(json!({
            "selectedHunkIndex": 0,
            "showAgentNotes": false,
            "liveCommentCount": 1,
            "liveComments": [],
        }));
        assert_eq!(parse_workdeck_session_snapshot(&invalid), None);
    }

    #[test]
    fn rejects_wrong_hunk_count_invalid_digest_catalog_and_unknown_keys() {
        let mut wrong_count = review_file("file-1", "src/example.ts");
        wrong_count["hunkCount"] = json!(2);
        assert_eq!(
            parse_workdeck_session_registration(&registration(vec![wrong_count])),
            None
        );

        let mut bad_digest = registration(Vec::new());
        bad_digest["info"]["reviewCapabilityDigest"] = json!("not-a-digest");
        assert_eq!(parse_workdeck_session_registration(&bad_digest), None);

        let mut bad_catalog = registration(Vec::new());
        bad_catalog["info"]["reviewCatalog"] = json!({});
        assert_eq!(parse_workdeck_session_registration(&bad_catalog), None);

        let mut unknown = snapshot(json!({
            "selectedHunkIndex": 0,
            "showAgentNotes": false,
            "liveComments": [],
        }));
        unknown["state"]["future"] = json!(true);
        assert_eq!(parse_workdeck_session_snapshot(&unknown), None);
    }

    #[test]
    fn side_values_stay_strict() {
        let mut comment = valid_comment("comment-1");
        comment["side"] = json!("both");
        assert_eq!(
            parse_workdeck_session_snapshot(&snapshot(json!({
                "selectedHunkIndex": 0,
                "showAgentNotes": true,
                "liveComments": [comment],
            }))),
            None
        );
        assert_eq!(serde_json::to_value(ReviewSide::New).unwrap(), json!("new"));
    }
}
