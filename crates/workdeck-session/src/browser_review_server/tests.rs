use super::*;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize};

use serde_json::{Value, json};
use workdeck_review::{
    REVIEW_PATCH_CONTENT_TYPE, ReviewAssemblyResult, ReviewAssemblyStep, ReviewResourceAddress,
    ReviewResourceChunk, ReviewResourceDescriptor, ReviewResourceDescriptorBase,
    ReviewResourceKind, review_resource_id,
};

use crate::{
    BrokerCommandOutcome, DaemonSessionSocket, RegisterSessionOptions, RegisterSessionResult,
    ReviewEventAssembler, SESSION_BROKER_REGISTRATION_VERSION, SessionBrokerStateError,
    SessionFileSummary, SessionReviewFile, SessionReviewHunk, SharedDaemonSessionSocket,
    WorkdeckReviewActionAppliedV1, WorkdeckReviewActionResultV1, WorkdeckReviewResourceCatalogV1,
    WorkdeckReviewResourceReadResultV1, WorkdeckSessionInfo, WorkdeckSessionInputKind,
    WorkdeckSessionRegistration, WorkdeckSessionSnapshot, WorkdeckSessionState,
    parse_review_event_begin, parse_review_event_chunk, parse_review_event_end,
    parse_review_event_frame, parse_review_event_frame_name, review_event_id,
};

const SESSION_ID: &str = "session-http-1";
const GENERATION: &str = "generation:integration:0";

struct ReviewSocket {
    state: Mutex<Weak<WorkdeckSessionBrokerState>>,
    own_socket: Mutex<Option<Weak<dyn DaemonSessionSocket>>>,
    resources: Mutex<BTreeMap<String, Vec<u8>>>,
    sent: Mutex<Vec<Value>>,
    corrupt: AtomicBool,
    fail_actions: AtomicBool,
}

impl ReviewSocket {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(Weak::new()),
            own_socket: Mutex::new(None),
            resources: Mutex::new(BTreeMap::new()),
            sent: Mutex::new(Vec::new()),
            corrupt: AtomicBool::new(false),
            fail_actions: AtomicBool::new(false),
        })
    }

    fn bind(self: &Arc<Self>, state: &Arc<WorkdeckSessionBrokerState>) {
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(state);
        let shared: SharedDaemonSessionSocket = Arc::clone(self) as SharedDaemonSessionSocket;
        *self
            .own_socket
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::downgrade(&shared));
    }

    fn shared(self: &Arc<Self>) -> SharedDaemonSessionSocket {
        Arc::clone(self) as SharedDaemonSessionSocket
    }

    fn messages(&self) -> Vec<Value> {
        self.sent
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
}

impl DaemonSessionSocket for ReviewSocket {
    fn send(&self, data: &str) -> Result<bool, SessionBrokerStateError> {
        let message = serde_json::from_str::<Value>(data)
            .map_err(|error| SessionBrokerStateError::message(error.to_string()))?;
        self.sent
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(message.clone());
        let command = message["command"].as_str().unwrap_or_default();
        let result = match command {
            "read_review_resource" => {
                let request = &message["input"]["request"];
                let resource_id = request["resourceId"].as_str().unwrap_or_default();
                let generation = request["generation"].as_str().unwrap_or_default();
                let offset = request["offset"].as_u64().unwrap_or_default();
                let length = request["length"].as_u64().unwrap_or_default();
                let resource = self
                    .resources
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .get(resource_id)
                    .cloned();
                match resource {
                    Some(resource) => {
                        let start = usize::try_from(offset).unwrap_or(usize::MAX);
                        let end = start
                            .saturating_add(usize::try_from(length).unwrap_or(usize::MAX))
                            .min(resource.len());
                        let mut bytes = resource.get(start..end).unwrap_or_default().to_vec();
                        if self.corrupt.load(Ordering::Acquire) && !bytes.is_empty() {
                            bytes[0] ^= 1;
                        }
                        serde_json::to_value(WorkdeckReviewResourceReadResultV1::Chunk {
                            ok: true,
                            chunk: ReviewResourceChunk {
                                generation: generation.into(),
                                resource_id: resource_id.into(),
                                offset,
                                byte_length: bytes.len() as u64,
                                encoding: "base64".into(),
                                data: base64::engine::general_purpose::STANDARD.encode(bytes),
                                content_digest: workdeck_core::review_digest(&resource),
                                content_size: resource.len() as u64,
                                eof: end == resource.len(),
                            },
                        })
                        .unwrap()
                    }
                    None => serde_json::to_value(WorkdeckReviewResourceReadResultV1::Failed(
                        crate::WorkdeckReviewFailureV1 {
                            ok: false,
                            code: WorkdeckReviewFailureCodeV1::UnknownResource,
                            message: "unknown resource".into(),
                            current_generation: generation.into(),
                        },
                    ))
                    .unwrap(),
                }
            }
            "apply_review_action" => {
                if self.fail_actions.load(Ordering::Acquire) {
                    return Err(SessionBrokerStateError::message(
                        "producer action transport failed",
                    ));
                }
                let generation = message["input"]["generation"].as_str().unwrap_or_default();
                if message["input"]["action"]["fileKey"] == "file:deadbeef" {
                    serde_json::to_value(WorkdeckReviewActionResultV1::Failed(
                        crate::WorkdeckReviewFailureV1 {
                            ok: false,
                            code: WorkdeckReviewFailureCodeV1::FileNotFound,
                            message: "That file is not part of the review.".into(),
                            current_generation: generation.into(),
                        },
                    ))
                    .unwrap()
                } else {
                    serde_json::to_value(WorkdeckReviewActionResultV1::Applied(
                        WorkdeckReviewActionAppliedV1 {
                            ok: true,
                            generation: generation.into(),
                            state_revision: 1,
                        },
                    ))
                    .unwrap()
                }
            }
            other => {
                return Err(SessionBrokerStateError::message(format!(
                    "unsupported {other}"
                )));
            }
        };
        let request_id = message["requestId"].as_str().unwrap_or_default().to_owned();
        let state = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let socket = self
            .own_socket
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .cloned()
            .unwrap();
        thread::spawn(move || {
            if let (Some(state), Some(socket)) = (state.upgrade(), socket.upgrade()) {
                state.handle_command_result(
                    &socket,
                    &request_id,
                    BrokerCommandOutcome::Success(result),
                );
            }
        });
        Ok(true)
    }
}

