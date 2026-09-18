//! Pinned wire snapshot for the Workdeck session registration and snapshot payloads.
//!
//! Guards `WORKDECK_SESSION_DAEMON_VERSION`: the daemon and every window exchange this
//! revision in the signed hello and require an exact match, so any change to what a session
//! puts on the wire must bump it. The fixture filename embeds the revision; a payload change
//! against an existing fixture for the current revision is the failure this test exists to
//! produce.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use workdeck_core::{ChangesetSource, ReviewNoteSource, ReviewSide};
use workdeck_diff::parse_patch;
use workdeck_review::ReviewPublicationAddress;
use workdeck_review::build_review_publication;
use workdeck_session::{
    SessionLiveCommentSummary, SessionReviewNoteSummary, SessionTerminalLocation,
    SessionTerminalMetadata, WORKDECK_SESSION_DAEMON_VERSION, WorkdeckExperimentalFeature,
    WorkdeckSessionInputKind, WorkdeckSessionRegistration, WorkdeckSessionSnapshot,
    WorkdeckSessionState, create_initial_session_snapshot, create_session_registration,
    default_session_broker_admin_paths, parse_workdeck_session_registration,
    parse_workdeck_session_snapshot, update_session_registration,
};

const FIXTURE_DIR: &str = "tests/fixtures";
const REGENERATE_ENV: &str = "WORKDECK_GENERATE_SESSION_WIRE";
const REGENERATE_COMMAND: &str =
    "WORKDECK_GENERATE_SESSION_WIRE=1 cargo test -p workdeck-session --test session_wire_snapshot";

const FIXED_SESSION_ID: &str = "00000000-0000-4000-8000-000000000000";
const FIXED_TIMESTAMP: &str = "2026-01-01T00:00:00.000Z";
const FIXED_CAPABILITY_DIGEST: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
const FIXED_GENERATION: &str = "generation:wire-corpus:1";
const NEXT_GENERATION: &str = "generation:wire-corpus:2";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionWireCorpusEntry {
    name: String,
    registration: Value,
    snapshot: Value,
}

/// Every launch mode the session surface registers, with a rename and an added file in the
/// changeset so optional per-file fields appear on the wire.
const LAUNCH_MODES: [(&str, WorkdeckSessionInputKind); 6] = [
    ("diff", WorkdeckSessionInputKind::Diff),
    ("vcs", WorkdeckSessionInputKind::Vcs),
    ("show", WorkdeckSessionInputKind::Show),
    ("stash-show", WorkdeckSessionInputKind::StashShow),
    ("patch", WorkdeckSessionInputKind::Patch),
    ("difftool", WorkdeckSessionInputKind::Difftool),
];

fn corpus_changeset() -> workdeck_core::Changeset {
    parse_patch(
        "diff --git a/src/renamed.ts b/src/renamed.ts\n\
         similarity index 90%\n\
         rename from src/original.ts\n\
         rename to src/renamed.ts\n\
         index 1111111..2222222 100644\n\
         --- a/src/original.ts\n\
         +++ b/src/renamed.ts\n\
         @@ -1,2 +1,2 @@\n\
         -export const value = 1;\n\
         +export const value = 2;\n\
         diff --git a/src/added.ts b/src/added.ts\n\
         new file mode 100644\n\
         index 0000000..3333333\n\
         --- /dev/null\n\
         +++ b/src/added.ts\n\
         @@ -0,0 +1 @@\n\
         +export const added = true;\n",
        "/repo",
        "wire corpus",
        ChangesetSource::WorkingTree { staged: false },
    )
    .expect("the corpus patch parses")
}

fn pinned_terminal() -> SessionTerminalMetadata {
    SessionTerminalMetadata {
        program: Some("tmux".into()),
        locations: vec![SessionTerminalLocation {
            source: "tmux".into(),
            tty: Some("/dev/ttys000".into()),
            window_id: Some("@1".into()),
            tab_id: Some("tab-1".into()),
            pane_id: Some("%1".into()),
            terminal_id: Some("terminal-1".into()),
            session_id: Some("$1".into()),
        }],
    }
}

