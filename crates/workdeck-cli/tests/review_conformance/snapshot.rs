//! Hunk MIT saved-note snapshot corpus through the public native projection.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};
use serde_json::{Value, json};
use workdeck_core::{Changeset, ChangesetSource, LineRange, ReviewSide};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::{
    CommentAnchor, ReviewComment, ReviewNoteResolution, ReviewState,
    build_extension_review_snapshot_with_generation,
};
use workdeck_tui::{ReviewApp, ReviewOptions, render};

pub(super) const CONSUMER: super::models::Consumer<fn(bool) -> Value> =
    super::models::Consumer::new("extension review snapshot", "extension API v8", projection);

fn comment(
    id: &str,
    file: &str,
    hunk: usize,
    line: u32,
    user: bool,
    resolution: ReviewNoteResolution,
) -> ReviewComment {
    ReviewComment {
        id: id.into(),
        parent_id: (id == "reply").then(|| "live".into()),
        source: if user { "user" } else { "agent" }.into(),
        author: None,
        created_at: None,
        file_path: None,
        hunk_index: None,
        side: None,
        line: None,
        summary: format!("note {id}"),
        rationale: None,
        markup: None,
        title: None,
        tags: Vec::new(),
        confidence: None,
        updated_at: None,
        resolution,
        anchor: CommentAnchor {
            file_key: file.into(),
            old_range: None,
            new_range: Some(LineRange {
                start: line,
                end: line,
            }),
            preferred_side: Some(ReviewSide::New),
            preferred_line: Some(line),
            intersecting_hunk_indices: vec![hunk],
            owner_hunk_index: Some(hunk),
        },
        editable: false,
    }
}

fn changeset() -> Changeset {
    let before = (1..=13)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    let after = before
        .replace("line 2\n", "line two\n")
        .replace("line 12\n", "line twelve\n");
    let files = [("alpha", "content:alpha:v2"), ("beta", "content:beta:v1")]
        .into_iter()
        .map(|(key, identity)| {
            let path = format!("{key}.ts");
            let mut file = diff_from_file_snapshots(
                FileSnapshot {
                    cache_key: "before",
                    contents: &before,
                    name: &path,
                },
                FileSnapshot {
                    cache_key: "after",
                    contents: &after,
                    name: &path,
                },
                FileComparisonOptions { context_radius: 1 },
            )
            .unwrap();
            assert_eq!(file.hunks.len(), 2);
            file.key = key.into();
            file.runtime_id = key.into();
            file.content_identity = identity.into();
            file
        })
        .collect();
    Changeset {
        id: "conformance".into(),
        source_label: "conformance".into(),
        title: "conformance".into(),
        summary: None,
        agent_summary: None,
        source: ChangesetSource::WorkingTree { staged: false },
        files,
    }
}

fn projection(include_reply: bool) -> Value {
    use ReviewNoteResolution::{Active, Orphaned, Stale};
    let changeset = changeset();
    let mut state = ReviewState::new(changeset.clone());
    for note in [
        comment("live", "alpha", 0, 2, false, Active),
        comment("orphaned", "retired", 0, 1, false, Orphaned),
        comment("user-stale", "beta", 1, 12, true, Stale),
    ] {
        state.add_comment(note).unwrap();
    }
    if include_reply {
        state
            .add_comment(comment("reply", "alpha", 0, 2, true, Active))
            .unwrap();
    } else {
        // The stable fixture has one fewer saved note but the same explicit
        // revision. Use a real non-exported layout transition to reach it.
        state.set_layout(workdeck_review::LayoutMode::Split);
    }
    state.select_file(1).unwrap();
    state.select_file(0).unwrap();
    assert_eq!(state.state_revision(), 6);
    let mut app = ReviewApp::new(
        changeset,
        ReviewOptions {
            sidebar: false,
            highlight: false,
            prompt_save_view_preferences: false,
            ..Default::default()
        },
    );
    *app.shared_state().lock().unwrap() = state;
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    for character in "not saved".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let area = Rect::new(0, 0, 140, 40);
    let mut buffer = Buffer::empty(area);
    render(area, &mut buffer, &app);
    let text = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        text.contains("not saved"),
        "the real terminal draft must be visible"
    );
    let shared = app.shared_state();
    let state = shared.lock().unwrap();
    let snapshot =
        build_extension_review_snapshot_with_generation("generation:conformance:3", &state);
    assert_eq!(state.comments().len(), if include_reply { 4 } else { 3 });
    assert_eq!(snapshot.state_revision, 6);
    let notes = snapshot.notes.iter().map(|note| {
        let mut value = json!({"id": note.id, "fileKey": note.file_key,
            "resolution": note.resolution, "intersectingHunkIndices": note.anchor.intersecting_hunk_indices});
        if let Some(parent) = &note.parent_id { value["parentId"] = json!(parent); }
        if let Some(preferred) = &note.anchor.preferred { value["preferred"] = json!(preferred); }
        if let Some(owner) = note.anchor.owner_hunk_index { value["ownerHunkIndex"] = json!(owner); }
        value
    }).collect::<Vec<_>>();
    json!({"generation": snapshot.generation, "stateRevision": snapshot.state_revision,
        "files": snapshot.files.iter().map(|file| json!({"fileKey": file.file_key,
            "contentIdentity": file.content_identity})).collect::<Vec<_>>(), "notes": notes})
}

#[test]
fn complete_saved_note_snapshot_excludes_a_real_unsaved_terminal_draft() {
    for (encoded, include_reply) in [
        (
            include_str!("../../../../port/hunk/oracles/review-conformance-main.json"),
            true,
        ),
        (
            include_str!("../../../../port/hunk/oracles/review-conformance-stable.json"),
            false,
        ),
    ] {
        let actual = (CONSUMER.project)(include_reply);
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let cases = oracle["results"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["group"] == "snapshot")
            .collect::<Vec<_>>();
        assert_eq!(cases.len(), 1);
        assert_eq!(
            super::models::snapshot(&actual),
            super::models::snapshot(&cases[0]["expected"])
        );
        assert_eq!(actual, cases[0]["expected"]);
        assert_eq!(cases[0]["actual"][0]["consumer"], CONSUMER.name);
        assert_eq!(actual, cases[0]["actual"][0]["output"]);
    }
}