struct Fixture {
    state: Arc<WorkdeckSessionBrokerState>,
    socket: Arc<ReviewSocket>,
    server: BrowserReviewServer,
    capability: String,
    resource_id: String,
    patch: Vec<u8>,
}

impl Fixture {
    fn new(patch: &[u8]) -> Self {
        Self::with_options(patch, BrowserReviewServerOptions::default())
    }

    fn with_options(patch: &[u8], options: BrowserReviewServerOptions) -> Self {
        let state = Arc::new(WorkdeckSessionBrokerState::default());
        let socket = ReviewSocket::new();
        socket.bind(&state);
        let capability = "A".repeat(crate::REVIEW_CAPABILITY_TOKEN_LENGTH);
        let resource_id = review_resource_id(&ReviewResourceAddress {
            kind: ReviewResourceKind::Patch,
            file_key: "file:00000001".into(),
            side: None,
        });
        socket
            .resources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(resource_id.clone(), patch.to_vec());
        let server = BrowserReviewServer::new(Arc::clone(&state), options);
        let fixture = Self {
            state,
            socket,
            server,
            capability,
            resource_id,
            patch: patch.to_vec(),
        };
        fixture.register(GENERATION);
        fixture
    }

    fn register(&self, generation: &str) {
        self.register_at(SESSION_ID, Some((generation, 0)));
    }

    fn register_at(&self, session_id: &str, publication: Option<(&str, u64)>) {
        let generation = publication.map_or(GENERATION, |(generation, _)| generation);
        let file = SessionReviewFile {
            summary: SessionFileSummary {
                id: "file-1".into(),
                path: "src/example.rs".into(),
                previous_path: None,
                additions: 1,
                deletions: 1,
                hunk_count: 1,
            },
            patch: None,
            hunks: vec![SessionReviewHunk {
                index: 0,
                header: "@@ -1 +1 @@".into(),
                old_range: Some([1, 1]),
                new_range: Some([1, 1]),
            }],
        };
        let registration = WorkdeckSessionRegistration {
            registration_version: SESSION_BROKER_REGISTRATION_VERSION,
            session_id: session_id.into(),
            pid: 123,
            cwd: "/repo".into(),
            repo_root: Some("/repo".into()),
            launched_at: "2026-08-25T00:00:00.000Z".into(),
            terminal: None,
            info: WorkdeckSessionInfo {
                input_kind: WorkdeckSessionInputKind::Vcs,
                title: "review".into(),
                source_label: "working tree".into(),
                experimental_features: Some(Vec::new()),
                files: vec![file],
                review_catalog: publication.map(|_| WorkdeckReviewResourceCatalogV1 {
                    generation: generation.into(),
                    file_keys_by_runtime_id: BTreeMap::from([(
                        "file-1".into(),
                        "file:00000001".into(),
                    )]),
                    resources: vec![ReviewResourceDescriptor::Patch {
                        descriptor: ReviewResourceDescriptorBase {
                            id: self.resource_id.clone(),
                            generation: generation.into(),
                            file_key: "file:00000001".into(),
                            byte_length: Some(self.patch.len() as u64),
                            digest: Some(workdeck_core::review_digest(&self.patch)),
                        },
                        content_type: REVIEW_PATCH_CONTENT_TYPE.into(),
                    }],
                }),
                review_capability_digest: Some(workdeck_core::review_digest(
                    self.capability.as_bytes(),
                )),
            },
        };
        let snapshot = WorkdeckSessionSnapshot {
            updated_at: "2026-08-25T00:00:00.000Z".into(),
            state: WorkdeckSessionState {
                selected_file_id: Some("file-1".into()),
                selected_file_path: Some("src/example.rs".into()),
                selected_hunk_index: 0,
                selected_hunk_old_range: Some([1, 1]),
                selected_hunk_new_range: Some([1, 1]),
                show_agent_notes: false,
                note_markup_width: None,
                live_comment_count: 0,
                live_comments: Vec::new(),
                review_note_count: Some(0),
                review_notes: Some(Vec::new()),
                review_publication: publication.map(|(generation, state_revision)| {
                    ReviewPublicationAddress {
                        generation: generation.into(),
                        state_revision,
                    }
                }),
            },
        };
        assert_eq!(
            self.state.register_session(
                self.socket.shared(),
                &serde_json::to_value(registration).unwrap(),
                &serde_json::to_value(snapshot).unwrap(),
                RegisterSessionOptions::default(),
            ),
            RegisterSessionResult::Registered
        );
    }