/// Replace process-specific registration facts with fixed values without changing the shape.
fn pin_registration(
    registration: WorkdeckSessionRegistration,
    experimental: bool,
) -> WorkdeckSessionRegistration {
    let mut pinned = registration;
    pinned.session_id = FIXED_SESSION_ID.into();
    pinned.pid = 4242;
    pinned.cwd = "/repo".into();
    pinned.launched_at = FIXED_TIMESTAMP.into();
    pinned.terminal = Some(pinned_terminal());
    pinned.info.review_capability_digest = Some(FIXED_CAPABILITY_DIGEST.into());
    pinned.info.experimental_features = Some(if experimental {
        vec![WorkdeckExperimentalFeature::Stml]
    } else {
        Vec::new()
    });
    if let Some(file) = pinned.info.files.first_mut() {
        // A session from an older build may still embed patch text; keep the field on the wire.
        file.patch =
            Some("@@ -1,2 +1,2 @@\n-export const value = 1;\n+export const value = 2;\n".into());
    }
    pinned
}

/// The fullest live state a session publishes: selection, notes, comments, and its position.
fn corpus_state(registration: &WorkdeckSessionRegistration) -> WorkdeckSessionState {
    let file = &registration.info.files[0];
    let root_comment = SessionLiveCommentSummary {
        comment_id: "comment-1".into(),
        file_path: file.summary.path.clone(),
        hunk_index: 0,
        side: ReviewSide::New,
        line: 1,
        summary: "Root live comment".into(),
        rationale: Some("Explains the change".into()),
        author: Some("agent".into()),
        created_at: FIXED_TIMESTAMP.into(),
    };
    let mut reply = root_comment.clone();
    reply.comment_id = "comment-2".into();
    reply.summary = "Reply live comment".into();
    let note = SessionReviewNoteSummary {
        note_id: "note-1".into(),
        parent_id: None,
        source: ReviewNoteSource::User,
        file_path: file.summary.path.clone(),
        hunk_index: Some(0),
        old_range: Some([1, 1]),
        new_range: Some([1, 1]),
        body: "A user note".into(),
        title: Some("Note title".into()),
        author: Some("reviewer".into()),
        created_at: FIXED_TIMESTAMP.into(),
        updated_at: Some(FIXED_TIMESTAMP.into()),
        editable: true,
    };
    let mut reply_note = note.clone();
    reply_note.note_id = "note-2".into();
    reply_note.parent_id = Some("note-1".into());
    reply_note.source = ReviewNoteSource::Agent;
    reply_note.editable = false;
    WorkdeckSessionState {
        selected_file_id: Some(file.summary.id.clone()),
        selected_file_path: Some(file.summary.path.clone()),
        selected_hunk_index: 0,
        selected_hunk_old_range: Some([1, 1]),
        selected_hunk_new_range: Some([1, 1]),
        show_agent_notes: true,
        note_markup_width: Some(72),
        live_comment_count: 2,
        live_comments: vec![root_comment, reply],
        review_note_count: Some(2),
        review_notes: Some(vec![note, reply_note]),
        review_publication: Some(ReviewPublicationAddress {
            generation: FIXED_GENERATION.into(),
            state_revision: 3,
        }),
    }
}

fn entry(
    name: &str,
    registration: &WorkdeckSessionRegistration,
    state: WorkdeckSessionState,
) -> SessionWireCorpusEntry {
    SessionWireCorpusEntry {
        name: name.into(),
        registration: serde_json::to_value(registration).expect("registration serializes"),
        snapshot: serde_json::to_value(WorkdeckSessionSnapshot {
            updated_at: FIXED_TIMESTAMP.into(),
            state,
        })
        .expect("snapshot serializes"),
    }
}

