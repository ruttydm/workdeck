//! Hunk MIT event corpus through the shared protocol and a real native HTTP listener.

use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use workdeck_review::{
    REVIEW_PATCH_CONTENT_TYPE, ReviewAssemblyResult, ReviewAssemblyStep, ReviewResourceAddress,
    ReviewResourceKind, review_resource_id,
};
use workdeck_session::*;

const SESSION_ID: &str = "session-conformance";
const GENERATION: &str = "generation:conformance:1";

type EventProjection = fn(&WorkdeckReviewPublicationBodyV1, u64) -> Value;
pub(super) const CONSUMERS: [(&str, EventProjection); 2] = [
    ("review event protocol", protocol_projection),
    ("browser review HTTP surface", http_projection),
];

#[derive(Clone, Copy)]
enum Window {
    Bytes(u64),
    PayloadSize,
    PayloadSizeMinusOne,
}

fn resolve_window(window: Window, payload_bytes: u64) -> u64 {
    match window {
        Window::Bytes(bytes) => bytes,
        Window::PayloadSize => payload_bytes,
        Window::PayloadSizeMinusOne => payload_bytes.saturating_sub(1).max(1),
    }
}

fn collapse_chunk_run(mut names: Vec<String>) -> Vec<String> {
    names.dedup();
    names
}

fn fixture(id: &str) -> (WorkdeckReviewPublicationBodyV1, u64) {
    let count = match id {
        "publication-in-one-frame" => 1,
        "publication-split-across-chunks" => 24,
        "publication-exactly-one-window" | "publication-one-byte-over-a-window" => 3,
        _ => panic!("untranslated event fixture: {id}"),
    };
    let keys = (0..count)
        .map(|index| format!("file:{index:016x}"))
        .collect::<Vec<_>>();
    let runtime_keys = keys
        .iter()
        .enumerate()
        .map(|(index, key)| (format!("file-{index}"), json!(key)))
        .collect::<serde_json::Map<_, _>>();
    let resources = keys
        .iter()
        .map(|key| {
            json!({
                "id": review_resource_id(&ReviewResourceAddress {
                    kind: ReviewResourceKind::Patch, file_key: key.clone(), side: None,
                }),
                "generation": GENERATION, "fileKey": key, "kind": "patch",
                "contentType": REVIEW_PATCH_CONTENT_TYPE,
            })
        })
        .collect::<Vec<_>>();
    // Build the provider-neutral native publication model before measuring its
    // serialized window, just as the source corpus measures its own model.
    let body: WorkdeckReviewPublicationBodyV1 = serde_json::from_value(json!({
        "protocolVersion": WORKDECK_REVIEW_PROTOCOL_VERSION, "sessionId": SESSION_ID,
        "publication": {"generation": GENERATION, "stateRevision": 4},
        "catalog": {"generation": GENERATION, "fileKeysByRuntimeId": runtime_keys,
            "resources": resources},
    }))
    .unwrap();
    let bytes = serde_json::to_vec(&body).unwrap().len() as u64;
    let window = match id {
        "publication-in-one-frame" => Window::Bytes(64 * 1024),
        "publication-split-across-chunks" => Window::Bytes(256),
        "publication-exactly-one-window" => Window::PayloadSize,
        "publication-one-byte-over-a-window" => Window::PayloadSizeMinusOne,
        _ => unreachable!(),
    };
    (body, resolve_window(window, bytes))
}

fn summarize(frames: &[ReviewEventSseFrame], expected: &[u8]) -> Value {
    assert!(!frames.is_empty());
    let round_trips = if frames.len() == 1 {
        parse_review_event_frame(&frames[0].data)
            .is_some_and(|frame| serde_json::to_vec(&frame.payload).unwrap() == expected)
    } else {
        let begin = parse_review_event_begin(&frames[0].data).unwrap();
        let end = parse_review_event_end(&frames.last().unwrap().data).unwrap();
        let mut assembler =
            ReviewEventAssembler::new(begin, Arc::new(workdeck_core::review_digest));
        for frame in &frames[1..frames.len() - 1] {
            let chunk = parse_review_event_chunk(&frame.data).unwrap();
            assert!(matches!(
                assembler.accept(&chunk, &STANDARD.decode(&chunk.data).unwrap()),
                ReviewAssemblyStep::Accepted { .. }
            ));
        }
        match assembler.finish(&end) {
            ReviewAssemblyResult::Assembled { bytes } => bytes == expected,
            ReviewAssemblyResult::Failed(_) => false,
        }
    };
    let names = frames
        .iter()
        .map(|frame| frame.event.clone())
        .collect::<Vec<_>>();
    let names = collapse_chunk_run(names);
    json!({"frames": names, "resumableFrames": frames.iter().filter(|frame| frame.id.is_some()).count(),
        "roundTrips": round_trips})
}

fn protocol_projection(body: &WorkdeckReviewPublicationBodyV1, window: u64) -> Value {
    let payload = serde_json::to_vec(body).unwrap();
    let frames = plan_review_event_frames(PlanReviewEventInput {
        event_type: ReviewEventTypeV1::Publication,
        address: &body.publication,
        body: serde_json::to_value(body).unwrap(),
        payload: &payload,
        content_digest: &workdeck_core::review_digest(&payload),
        encode_chunk: |bytes: &[u8]| STANDARD.encode(bytes),
        chunk_bytes: Some(window),
    })
    .unwrap();
    summarize(&frames, &payload)
}

struct FixtureSocket;