    fn publish_revision(&self, state_revision: u64) {
        let snapshot = WorkdeckSessionSnapshot {
            updated_at: "2026-08-25T00:00:01.000Z".into(),
            state: WorkdeckSessionState {
                selected_file_id: Some("file-1".into()),
                selected_file_path: Some("src/example.rs".into()),
                selected_hunk_index: 0,
                selected_hunk_old_range: Some([1, 1]),
                selected_hunk_new_range: Some([1, 1]),
                show_agent_notes: false,
                note_markup_width: None,
                live_comment_count: 0,
                live_comments: Vec::new(),
                review_note_count: Some(0),
                review_notes: Some(Vec::new()),
                review_publication: Some(ReviewPublicationAddress {
                    generation: GENERATION.into(),
                    state_revision,
                }),
            },
        };
        assert_eq!(
            self.state.update_snapshot(
                &self.socket.shared(),
                SESSION_ID,
                &serde_json::to_value(snapshot).unwrap(),
            ),
            crate::UpdateSnapshotResult::Updated
        );
    }

    fn request(&self, route: WorkdeckReviewHttpRoute) -> crate::SessionBrokerHttpRequest {
        let mut request = crate::SessionBrokerHttpRequest::get(format!(
            "http://127.0.0.1:7000{}",
            crate::review_http_path(&route)
        ));
        request
            .headers
            .insert("host".into(), "127.0.0.1:7000".into());
        request.headers.insert(
            WORKDECK_REVIEW_CAPABILITY_HEADER.into(),
            self.capability.clone(),
        );
        request
    }

    fn publication_route(&self) -> WorkdeckReviewHttpRoute {
        WorkdeckReviewHttpRoute::Publication {
            session_id: SESSION_ID.into(),
        }
    }

    fn resource_route(&self, generation: &str) -> WorkdeckReviewHttpRoute {
        WorkdeckReviewHttpRoute::Resource {
            session_id: SESSION_ID.into(),
            generation: generation.into(),
            resource_id: self.resource_id.clone(),
        }
    }

    fn action_request(&self, body: Value) -> crate::SessionBrokerHttpRequest {
        let mut request = self.request(WorkdeckReviewHttpRoute::Actions {
            session_id: SESSION_ID.into(),
        });
        request.method = "POST".into();
        request
            .headers
            .insert("content-type".into(), "application/json".into());
        request.body = serde_json::to_vec(&body).unwrap();
        request
    }
}

fn response_bytes(mut response: BrokerHttpResponse) -> Vec<u8> {
    response
        .body
        .take()
        .map(|body| body.read_all().unwrap())
        .unwrap_or_default()
}

fn response_json(response: BrokerHttpResponse) -> Value {
    serde_json::from_slice(&response_bytes(response)).unwrap()
}

fn action(generation: &str, action: Value) -> Value {
    json!({
        "protocolVersion": WORKDECK_REVIEW_PROTOCOL_VERSION,
        "generation": generation,
        "actor": {"clientId":"test-client","kind":"browser"},
        "action": action
    })
}

#[derive(Debug)]
struct ParsedSseFrame {
    id: Option<String>,
    event: String,
    data: Value,
}