/// Build the deterministic wire corpus: one entry per launch mode from the initial
/// registration, plus one reload entry so `update_session_registration` is covered as well.
fn build_test_session_wire_corpus() -> Vec<SessionWireCorpusEntry> {
    let mut entries = Vec::new();
    for (name, input_kind) in LAUNCH_MODES {
        let changeset = corpus_changeset();
        let experimental = input_kind == WorkdeckSessionInputKind::Vcs;
        let bootstrap = workdeck_session::SessionRegistrationBootstrap {
            input_kind,
            changeset: changeset.clone(),
            source_label: changeset.effective_source_label().to_owned(),
            experimental,
            initial_show_agent_notes: true,
        };
        let publication = build_review_publication(
            &changeset.files,
            FIXED_GENERATION,
            Some(changeset.effective_source_label()),
        );
        let registration = pin_registration(
            create_session_registration(&bootstrap, &publication).expect("registration builds"),
            experimental,
        );
        let initial = create_initial_session_snapshot(&bootstrap, &publication);
        entries.push(entry(
            &format!("{name}:initial"),
            &registration,
            initial.state,
        ));
        entries.push(entry(
            &format!("{name}:live"),
            &registration,
            corpus_state(&registration),
        ));
    }

    // One reload from the first mode into the second so the update path is on the wire too.
    let (first_kind, second_kind) = (LAUNCH_MODES[0].1, LAUNCH_MODES[1].1);
    let changeset = corpus_changeset();
    let first_bootstrap = workdeck_session::SessionRegistrationBootstrap {
        input_kind: first_kind,
        changeset: changeset.clone(),
        source_label: changeset.effective_source_label().to_owned(),
        experimental: false,
        initial_show_agent_notes: true,
    };
    let first_publication = build_review_publication(
        &changeset.files,
        FIXED_GENERATION,
        Some(changeset.effective_source_label()),
    );
    let current =
        create_session_registration(&first_bootstrap, &first_publication).expect("registration");
    let second_bootstrap = workdeck_session::SessionRegistrationBootstrap {
        input_kind: second_kind,
        changeset: changeset.clone(),
        source_label: changeset.effective_source_label().to_owned(),
        experimental: true,
        initial_show_agent_notes: false,
    };
    let second_publication = build_review_publication(
        &changeset.files,
        NEXT_GENERATION,
        Some(changeset.effective_source_label()),
    );
    let reloaded = pin_registration(
        update_session_registration(&current, &second_bootstrap, &second_publication)
            .expect("registration updates"),
        true,
    );
    let mut live_state = corpus_state(&reloaded);
    if let Some(publication) = &mut live_state.review_publication {
        publication.generation = NEXT_GENERATION.into();
    }
    entries.push(entry("reload:diff->vcs", &reloaded, live_state));
    entries
}

fn fixture_path() -> PathBuf {
    PathBuf::from(FIXTURE_DIR).join(format!(
        "session-wire.v{WORKDECK_SESSION_DAEMON_VERSION}.json"
    ))
}

fn fixture_names() -> Vec<String> {
    std::fs::read_dir(FIXTURE_DIR)
        .expect("the fixtures directory exists")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            let stem = name.strip_suffix(".json").unwrap_or(name);
            let Some(version) = stem.strip_prefix("session-wire.v") else {
                return false;
            };
            !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
        })
        .collect()
}

#[test]
fn every_corpus_payload_round_trips_through_the_daemon_parsers() {
    for corpus in build_test_session_wire_corpus() {
        let parsed = parse_workdeck_session_registration(&corpus.registration)
            .unwrap_or_else(|| panic!("{} registration must parse", corpus.name));
        assert_eq!(
            serde_json::to_value(parsed).expect("parsed registration serializes"),
            corpus.registration,
            "{}",
            corpus.name
        );
        let parsed = parse_workdeck_session_snapshot(&corpus.snapshot)
            .unwrap_or_else(|| panic!("{} snapshot must parse", corpus.name));
        assert_eq!(
            serde_json::to_value(parsed).expect("parsed snapshot serializes"),
            corpus.snapshot,
            "{}",
            corpus.name
        );
    }
}