impl DaemonSessionSocket for FixtureSocket {
    fn send(&self, _data: &str) -> Result<bool, SessionBrokerStateError> {
        Ok(true)
    }
}

fn http_projection(body: &WorkdeckReviewPublicationBodyV1, window: u64) -> Value {
    let state = Arc::new(WorkdeckSessionBrokerState::default());
    let capability = "A".repeat(REVIEW_CAPABILITY_TOKEN_LENGTH);
    assert_eq!(
        state.register_session(
            Arc::new(FixtureSocket),
            &json!({
                "registrationVersion": SESSION_BROKER_REGISTRATION_VERSION,
                "sessionId": SESSION_ID, "pid": std::process::id(), "cwd": "/repo",
                "launchedAt": "2024-01-01T00:00:00.000Z",
                "info": {"inputKind": "vcs", "title": "conformance", "sourceLabel": "/repo",
                    "files": [], "reviewCatalog": body.catalog,
                    "reviewCapabilityDigest": workdeck_core::review_digest(capability.as_bytes())},
            }),
            &json!({"updatedAt": "2024-01-01T00:00:00.000Z", "state": {
                "selectedHunkIndex": 0, "showAgentNotes": false, "liveCommentCount": 0,
                "liveComments": [], "reviewPublication": body.publication,
            }}),
            RegisterSessionOptions::default()
        ),
        RegisterSessionResult::Registered
    );

    let review = BrowserReviewServer::new(
        state.clone(),
        BrowserReviewServerOptions {
            event_chunk_bytes: window,
            ..Default::default()
        },
    );
    let daemon = SessionBrokerDaemon::new(SessionBrokerDaemonOptions::new(state.clone())).unwrap();
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.handle_request = Some(review.handler());
    let server = serve_session_broker_daemon(options).unwrap();
    let config = ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(5)))
        .build();
    let agent: ureq::Agent = config.into();
    let url = format!(
        "http://{}{}",
        server.address(),
        review_http_path(&WorkdeckReviewHttpRoute::Events {
            session_id: SESSION_ID.into(),
        })
    );
    let mut response = agent
        .get(&url)
        .header(WORKDECK_REVIEW_CAPABILITY_HEADER, &capability)
        .call()
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let frames = read_one_event(BufReader::new(response.body_mut().as_reader()));
    drop(response);
    review.close();
    server.stop();
    assert!(server.wait_stopped(Duration::from_secs(5)));
    state.shutdown(None);
    summarize(&frames, &serde_json::to_vec(body).unwrap())
}

fn read_one_event(mut reader: impl BufRead) -> Vec<ReviewEventSseFrame> {
    let mut frames = Vec::new();
    let mut frame = ReviewEventSseFrame {
        id: None,
        event: String::new(),
        data: Value::Null,
    };
    let mut has_data = false;
    let mut bytes = 0;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).unwrap();
        assert_ne!(read, 0, "event stream ended before its resumable frame");
        bytes += read;
        assert!(
            bytes < 1024 * 1024,
            "fixture event exceeded its bounded capture"
        );
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if !frame.event.is_empty() && has_data {
                let complete = frame.id.is_some();
                frames.push(frame);
                if complete {
                    return frames;
                }
            }
            frame = ReviewEventSseFrame {
                id: None,
                event: String::new(),
                data: Value::Null,
            };
            has_data = false;
        } else if let Some(id) = line.strip_prefix("id: ") {
            frame.id = Some(id.into());
        } else if let Some(event) = line.strip_prefix("event: ") {
            frame.event = event.into();
        } else if let Some(data) = line.strip_prefix("data: ") {
            frame.data = serde_json::from_str(data).unwrap();
            has_data = true;
        }
    }
}

#[test]
fn framing_helpers_preserve_window_edges_and_only_collapse_adjacent_names() {
    assert_eq!(resolve_window(Window::Bytes(256), 1000), 256);
    assert_eq!(resolve_window(Window::PayloadSize, 1000), 1000);
    assert_eq!(resolve_window(Window::PayloadSizeMinusOne, 1000), 999);
    assert_eq!(resolve_window(Window::PayloadSize, 0), 0);
    assert_eq!(resolve_window(Window::PayloadSizeMinusOne, 0), 1);
    assert_eq!(resolve_window(Window::PayloadSizeMinusOne, 1), 1);
    assert!(collapse_chunk_run(Vec::new()).is_empty());
    let names = ["begin", "chunk", "chunk", "end", "chunk", "end"];
    assert_eq!(
        collapse_chunk_run(names.map(str::to_owned).to_vec()),
        ["begin", "chunk", "end", "chunk", "end"]
    );
}

#[test]
fn protocol_and_real_http_surface_match_all_pinned_event_windows() {
    for encoded in [
        include_str!("../../../../port/hunk/oracles/review-conformance-main.json"),
        include_str!("../../../../port/hunk/oracles/review-conformance-stable.json"),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let mut count = 0;
        for case in oracle["results"].as_array().unwrap() {
            if case["group"] != "events" {
                continue;
            }
            let (body, window) = fixture(case["id"].as_str().unwrap());
            for (name, project) in CONSUMERS {
                let actual = project(&body, window);
                assert_eq!(
                    actual, case["expected"],
                    "{}: {name}: {}",
                    case["id"], oracle["upstream"]
                );
                let captured = case["actual"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|consumer| consumer["consumer"] == name)
                    .unwrap();
                assert_eq!(
                    actual, captured["output"],
                    "captured {name}: {}",
                    case["id"]
                );
            }
            count += 1;
        }
        assert_eq!(count, 4);
    }
}