fn parse_sse_record(record: &str) -> Option<ParsedSseFrame> {
    if record.starts_with(':') {
        return None;
    }
    let mut id = None;
    let mut event = None;
    let mut data = None;
    for line in record.lines() {
        if let Some(value) = line.strip_prefix("id: ") {
            id = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("event: ") {
            event = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("data: ") {
            data = serde_json::from_str(value).ok();
        }
    }
    Some(ParsedSseFrame {
        id,
        event: event?,
        data: data?,
    })
}

fn read_sse_events(response: &mut BrokerHttpResponse, complete: usize) -> Vec<ParsedSseFrame> {
    let Some(BoundedHttpBody::Streaming(body)) = response.body.as_mut() else {
        panic!("expected a streaming response body");
    };
    let mut frames = Vec::new();
    let mut buffer = String::new();
    while frames
        .iter()
        .filter(|frame: &&ParsedSseFrame| frame.id.is_some())
        .count()
        < complete
    {
        let mut bytes = [0_u8; 4096];
        let read = body.read(&mut bytes).unwrap();
        assert_ne!(read, 0, "event stream ended before {complete} events");
        buffer.push_str(std::str::from_utf8(&bytes[..read]).unwrap());
        while let Some(boundary) = buffer.find("\n\n") {
            let record = buffer[..boundary].to_owned();
            buffer.drain(..boundary + 2);
            if let Some(frame) = parse_sse_record(&record) {
                frames.push(frame);
            }
        }
    }
    frames
}

#[test]
fn byte_ranges_accept_only_one_bounded_start_or_start_end_window() {
    assert_eq!(
        parse_byte_range("bytes=0-"),
        Some(ByteRange {
            start: 0,
            end: None
        })
    );
    assert_eq!(
        parse_byte_range(" bytes=12-34 "),
        Some(ByteRange {
            start: 12,
            end: Some(34)
        })
    );
    for invalid in [
        "bytes=-12",
        "bytes=12-3",
        "bytes=1-2,4-5",
        "items=1-2",
        "bytes=1234567890123456-",
        "bytes=a-b",
    ] {
        assert_eq!(parse_byte_range(invalid), None, "accepted {invalid}");
    }
}

#[test]
fn serves_the_publication_to_a_caller_holding_the_capability() {
    let fixture = Fixture::new(b"@@ -1 +1 @@\n-old\n+new\n");
    let response = fixture
        .server
        .handle(&fixture.request(fixture.publication_route()))
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );
    assert_eq!(
        response.headers.get("x-frame-options").map(String::as_str),
        Some("DENY")
    );
    let body = response_json(response);
    assert_eq!(body["protocolVersion"], WORKDECK_REVIEW_PROTOCOL_VERSION);
    assert_eq!(body["sessionId"], SESSION_ID);
    assert_eq!(body["publication"]["generation"], GENERATION);
    assert!(!body["catalog"]["resources"].as_array().unwrap().is_empty());
}

#[test]
fn unexpected_action_transport_failures_are_internal_server_errors() {
    let fixture = Fixture::new(b"patch");
    fixture.socket.fail_actions.store(true, Ordering::Release);
    let response = fixture
        .server
        .handle(&fixture.action_request(action(
            GENERATION,
            json!({"type":"filter/set","filter":"alpha"}),
        )))
        .unwrap();
    assert_eq!(response.status, 500);
    assert!(response_bytes(response).is_empty());
}

#[test]
fn refuses_a_request_with_no_capability() {
    let fixture = Fixture::new(b"patch");
    let mut request = fixture.request(fixture.publication_route());
    request.headers.remove(WORKDECK_REVIEW_CAPABILITY_HEADER);
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 401);
    assert_eq!(response_json(response)["code"], "unauthorized");
}

#[test]
fn wrong_and_malformed_capabilities_receive_the_identical_answer() {
    let fixture = Fixture::new(b"patch");
    let mut wrong = fixture.request(fixture.publication_route());
    wrong.headers.insert(
        WORKDECK_REVIEW_CAPABILITY_HEADER.into(),
        "B".repeat(crate::REVIEW_CAPABILITY_TOKEN_LENGTH),
    );
    let mut malformed = fixture.request(fixture.publication_route());
    malformed
        .headers
        .insert(WORKDECK_REVIEW_CAPABILITY_HEADER.into(), "short".into());
    let wrong = fixture.server.handle(&wrong).unwrap();
    let malformed = fixture.server.handle(&malformed).unwrap();
    assert_eq!(wrong.status, 401);
    assert_eq!(malformed.status, 401);
    assert_eq!(response_json(wrong), response_json(malformed));
}

#[test]
fn authorization_does_not_reveal_whether_a_session_exists() {
    let fixture = Fixture::new(b"patch");
    let request = fixture.request(WorkdeckReviewHttpRoute::Publication {
        session_id: "session-other".into(),
    });
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 401);
    assert_eq!(response_json(response)["code"], "unauthorized");
}