#[test]
fn the_live_corpus_carries_every_optional_wire_field() {
    let corpus = build_test_session_wire_corpus();
    let live: Vec<&SessionWireCorpusEntry> = corpus
        .iter()
        .filter(|entry| entry.name.ends_with(":live") || entry.name.starts_with("reload:"))
        .collect();
    let mut registration_keys = BTreeSet::new();
    let mut state_keys = BTreeSet::new();
    for entry in &live {
        if let Some(info) = entry.registration.get("info").and_then(Value::as_object) {
            registration_keys.extend(info.keys().cloned());
        }
        if let Some(files) = entry
            .registration
            .pointer("/info/files")
            .and_then(Value::as_array)
        {
            for file in files {
                registration_keys.extend(
                    file.as_object()
                        .into_iter()
                        .flat_map(|file| file.keys().cloned()),
                );
                for hunk in file
                    .get("hunks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    registration_keys.extend(
                        hunk.as_object()
                            .into_iter()
                            .flat_map(|hunk| hunk.keys().cloned()),
                    );
                }
            }
        }
        if let Some(state) = entry.snapshot.get("state").and_then(Value::as_object) {
            state_keys.extend(state.keys().cloned());
            for comment in state
                .get("liveComments")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                state_keys.extend(
                    comment
                        .as_object()
                        .into_iter()
                        .flat_map(|comment| comment.keys().cloned()),
                );
            }
        }
    }
    for key in [
        "inputKind",
        "title",
        "sourceLabel",
        "experimentalFeatures",
        "files",
        "reviewCatalog",
        "reviewCapabilityDigest",
        "previousPath",
        "patch",
        "hunkCount",
        "oldRange",
        "newRange",
    ] {
        assert!(
            registration_keys.contains(key),
            "the corpus never puts {key} on the registration wire"
        );
    }
    for key in [
        "selectedFileId",
        "selectedFilePath",
        "selectedHunkOldRange",
        "selectedHunkNewRange",
        "noteMarkupWidth",
        "liveCommentCount",
        "liveComments",
        "reviewNoteCount",
        "reviewNotes",
        "reviewPublication",
        "parentId",
        "rationale",
        "author",
        "title",
        "updatedAt",
        "editable",
    ] {
        assert!(
            state_keys.contains(key)
                || live.iter().any(|entry| {
                    entry
                        .snapshot
                        .pointer("/state/reviewNotes")
                        .and_then(Value::as_array)
                        .is_some_and(|notes| {
                            notes.iter().any(|note| {
                                note.as_object().is_some_and(|note| note.contains_key(key))
                            })
                        })
                }),
            "the corpus never puts {key} on the snapshot wire"
        );
    }
}

#[test]
fn the_wire_corpus_matches_the_pinned_fixture() {
    let corpus = build_test_session_wire_corpus();
    let rendered = serde_json::to_value(&corpus).expect("the corpus serializes");
    let fixture_path = fixture_path();
    if std::env::var(REGENERATE_ENV).as_deref() == Ok("1") {
        std::fs::write(
            &fixture_path,
            serde_json::to_string_pretty(&rendered).expect("fixture serializes"),
        )
        .expect("the fixture is writable");
        panic!(
            "regenerated {}; re-run the tests without {REGENERATE_ENV}",
            fixture_path.display()
        );
    }
    let Ok(checked_in) = std::fs::read_to_string(&fixture_path) else {
        panic!(
            "No wire fixture exists for WORKDECK_SESSION_DAEMON_VERSION \
             {WORKDECK_SESSION_DAEMON_VERSION}. Run `{REGENERATE_COMMAND}` to write {}{} and \
             delete stale fixtures in the same change.",
            fixture_path.display(),
            if fixture_names().is_empty() {
                String::new()
            } else {
                format!(" and delete {}", fixture_names().join(", "))
            }
        );
    };
    // Compare JSON values rather than text so formatter reflow of the fixture is not a change.
    let checked_in: Value = serde_json::from_str(&checked_in).expect("the fixture is valid JSON");
    if checked_in != rendered {
        panic!(
            "The session wire payload changed but WORKDECK_SESSION_DAEMON_VERSION is still \
             {WORKDECK_SESSION_DAEMON_VERSION}. A daemon and a window must speak the same \
             revision, so bump WORKDECK_SESSION_DAEMON_VERSION in \
             crates/workdeck-session/src/broker_config.rs, then run `{REGENERATE_COMMAND}` and \
             delete {0} in the same change. If the payload change was unintended, revert it \
             instead.",
            fixture_path.display()
        );
    }
}

#[test]
fn exactly_one_wire_fixture_is_checked_in_for_the_current_revision() {
    assert_eq!(
        fixture_names(),
        vec![format!(
            "session-wire.v{WORKDECK_SESSION_DAEMON_VERSION}.json"
        )]
    );
}

#[test]
fn admin_paths_stay_outside_the_session_wire_namespace() {
    // The frozen admin scope is versioned separately from the session wire; pin that here so
    // the fixture cannot silently absorb an admin change.
    let paths = default_session_broker_admin_paths();
    assert!(paths.control.starts_with("/session-admin"));
}