#[test]
fn responses_never_echo_the_clear_capability() {
    let fixture = Fixture::new(b"sensitive patch");
    for request in [
        fixture.request(fixture.publication_route()),
        fixture.request(fixture.resource_route(GENERATION)),
    ] {
        let response = fixture.server.handle(&request).unwrap();
        assert!(
            response
                .headers
                .values()
                .all(|value| !value.contains(&fixture.capability))
        );
        assert!(!String::from_utf8_lossy(&response_bytes(response)).contains(&fixture.capability));
    }
}

#[test]
fn every_review_response_carries_security_headers_without_cors() {
    let fixture = Fixture::new(b"patch");
    let response = fixture
        .server
        .handle(&fixture.request(fixture.publication_route()))
        .unwrap();
    assert_eq!(
        response.headers.get("cache-control").map(String::as_str),
        Some("no-store")
    );
    assert_eq!(
        response.headers.get("referrer-policy").map(String::as_str),
        Some("no-referrer")
    );
    assert_eq!(
        response
            .headers
            .get("x-content-type-options")
            .map(String::as_str),
        Some("nosniff")
    );
    assert_eq!(
        response.headers.get("x-frame-options").map(String::as_str),
        Some("DENY")
    );
    assert!(!response.headers.contains_key("access-control-allow-origin"));
}

#[test]
fn declines_non_review_paths_and_owns_malformed_paths_under_its_prefix() {
    let fixture = Fixture::new(b"patch");
    let mut outside = crate::SessionBrokerHttpRequest::get("http://127.0.0.1:7000/health");
    outside
        .headers
        .insert("host".into(), "127.0.0.1:7000".into());
    assert!(fixture.server.handle(&outside).is_none());
    let mut inside = crate::SessionBrokerHttpRequest::get(
        "http://127.0.0.1:7000/review-api/session-http-1/snapshot",
    );
    inside
        .headers
        .insert("host".into(), "127.0.0.1:7000".into());
    inside.headers.insert(
        WORKDECK_REVIEW_CAPABILITY_HEADER.into(),
        fixture.capability.clone(),
    );
    let response = fixture.server.handle(&inside).unwrap();
    assert_eq!(response.status, 400);
    assert_eq!(response_json(response)["code"], "invalid-request");
}

#[test]
fn refuses_a_request_whose_host_is_not_loopback() {
    let fixture = Fixture::new(b"patch");
    let mut request = fixture.request(fixture.publication_route());
    request
        .headers
        .insert("host".into(), "review.example.com".into());
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 403);
    assert_eq!(response_json(response)["code"], "forbidden-origin");
}

#[test]
fn refuses_a_cross_origin_browser_request() {
    let fixture = Fixture::new(b"patch");
    let mut cross = fixture.request(fixture.publication_route());
    cross
        .headers
        .insert("origin".into(), "http://evil.example".into());
    let response = fixture.server.handle(&cross).unwrap();
    assert_eq!(response.status, 403);
    assert_eq!(response_json(response)["code"], "forbidden-origin");
}

#[test]
fn accepts_its_own_origin() {
    let fixture = Fixture::new(b"patch");
    let mut own = fixture.request(fixture.publication_route());
    own.headers
        .insert("origin".into(), "http://127.0.0.1:7000".into());
    assert_eq!(fixture.server.handle(&own).unwrap().status, 200);
}

#[test]
fn refuses_a_review_route_reached_with_the_wrong_method() {
    let fixture = Fixture::new(b"patch");
    let mut request = fixture.request(fixture.publication_route());
    request.method = "POST".into();
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 405);
    assert_eq!(response_json(response)["code"], "method-not-allowed");
}

#[test]
fn serves_a_whole_verified_resource_through_the_broker_read_path() {
    let fixture = Fixture::new(b"0123456789");
    let response = fixture
        .server
        .handle(&fixture.request(fixture.resource_route(GENERATION)))
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some(REVIEW_PATCH_CONTENT_TYPE)
    );
    assert_eq!(response_bytes(response), fixture.patch);
}

#[test]
fn serves_one_requested_resource_window_as_a_partial_response() {
    let fixture = Fixture::new(b"0123456789abcdef");
    let mut request = fixture.request(fixture.resource_route(GENERATION));
    request.headers.insert("range".into(), "bytes=4-9".into());
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 206);
    assert_eq!(
        response.headers.get("content-range").map(String::as_str),
        Some("bytes 4-9/16")
    );
    assert_eq!(response_bytes(response), b"456789");
}

#[test]
fn refuses_a_malformed_range_before_reading_the_resource() {
    let fixture = Fixture::new(b"0123456789");
    let before = fixture.socket.messages().len();
    let mut request = fixture.request(fixture.resource_route(GENERATION));
    request.headers.insert("range".into(), "items=0-1".into());
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 416);
    assert_eq!(fixture.socket.messages().len(), before);
}

#[test]
fn refuses_a_range_that_starts_past_the_resource_end() {
    let fixture = Fixture::new(b"0123456789");
    let mut request = fixture.request(fixture.resource_route(GENERATION));
    request.headers.insert("range".into(), "bytes=10-".into());
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 416);
    assert_eq!(
        response.headers.get("content-range").map(String::as_str),
        Some("bytes */10")
    );
}

#[test]
fn serves_an_empty_resource_but_refuses_every_range_within_it() {
    let fixture = Fixture::new(b"");
    let whole = fixture
        .server
        .handle(&fixture.request(fixture.resource_route(GENERATION)))
        .unwrap();
    assert_eq!(whole.status, 200);
    assert!(response_bytes(whole).is_empty());
    let mut ranged = fixture.request(fixture.resource_route(GENERATION));
    ranged.headers.insert("range".into(), "bytes=0-".into());
    let ranged = fixture.server.handle(&ranged).unwrap();
    assert_eq!(ranged.status, 416);
    assert_eq!(
        ranged.headers.get("content-range").map(String::as_str),
        Some("bytes */0")
    );
}

#[test]
fn reports_a_resource_the_generation_does_not_offer_as_unknown() {
    let fixture = Fixture::new(b"0123456789");
    let unknown = WorkdeckReviewHttpRoute::Resource {
        session_id: SESSION_ID.into(),
        generation: GENERATION.into(),
        resource_id: "resource:patch:file:deadbeef".into(),
    };
    let response = fixture.server.handle(&fixture.request(unknown)).unwrap();
    assert_eq!(response.status, 404);
    assert_eq!(response_json(response)["code"], "unknown-resource");
}

#[test]
fn reports_corrupted_content_as_an_integrity_failure_not_unknown() {
    let corrupt = Fixture::new(b"0123456789");
    corrupt.socket.corrupt.store(true, Ordering::Release);
    let response = corrupt
        .server
        .handle(&corrupt.request(corrupt.resource_route(GENERATION)))
        .unwrap();
    assert_eq!(response.status, 502);
    assert_eq!(response_json(response)["code"], "resource-integrity");
}

#[test]
fn reports_a_read_against_a_retired_generation_as_stale() {
    let retired = Fixture::new(b"0123456789");
    let old = retired.resource_route(GENERATION);
    retired.register("generation:integration:1");
    let response = retired.server.handle(&retired.request(old)).unwrap();
    assert_eq!(response.status, 409);
    assert_eq!(response_json(response)["code"], "stale-generation");
}

#[test]
fn authorizes_before_admitting_an_action_to_shared_body_control() {
    let admissions = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&admissions);
    let fixture = Fixture::with_options(
        b"patch",
        BrowserReviewServerOptions {
            handle_action_control: Some(Arc::new(move |request, handler| {
                count.fetch_add(1, Ordering::Relaxed);
                handler(&request.body)
            })),
            ..BrowserReviewServerOptions::default()
        },
    );
    let mut request = fixture.action_request(action(
        GENERATION,
        json!({"type":"filter/set","filter":"alpha"}),
    ));
    request.headers.remove(WORKDECK_REVIEW_CAPABILITY_HEADER);
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 401);
    assert_eq!(admissions.load(Ordering::Relaxed), 0);
}

#[test]
fn authorized_action_bytes_pass_once_through_shared_control() {
    let admissions = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&admissions);
    let fixture = Fixture::with_options(
        b"patch",
        BrowserReviewServerOptions {
            handle_action_control: Some(Arc::new(move |request, handler| {
                count.fetch_add(1, Ordering::Relaxed);
                handler(&request.body)
            })),
            ..BrowserReviewServerOptions::default()
        },
    );
    let response = fixture
        .server
        .handle(&fixture.action_request(action(
            GENERATION,
            json!({"type":"filter/set","filter":"bounded"}),
        )))
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(admissions.load(Ordering::Relaxed), 1);
}

#[test]
fn applies_an_action_and_reports_the_new_review_position() {
    let fixture = Fixture::new(b"patch");
    let response = fixture
        .server
        .handle(&fixture.action_request(action(
            GENERATION,
            json!({"type":"filter/set","filter":"alpha"}),
        )))
        .unwrap();
    assert_eq!(response.status, 200);
    let body = response_json(response);
    assert_eq!(body["ok"], true);
    assert_eq!(body["generation"], GENERATION);
}

#[test]
fn refuses_an_action_envelope_the_protocol_cannot_express() {
    let fixture = Fixture::new(b"patch");
    let mut invalid = action(GENERATION, json!({"type":"filter/set","filter":"alpha"}));
    invalid["unexpected"] = json!(true);
    let response = fixture
        .server
        .handle(&fixture.action_request(invalid))
        .unwrap();
    assert_eq!(response.status, 400);
    assert_eq!(response_json(response)["code"], "invalid-request");
}

#[test]
fn distinguishes_an_unknown_action_from_one_it_cannot_parse() {
    let fixture = Fixture::new(b"patch");
    let unsupported = action(
        GENERATION,
        json!({"type":"notes/archive-user","noteId":"user:1"}),
    );
    let response = fixture
        .server
        .handle(&fixture.action_request(unsupported))
        .unwrap();
    assert_eq!(response_json(response)["code"], "unsupported-action");

    let malformed = action(GENERATION, json!({"type":"filter/set"}));
    let response = fixture
        .server
        .handle(&fixture.action_request(malformed))
        .unwrap();
    assert_eq!(response_json(response)["code"], "invalid-request");
}

#[test]
fn reports_an_action_addressed_to_a_generation_the_session_left() {
    let fixture = Fixture::new(b"patch");
    let response = fixture
        .server
        .handle(&fixture.action_request(action(
            "generation:integration:99",
            json!({"type":"filter/set","filter":"alpha"}),
        )))
        .unwrap();
    assert_eq!(response.status, 409);
    assert_eq!(response_json(response)["code"], "stale-generation");
}

#[test]
fn reports_a_semantic_action_rejection_with_the_producer_code() {
    let fixture = Fixture::new(b"patch");
    let response = fixture
        .server
        .handle(&fixture.action_request(action(
            GENERATION,
            json!({"type":"selection/select-file","fileKey":"file:deadbeef"}),
        )))
        .unwrap();
    assert_eq!(response.status, 404);
    assert_eq!(response_json(response)["code"], "file-not-found");
}

#[test]
fn refuses_action_bodies_not_sent_as_json() {
    let fixture = Fixture::new(b"patch");
    let mut request = fixture.action_request(action(
        GENERATION,
        json!({"type":"filter/set","filter":"alpha"}),
    ));
    request
        .headers
        .insert("content-type".into(), "text/plain".into());
    let response = fixture.server.handle(&request).unwrap();
    assert_eq!(response.status, 415);
    assert_eq!(response_json(response)["code"], "unsupported-media-type");
}

#[test]
fn event_stream_opens_with_the_publication_the_review_is_at() {
    let fixture = Fixture::new(b"patch");
    let mut response = fixture
        .server
        .handle(&fixture.request(WorkdeckReviewHttpRoute::Events {
            session_id: SESSION_ID.into(),
        }))
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some(REVIEW_EVENT_STREAM_CONTENT_TYPE)
    );
    let frames = read_sse_events(&mut response, 1);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].event, "publication");
    let address = ReviewPublicationAddress {
        generation: GENERATION.into(),
        state_revision: 0,
    };
    assert_eq!(
        frames[0].id.as_deref(),
        Some(review_event_id(ReviewEventTypeV1::Publication, &address).as_str())
    );
    let event = parse_review_event_frame(&frames[0].data).unwrap();
    assert_eq!(event.payload["sessionId"], SESSION_ID);
    assert_eq!(event.payload["publication"]["generation"], GENERATION);
    assert_eq!(event.payload["publication"]["stateRevision"], 0);
}

#[test]
fn event_stream_sends_a_further_event_for_a_new_publication_position() {
    let fixture = Fixture::new(b"patch");
    let mut response = fixture
        .server
        .handle(&fixture.request(WorkdeckReviewHttpRoute::Events {
            session_id: SESSION_ID.into(),
        }))
        .unwrap();
    fixture.publish_revision(1);
    let frames = read_sse_events(&mut response, 2);
    assert_eq!(
        frames
            .iter()
            .map(|frame| frame.event.as_str())
            .collect::<Vec<_>>(),
        ["publication", "publication"]
    );
    let first = parse_review_event_frame(&frames[0].data).unwrap();
    let second = parse_review_event_frame(&frames[1].data).unwrap();
    assert!(second.state_revision > first.state_revision);
}

#[test]
fn event_stream_chunks_a_large_event_and_reassembles_it_byte_for_byte() {
    let fixture = Fixture::with_options(
        b"patch",
        BrowserReviewServerOptions {
            event_chunk_bytes: 32,
            ..BrowserReviewServerOptions::default()
        },
    );
    let mut response = fixture
        .server
        .handle(&fixture.request(WorkdeckReviewHttpRoute::Events {
            session_id: SESSION_ID.into(),
        }))
        .unwrap();
    let frames = read_sse_events(&mut response, 1);
    assert!(frames.len() > 3);
    assert_eq!(
        parse_review_event_frame_name(&frames[0].event),
        Some(crate::ReviewEventFrameIdentity {
            event_type: ReviewEventTypeV1::Publication,
            phase: Some(crate::ReviewEventFramePhase::Begin),
        })
    );
    assert_eq!(
        parse_review_event_frame_name(&frames.last().unwrap().event),
        Some(crate::ReviewEventFrameIdentity {
            event_type: ReviewEventTypeV1::Publication,
            phase: Some(crate::ReviewEventFramePhase::End),
        })
    );
    assert!(frames[1..frames.len() - 1].iter().all(|frame| {
        parse_review_event_frame_name(&frame.event)
            == Some(crate::ReviewEventFrameIdentity {
                event_type: ReviewEventTypeV1::Publication,
                phase: Some(crate::ReviewEventFramePhase::Chunk),
            })
    }));
    assert_eq!(frames.iter().filter(|frame| frame.id.is_some()).count(), 1);

    let begin = parse_review_event_begin(&frames[0].data).unwrap();
    assert_eq!(begin.chunk_count, u64::try_from(frames.len() - 2).unwrap());
    let mut assembler = ReviewEventAssembler::new(begin, Arc::new(workdeck_core::review_digest));
    for frame in &frames[1..frames.len() - 1] {
        let chunk = parse_review_event_chunk(&frame.data).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&chunk.data)
            .unwrap();
        assert!(matches!(
            assembler.accept(&chunk, &bytes),
            ReviewAssemblyStep::Accepted { .. }
        ));
    }
    let end = parse_review_event_end(&frames.last().unwrap().data).unwrap();
    let ReviewAssemblyResult::Assembled { bytes } = assembler.finish(&end) else {
        panic!("chunked publication failed integrity verification");
    };
    let body = serde_json::from_slice::<Value>(&bytes).unwrap();
    assert_eq!(body["sessionId"], SESSION_ID);
    assert_eq!(body["publication"]["generation"], GENERATION);
    assert_eq!(body["publication"]["stateRevision"], 0);
}

#[test]
fn event_stream_ends_with_disconnect_when_the_session_goes_away() {
    let fixture = Fixture::new(b"patch");
    let mut response = fixture
        .server
        .handle(&fixture.request(WorkdeckReviewHttpRoute::Events {
            session_id: SESSION_ID.into(),
        }))
        .unwrap();
    fixture.state.unregister_socket(&fixture.socket.shared());
    let frames = read_sse_events(&mut response, 2);
    assert_eq!(
        frames
            .iter()
            .map(|frame| frame.event.as_str())
            .collect::<Vec<_>>(),
        ["publication", "disconnect"]
    );
}

#[test]
fn event_stream_refuses_more_streams_than_the_per_review_limit() {
    let fixture = Fixture::with_options(
        b"patch",
        BrowserReviewServerOptions {
            max_streams_per_session: 1,
            ..BrowserReviewServerOptions::default()
        },
    );
    let route = WorkdeckReviewHttpRoute::Events {
        session_id: SESSION_ID.into(),
    };
    let first = fixture
        .server
        .handle(&fixture.request(route.clone()))
        .unwrap();
    let second = fixture.server.handle(&fixture.request(route)).unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(second.status, 503);
    assert_eq!(response_json(second)["code"], "too-many-streams");
    drop(first);
}

#[test]
fn event_stream_refuses_a_registered_session_that_publishes_no_review() {
    let fixture = Fixture::new(b"patch");
    let quiet_session = "session-http-quiet";
    fixture.register_at(quiet_session, None);
    let events = fixture
        .server
        .handle(&fixture.request(WorkdeckReviewHttpRoute::Events {
            session_id: quiet_session.into(),
        }))
        .unwrap();
    assert_eq!(events.status, 409);
    assert_eq!(response_json(events)["code"], "no-publication");
    let publication = fixture
        .server
        .handle(&fixture.request(WorkdeckReviewHttpRoute::Publication {
            session_id: quiet_session.into(),
        }))
        .unwrap();
    assert_eq!(publication.status, 409);
    assert_eq!(response_json(publication)["code"], "no-publication");
}

#[test]
fn event_stream_closes_at_exactly_one_full_budget_behind() {
    let fixture = Fixture::with_options(
        b"patch",
        BrowserReviewServerOptions {
            max_stream_buffer_bytes: 4,
            ..BrowserReviewServerOptions::default()
        },
    );
    let stream = Arc::new(ReviewEventStream {
        id: 999,
        session_id: SESSION_ID.into(),
        capability_digest: "digest".into(),
        address: Mutex::new(ReviewPublicationAddress {
            generation: GENERATION.into(),
            state_revision: 0,
        }),
        queue: Arc::new(StreamQueue::default()),
        closed: AtomicBool::new(false),
    });
    fixture
        .server
        .inner
        .streams
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(stream.id, Arc::clone(&stream));

    assert!(fixture.server.inner.write(&stream, vec![0; 7]));
    assert!(!stream.closed.load(Ordering::Acquire));
    assert!(!fixture.server.inner.write(&stream, vec![0]));
    assert!(stream.closed.load(Ordering::Acquire));
    let queued = stream
        .queue
        .state
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    assert!(queued.closed);
    assert_eq!(
        queued.error.as_deref(),
        Some("The review event stream fell too far behind.")
    );
}
