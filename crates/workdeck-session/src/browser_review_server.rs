//! Capability-authorized HTTP access to one daemon's live review publications.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Value, json};
use url::Url;
use workdeck_review::{REVIEW_RESOURCE_CHUNK_BYTES, ReviewPublicationAddress};

use crate::{
    BoundedHttpBody, BrokerBody, BrokerHttpResponse, MAX_HTTP_BODY_BYTES,
    MAX_WORKDECK_REVIEW_ENVELOPE_BYTES, NativeSessionBrokerHttpHandler, REVIEW_EVENT_CHUNK_BYTES,
    REVIEW_EVENT_HEARTBEAT_FRAME, REVIEW_EVENT_STREAM_CONTENT_TYPE, ReviewEventTypeV1,
    ReviewGenerationRetiredError, ReviewPublicationEvent, ReviewPublicationSubscription,
    ReviewResourceReadError, WORKDECK_REVIEW_CAPABILITY_HEADER, WORKDECK_REVIEW_HTTP_PATH_PREFIX,
    WORKDECK_REVIEW_PROTOCOL_VERSION, WorkdeckReviewActionResultV1,
    WorkdeckReviewClientErrorCodeV1, WorkdeckReviewFailureCodeV1, WorkdeckReviewHttpFailureV1,
    WorkdeckReviewHttpRoute, WorkdeckReviewParseFailureReason, WorkdeckReviewParseResult,
    WorkdeckReviewPublicationBodyV1, WorkdeckSessionBrokerError, WorkdeckSessionBrokerState,
    encode_review_event_frame, is_review_capability_token, parse_review_http_path,
    parse_workdeck_review_action_envelope, plan_review_event_frames, review_error_message,
};

const DEFAULT_HEARTBEAT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_STREAMS: usize = 64;
const DEFAULT_MAX_STREAMS_PER_SESSION: usize = 8;
const DEFAULT_MAX_STREAM_BUFFER_BYTES: usize = 2 * MAX_WORKDECK_REVIEW_ENVELOPE_BYTES as usize;

pub type BrowserReviewActionHandler =
    Arc<dyn Fn(&[u8]) -> crate::SessionBrokerHttpResponse + Send + Sync>;
pub type BrowserReviewActionControl = Arc<
    dyn Fn(
            &crate::SessionBrokerHttpRequest,
            BrowserReviewActionHandler,
        ) -> crate::SessionBrokerHttpResponse
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct BrowserReviewServerOptions {
    pub heartbeat: Duration,
    pub max_streams: usize,
    pub max_streams_per_session: usize,
    pub max_stream_buffer_bytes: usize,
    pub event_chunk_bytes: u64,
    pub allow_remote: bool,
    pub handle_action_control: Option<BrowserReviewActionControl>,
}

impl Default for BrowserReviewServerOptions {
    fn default() -> Self {
        Self {
            heartbeat: DEFAULT_HEARTBEAT,
            max_streams: DEFAULT_MAX_STREAMS,
            max_streams_per_session: DEFAULT_MAX_STREAMS_PER_SESSION,
            max_stream_buffer_bytes: DEFAULT_MAX_STREAM_BUFFER_BYTES,
            event_chunk_bytes: REVIEW_EVENT_CHUNK_BYTES,
            allow_remote: false,
            handle_action_control: None,
        }
    }
}

#[derive(Clone)]
pub struct BrowserReviewServer {
    inner: Arc<BrowserReviewServerInner>,
}

struct BrowserReviewServerInner {
    state: Arc<WorkdeckSessionBrokerState>,
    options: BrowserReviewServerOptions,
    streams: Mutex<BTreeMap<u64, Arc<ReviewEventStream>>>,
    next_stream_id: AtomicU64,
    subscription: Mutex<Option<ReviewPublicationSubscription>>,
    heartbeat: Arc<(Mutex<bool>, Condvar)>,
    closed: AtomicBool,
}

struct ReviewEventStream {
    id: u64,
    session_id: String,
    capability_digest: String,
    address: Mutex<ReviewPublicationAddress>,
    queue: Arc<StreamQueue>,
    closed: AtomicBool,
}

#[derive(Default)]
struct StreamQueueState {
    chunks: VecDeque<Vec<u8>>,
    queued_bytes: usize,
    closed: bool,
    error: Option<String>,
}

#[derive(Default)]
struct StreamQueue {
    state: Mutex<StreamQueueState>,
    changed: Condvar,
}

struct ReviewEventBody {
    queue: Arc<StreamQueue>,
    stream_id: u64,
    server: Weak<BrowserReviewServerInner>,
    current: Vec<u8>,
    offset: usize,
}

impl Read for ReviewEventBody {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            if self.offset < self.current.len() {
                let available = &self.current[self.offset..];
                let length = available.len().min(output.len());
                output[..length].copy_from_slice(&available[..length]);
                self.offset += length;
                if self.offset == self.current.len() {
                    self.current.clear();
                    self.offset = 0;
                }
                return Ok(length);
            }
            let mut state = self
                .queue
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            while state.chunks.is_empty() && !state.closed {
                state = self
                    .queue
                    .changed
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            if let Some(error) = state.error.take() {
                return Err(io::Error::other(error));
            }
            if let Some(chunk) = state.chunks.pop_front() {
                state.queued_bytes = state.queued_bytes.saturating_sub(chunk.len());
                self.current = chunk;
                continue;
            }
            return Ok(0);
        }
    }
}

impl BrokerBody for ReviewEventBody {
    fn cancel(&mut self) -> io::Result<()> {
        self.remove();
        Ok(())
    }
}

impl Drop for ReviewEventBody {
    fn drop(&mut self) {
        self.remove();
    }
}

impl ReviewEventBody {
    fn remove(&self) {
        if let Some(server) = self.server.upgrade() {
            server.remove_stream(self.stream_id, None);
        }
    }
}

impl BrowserReviewServer {
    #[must_use]
    pub fn new(
        state: Arc<WorkdeckSessionBrokerState>,
        options: BrowserReviewServerOptions,
    ) -> Self {
        let inner = Arc::new(BrowserReviewServerInner {
            state,
            options,
            streams: Mutex::new(BTreeMap::new()),
            next_stream_id: AtomicU64::new(1),
            subscription: Mutex::new(None),
            heartbeat: Arc::new((Mutex::new(false), Condvar::new())),
            closed: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&inner);
        let subscription = inner.state.subscribe_review_publications(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.observe(event);
            }
        });
        *inner
            .subscription
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(subscription);
        start_heartbeat(&inner);
        Self { inner }
    }

    #[must_use]
    pub fn handler(&self) -> NativeSessionBrokerHttpHandler {
        let server = self.clone();
        Arc::new(move |request, _address| {
            let server = server.clone();
            Box::pin(async move { server.handle(&request) })
        })
    }

    #[must_use]
    pub fn handle(&self, request: &crate::SessionBrokerHttpRequest) -> Option<BrokerHttpResponse> {
        let path = Url::parse(&request.url).ok()?.path().to_owned();
        if !path.starts_with(&format!("{WORKDECK_REVIEW_HTTP_PATH_PREFIX}/")) {
            return None;
        }
        if !self.inner.check_origin(request) {
            return Some(self.inner.failure(
                WorkdeckReviewClientErrorCodeV1::ForbiddenOrigin,
                None,
                None,
            ));
        }
        let Some(route) = parse_review_http_path(&path) else {
            return Some(self.inner.failure(
                WorkdeckReviewClientErrorCodeV1::InvalidRequest,
                None,
                None,
            ));
        };
        let session_id = route_session_id(&route);
        let Some(authorization) = self.inner.authorize(request, session_id) else {
            return Some(self.inner.failure(
                WorkdeckReviewClientErrorCodeV1::Unauthorized,
                None,
                None,
            ));
        };
        let response = match &route {
            WorkdeckReviewHttpRoute::Publication { session_id } => {
                if request.method != "GET" {
                    self.inner.failure(
                        WorkdeckReviewClientErrorCodeV1::MethodNotAllowed,
                        None,
                        None,
                    )
                } else {
                    self.inner.handle_publication(session_id)
                }
            }
            WorkdeckReviewHttpRoute::Events { session_id } => {
                if request.method != "GET" {
                    self.inner.failure(
                        WorkdeckReviewClientErrorCodeV1::MethodNotAllowed,
                        None,
                        None,
                    )
                } else {
                    self.inner.handle_events(session_id, authorization)
                }
            }
            WorkdeckReviewHttpRoute::Resource { .. } => {
                if request.method != "GET" {
                    self.inner.failure(
                        WorkdeckReviewClientErrorCodeV1::MethodNotAllowed,
                        None,
                        None,
                    )
                } else {
                    self.inner.handle_resource(request, &route)
                }
            }
            WorkdeckReviewHttpRoute::Actions { session_id } => {
                if request.method != "POST" {
                    self.inner.failure(
                        WorkdeckReviewClientErrorCodeV1::MethodNotAllowed,
                        None,
                        None,
                    )
                } else {
                    self.inner.handle_action_request(request, session_id)
                }
            }
        };
        Some(response)
    }

    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.inner
            .streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    pub fn broadcast_heartbeat(&self) {
        self.inner.broadcast_heartbeat();
    }

    pub fn close(&self) {
        self.inner.close();
    }
}

impl Drop for BrowserReviewServer {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            self.inner.close();
        }
    }
}

struct ReviewAuthorization {
    capability_digest: String,
}

impl BrowserReviewServerInner {
    fn check_origin(&self, request: &crate::SessionBrokerHttpRequest) -> bool {
        let Some(host) = request.header("host") else {
            return false;
        };
        let host_name = crate::parse_host_and_port(host).map(|value| value.host);
        if !self.options.allow_remote
            && host_name
                .as_deref()
                .is_none_or(|host| !crate::is_loopback_host(host))
        {
            return false;
        }
        let Some(origin) = request.header("origin") else {
            return true;
        };
        let Ok(url) = Url::parse(&request.url) else {
            return false;
        };
        origin == format!("{}://{host}", url.scheme())
    }

    fn authorize(
        &self,
        request: &crate::SessionBrokerHttpRequest,
        session_id: &str,
    ) -> Option<ReviewAuthorization> {
        let presented = request.header(WORKDECK_REVIEW_CAPABILITY_HEADER)?;
        if !is_review_capability_token(presented) {
            return None;
        }
        let digest = workdeck_core::review_digest(presented.as_bytes());
        let expected = self.state.get_review_capability_digest(session_id);
        constant_time_digest_match(&digest, expected.as_deref()).then(|| ReviewAuthorization {
            capability_digest: expected.expect("successful comparison has an expected digest"),
        })
    }

    fn publication_body(&self, session_id: &str) -> Option<WorkdeckReviewPublicationBodyV1> {
        let publication = self.state.get_review_publication(session_id)?;
        Some(WorkdeckReviewPublicationBodyV1 {
            protocol_version: WORKDECK_REVIEW_PROTOCOL_VERSION,
            session_id: session_id.into(),
            publication: publication.address,
            catalog: publication.catalog,
        })
    }

    fn handle_publication(&self, session_id: &str) -> BrokerHttpResponse {
        self.publication_body(session_id).map_or_else(
            || self.failure(WorkdeckReviewClientErrorCodeV1::NoPublication, None, None),
            |body| self.json(&body, 200),
        )
    }

    fn handle_resource(
        &self,
        request: &crate::SessionBrokerHttpRequest,
        route: &WorkdeckReviewHttpRoute,
    ) -> BrokerHttpResponse {
        let WorkdeckReviewHttpRoute::Resource {
            session_id,
            generation,
            resource_id,
        } = route
        else {
            return self.failure(WorkdeckReviewClientErrorCodeV1::InvalidRequest, None, None);
        };
        let descriptor = self
            .state
            .get_review_publication(session_id)
            .and_then(|publication| {
                publication
                    .catalog
                    .resources
                    .into_iter()
                    .find(|resource| resource.base().id == *resource_id)
            });
        let range = match request.header("range") {
            Some(value) => match parse_byte_range(value) {
                Some(range) => Some(range),
                None => return self.range_not_satisfiable(None),
            },
            None => None,
        };
        let bytes = match self
            .state
            .load_review_resource(session_id, generation, resource_id)
        {
            Ok(bytes) => bytes,
            Err(error) => return self.resource_failure(error),
        };
        if range.as_ref().is_some_and(|range| {
            range.start >= bytes.len() || range.end.is_some_and(|end| range.start > end)
        }) {
            return self.range_not_satisfiable(Some(bytes.len()));
        }
        let start = range.as_ref().map_or(0, |range| range.start);
        let requested_end = range
            .as_ref()
            .and_then(|range| range.end)
            .unwrap_or_else(|| bytes.len().saturating_sub(1));
        let end = requested_end
            .min(bytes.len().saturating_sub(1))
            .min(start.saturating_add(REVIEW_RESOURCE_CHUNK_BYTES as usize - 1));
        let partial = range.is_some() || (!bytes.is_empty() && end != bytes.len() - 1);
        let body = if bytes.is_empty() {
            Vec::new()
        } else {
            bytes[start..=end].to_vec()
        };
        let content_type = descriptor
            .as_ref()
            .map(resource_content_type)
            .unwrap_or("application/octet-stream");
        let mut headers = security_headers();
        headers.insert("accept-ranges".into(), "bytes".into());
        headers.insert("content-type".into(), content_type.into());
        if partial {
            headers.insert(
                "content-range".into(),
                format!("bytes {start}-{end}/{}", bytes.len()),
            );
        }
        retained_response(if partial { 206 } else { 200 }, headers, body)
    }

    fn handle_action_request(
        self: &Arc<Self>,
        request: &crate::SessionBrokerHttpRequest,
        session_id: &str,
    ) -> BrokerHttpResponse {
        if request
            .header("content-type")
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .is_none_or(|value| !value.eq_ignore_ascii_case("application/json"))
        {
            return self.failure(
                WorkdeckReviewClientErrorCodeV1::UnsupportedMediaType,
                None,
                None,
            );
        }
        if let Some(control) = &self.options.handle_action_control {
            let inner = Arc::clone(self);
            let session_id = session_id.to_owned();
            return native_response(control(
                request,
                Arc::new(move |body| inner.handle_action(&session_id, body)),
            ));
        }
        native_response(self.handle_action(session_id, &request.body))
    }

    fn handle_action(&self, session_id: &str, body: &[u8]) -> crate::SessionBrokerHttpResponse {
        let max = MAX_WORKDECK_REVIEW_ENVELOPE_BYTES.min(MAX_HTTP_BODY_BYTES) as usize;
        if body.len() > max {
            return self.failure_http(WorkdeckReviewClientErrorCodeV1::PayloadTooLarge, None, None);
        }
        let value = std::str::from_utf8(body)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        let Some(value) = value else {
            return self.failure_http(WorkdeckReviewClientErrorCodeV1::InvalidRequest, None, None);
        };
        let envelope = match parse_workdeck_review_action_envelope(&value) {
            WorkdeckReviewParseResult::Parsed(envelope) => envelope,
            WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Unsupported) => {
                return self.failure_http(
                    WorkdeckReviewClientErrorCodeV1::UnsupportedAction,
                    None,
                    None,
                );
            }
            WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid) => {
                return self.failure_http(
                    WorkdeckReviewClientErrorCodeV1::InvalidRequest,
                    None,
                    None,
                );
            }
        };
        match self.state.apply_review_action(
            session_id,
            &envelope.generation,
            envelope.action,
            Some(envelope.actor),
            envelope.expected_state_revision,
        ) {
            Ok(WorkdeckReviewActionResultV1::Applied(applied)) => review_json_http(
                200,
                &serde_json::to_value(applied).expect("review action result serializes"),
            ),
            Ok(WorkdeckReviewActionResultV1::Failed(failure)) => self.failure_http(
                review_failure_code(failure.code),
                Some(failure.message),
                Some(failure.current_generation),
            ),
            Err(WorkdeckSessionBrokerError::GenerationRetired(error)) => self.failure_http(
                WorkdeckReviewClientErrorCodeV1::StaleGeneration,
                None,
                error.current_generation,
            ),
            // Hunk lets unexpected producer/broker failures escape the review route, so the
            // runtime reports an internal server error rather than misclassifying them as a
            // resource read failure. Preserve that boundary in the native handler.
            Err(_) => crate::SessionBrokerHttpResponse::empty(500),
        }
    }

    fn handle_events(
        self: &Arc<Self>,
        session_id: &str,
        authorization: ReviewAuthorization,
    ) -> BrokerHttpResponse {
        let Some(opening) = self.publication_body(session_id) else {
            return self.failure(WorkdeckReviewClientErrorCodeV1::NoPublication, None, None);
        };
        let mut streams = self
            .streams
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let per_session = streams
            .values()
            .filter(|stream| stream.session_id == session_id)
            .count();
        if streams.len() >= self.options.max_streams
            || per_session >= self.options.max_streams_per_session
        {
            return self.failure(WorkdeckReviewClientErrorCodeV1::TooManyStreams, None, None);
        }
        let id = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        let stream = Arc::new(ReviewEventStream {
            id,
            session_id: session_id.into(),
            capability_digest: authorization.capability_digest,
            address: Mutex::new(opening.publication.clone()),
            queue: Arc::new(StreamQueue::default()),
            closed: AtomicBool::new(false),
        });
        streams.insert(id, Arc::clone(&stream));
        drop(streams);
        self.send_event(
            &stream,
            ReviewEventTypeV1::Publication,
            &opening.publication,
            serde_json::to_value(&opening).expect("review publication serializes"),
        );
        let body = ReviewEventBody {
            queue: Arc::clone(&stream.queue),
            stream_id: id,
            server: Arc::downgrade(self),
            current: Vec::new(),
            offset: 0,
        };
        let mut headers = security_headers();
        headers.insert(
            "content-type".into(),
            REVIEW_EVENT_STREAM_CONTENT_TYPE.into(),
        );
        headers.insert("x-accel-buffering".into(), "no".into());
        BrokerHttpResponse {
            status: 200,
            status_text: String::new(),
            headers,
            body: Some(BoundedHttpBody::Streaming(Box::new(body))),
        }
    }

    fn observe(self: &Arc<Self>, event: ReviewPublicationEvent) {
        let streams = self
            .streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .filter(|stream| stream.session_id == event.session_id())
            .cloned()
            .collect::<Vec<_>>();
        for stream in streams {
            match &event {
                ReviewPublicationEvent::Retired { .. } => self.send_disconnect(&stream),
                ReviewPublicationEvent::Published { .. } => {
                    if self
                        .state
                        .get_review_capability_digest(&stream.session_id)
                        .as_deref()
                        != Some(stream.capability_digest.as_str())
                    {
                        self.remove_stream(stream.id, None);
                    } else if let Some(body) = self.publication_body(&stream.session_id) {
                        self.send_event(
                            &stream,
                            ReviewEventTypeV1::Publication,
                            &body.publication,
                            serde_json::to_value(&body).expect("review publication serializes"),
                        );
                    } else {
                        self.send_disconnect(&stream);
                    }
                }
            }
        }
    }

    fn send_disconnect(self: &Arc<Self>, stream: &Arc<ReviewEventStream>) {
        let address = stream
            .address
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        self.send_event(
            stream,
            ReviewEventTypeV1::Disconnect,
            &address,
            json!({"sessionId": stream.session_id}),
        );
        self.remove_stream(stream.id, None);
    }

    fn send_event(
        &self,
        stream: &Arc<ReviewEventStream>,
        event_type: ReviewEventTypeV1,
        address: &ReviewPublicationAddress,
        body: Value,
    ) {
        if stream.closed.load(Ordering::Acquire) {
            return;
        }
        *stream
            .address
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = address.clone();
        let Ok(payload) = serde_json::to_vec(&body) else {
            self.remove_stream(stream.id, Some("Could not encode review event.".into()));
            return;
        };
        let digest = workdeck_core::review_digest(&payload);
        let frames = plan_review_event_frames(crate::PlanReviewEventInput {
            event_type,
            address,
            body,
            payload: &payload,
            content_digest: &digest,
            encode_chunk: encode_event_chunk,
            chunk_bytes: Some(self.options.event_chunk_bytes.min(REVIEW_EVENT_CHUNK_BYTES)),
        });
        let Ok(frames) = frames else {
            self.remove_stream(
                stream.id,
                Some("Review event exceeded its byte limit.".into()),
            );
            return;
        };
        for frame in frames {
            let Ok(encoded) = encode_review_event_frame(&frame) else {
                self.remove_stream(
                    stream.id,
                    Some("Could not encode review event frame.".into()),
                );
                return;
            };
            if !self.write(stream, encoded.into_bytes()) {
                return;
            }
        }
    }

    fn broadcast_heartbeat(&self) {
        let streams = self
            .streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for stream in streams {
            self.write(&stream, REVIEW_EVENT_HEARTBEAT_FRAME.as_bytes().to_vec());
        }
    }

    fn write(&self, stream: &Arc<ReviewEventStream>, bytes: Vec<u8>) -> bool {
        if stream.closed.load(Ordering::Acquire) {
            return false;
        }
        let mut state = stream
            .queue
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.queued_bytes = state.queued_bytes.saturating_add(bytes.len());
        state.chunks.push_back(bytes);
        if state.queued_bytes >= self.options.max_stream_buffer_bytes.saturating_mul(2) {
            state.error = Some("The review event stream fell too far behind.".into());
            state.closed = true;
            stream.closed.store(true, Ordering::Release);
            drop(state);
            stream.queue.changed.notify_all();
            self.streams
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&stream.id);
            return false;
        }
        drop(state);
        stream.queue.changed.notify_all();
        true
    }

    fn remove_stream(&self, id: u64, error: Option<String>) {
        let stream = self
            .streams
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&id);
        let Some(stream) = stream else {
            return;
        };
        if stream.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut state = stream
            .queue
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.error = error;
        state.closed = true;
        drop(state);
        stream.queue.changed.notify_all();
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        *self
            .subscription
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        {
            let mut stop = self
                .heartbeat
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            *stop = true;
            self.heartbeat.1.notify_all();
        }
        let ids = self
            .streams
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for id in ids {
            self.remove_stream(id, None);
        }
    }

    fn json(&self, value: &impl serde::Serialize, status: u16) -> BrokerHttpResponse {
        let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
        let mut headers = security_headers();
        headers.insert(
            "content-type".into(),
            "application/json; charset=utf-8".into(),
        );
        retained_response(status, headers, body)
    }

    fn failure(
        &self,
        code: WorkdeckReviewClientErrorCodeV1,
        message: Option<String>,
        current_generation: Option<String>,
    ) -> BrokerHttpResponse {
        native_response(self.failure_http(code, message, current_generation))
    }

    fn failure_http(
        &self,
        code: WorkdeckReviewClientErrorCodeV1,
        message: Option<String>,
        current_generation: Option<String>,
    ) -> crate::SessionBrokerHttpResponse {
        review_failure_http(code, message, current_generation)
    }

    fn resource_failure(&self, error: WorkdeckSessionBrokerError) -> BrokerHttpResponse {
        match error {
            WorkdeckSessionBrokerError::ResourceRead(ReviewResourceReadError { code, message }) => {
                self.failure(review_failure_code(code), Some(message), None)
            }
            WorkdeckSessionBrokerError::GenerationRetired(ReviewGenerationRetiredError {
                current_generation,
            }) => self.failure(
                WorkdeckReviewClientErrorCodeV1::StaleGeneration,
                None,
                current_generation,
            ),
            _ => self.failure(
                WorkdeckReviewClientErrorCodeV1::ResourceUnavailable,
                None,
                None,
            ),
        }
    }

    fn range_not_satisfiable(&self, size: Option<usize>) -> BrokerHttpResponse {
        let mut headers = security_headers();
        if let Some(size) = size {
            headers.insert("content-range".into(), format!("bytes */{size}"));
        }
        retained_response(416, headers, Vec::new())
    }
}

fn start_heartbeat(inner: &Arc<BrowserReviewServerInner>) {
    let weak = Arc::downgrade(inner);
    let control = Arc::clone(&inner.heartbeat);
    let interval = inner.options.heartbeat;
    let _ = thread::Builder::new()
        .name("workdeck-review-heartbeat".into())
        .spawn(move || {
            let mut stopped = control.0.lock().unwrap_or_else(|error| error.into_inner());
            loop {
                let waited = control
                    .1
                    .wait_timeout_while(stopped, interval, |stopped| !*stopped)
                    .unwrap_or_else(|error| error.into_inner());
                stopped = waited.0;
                if *stopped {
                    return;
                }
                drop(stopped);
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.broadcast_heartbeat();
                stopped = control.0.lock().unwrap_or_else(|error| error.into_inner());
            }
        });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: Option<usize>,
}

#[must_use]
pub fn parse_byte_range(header: &str) -> Option<ByteRange> {
    let range = header.trim().strip_prefix("bytes=")?;
    let (start, end) = range.split_once('-')?;
    if start.is_empty()
        || start.len() > 15
        || !start.bytes().all(|byte| byte.is_ascii_digit())
        || end.len() > 15
        || !end.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let start = start.parse::<usize>().ok()?;
    let end = (!end.is_empty())
        .then(|| end.parse::<usize>().ok())
        .flatten();
    if end.is_some_and(|end| end < start) {
        return None;
    }
    Some(ByteRange { start, end })
}

fn route_session_id(route: &WorkdeckReviewHttpRoute) -> &str {
    match route {
        WorkdeckReviewHttpRoute::Publication { session_id }
        | WorkdeckReviewHttpRoute::Events { session_id }
        | WorkdeckReviewHttpRoute::Actions { session_id }
        | WorkdeckReviewHttpRoute::Resource { session_id, .. } => session_id,
    }
}

fn constant_time_digest_match(presented: &str, expected: Option<&str>) -> bool {
    let Some(presented) = decode_digest(presented) else {
        return false;
    };
    let mut random = [0_u8; 32];
    let _ = getrandom::fill(&mut random);
    let expected_bytes = expected.and_then(decode_digest).unwrap_or(random);
    let difference = presented
        .iter()
        .zip(expected_bytes)
        .fold(0_u8, |difference, (left, right)| {
            difference | (*left ^ right)
        });
    difference == 0 && expected.is_some()
}

fn decode_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(bytes)
}

fn resource_content_type(resource: &workdeck_review::ReviewResourceDescriptor) -> &str {
    match resource {
        workdeck_review::ReviewResourceDescriptor::CanonicalFile { content_type, .. }
        | workdeck_review::ReviewResourceDescriptor::Patch { content_type, .. }
        | workdeck_review::ReviewResourceDescriptor::Source { content_type, .. } => content_type,
    }
}

fn encode_event_chunk(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn security_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("cache-control".into(), "no-store".into()),
        (
            "content-security-policy".into(),
            "default-src 'none'; frame-ancestors 'none'; base-uri 'none'".into(),
        ),
        ("referrer-policy".into(), "no-referrer".into()),
        ("x-content-type-options".into(), "nosniff".into()),
        ("x-frame-options".into(), "DENY".into()),
    ])
}

fn retained_response(
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
) -> BrokerHttpResponse {
    BrokerHttpResponse {
        status,
        status_text: String::new(),
        headers,
        body: (!body.is_empty()).then(|| {
            BoundedHttpBody::Streaming(Box::new(std::io::Cursor::new(body)) as Box<dyn BrokerBody>)
        }),
    }
}

fn native_response(response: crate::SessionBrokerHttpResponse) -> BrokerHttpResponse {
    retained_response(response.status, response.headers, response.body)
}

pub(crate) fn review_failure_http(
    code: WorkdeckReviewClientErrorCodeV1,
    message: Option<String>,
    current_generation: Option<String>,
) -> crate::SessionBrokerHttpResponse {
    let body = WorkdeckReviewHttpFailureV1 {
        ok: false,
        code,
        message: message.unwrap_or_else(|| review_error_message(code)),
        current_generation,
    };
    review_json_http(
        review_error_status(code),
        &serde_json::to_value(body).expect("review failure serializes"),
    )
}

fn review_json_http(status: u16, body: &Value) -> crate::SessionBrokerHttpResponse {
    let mut response = crate::SessionBrokerHttpResponse::json(status, body);
    response.headers = security_headers();
    response.headers.insert(
        "content-type".into(),
        "application/json; charset=utf-8".into(),
    );
    response
}

const fn review_error_status(code: WorkdeckReviewClientErrorCodeV1) -> u16 {
    use WorkdeckReviewClientErrorCodeV1 as Code;
    match code {
        Code::StaleGeneration
        | Code::DraftMissing
        | Code::DraftActive
        | Code::DraftModeMismatch
        | Code::NoteHasReplies
        | Code::NoteIdConflict
        | Code::InvalidNoteParent
        | Code::NoPublication => 409,
        Code::InvalidRequest | Code::UnsupportedAction | Code::BlankNote | Code::MissingFact => 400,
        Code::FileNotFound
        | Code::HunkNotFound
        | Code::GapNotFound
        | Code::NoteNotFound
        | Code::UnknownResource => 404,
        Code::NoteNotEditable | Code::ForbiddenOrigin => 403,
        Code::NoteTooLarge | Code::ResourceTooLarge | Code::PayloadTooLarge => 413,
        Code::ResourceUnavailable | Code::ResourceIntegrity => 502,
        Code::InvalidRange => 416,
        Code::Unauthorized => 401,
        Code::MethodNotAllowed => 405,
        Code::UnsupportedMediaType => 415,
        Code::TooManyStreams => 503,
    }
}

const fn review_failure_code(code: WorkdeckReviewFailureCodeV1) -> WorkdeckReviewClientErrorCodeV1 {
    use WorkdeckReviewClientErrorCodeV1 as Client;
    use WorkdeckReviewFailureCodeV1 as Failure;
    match code {
        Failure::StaleGeneration => Client::StaleGeneration,
        Failure::InvalidRequest => Client::InvalidRequest,
        Failure::FileNotFound => Client::FileNotFound,
        Failure::HunkNotFound => Client::HunkNotFound,
        Failure::GapNotFound => Client::GapNotFound,
        Failure::DraftMissing => Client::DraftMissing,
        Failure::DraftActive => Client::DraftActive,
        Failure::DraftModeMismatch => Client::DraftModeMismatch,
        Failure::NoteNotFound => Client::NoteNotFound,
        Failure::NoteNotEditable => Client::NoteNotEditable,
        Failure::NoteHasReplies => Client::NoteHasReplies,
        Failure::NoteIdConflict => Client::NoteIdConflict,
        Failure::InvalidNoteParent => Client::InvalidNoteParent,
        Failure::BlankNote => Client::BlankNote,
        Failure::NoteTooLarge => Client::NoteTooLarge,
        Failure::MissingFact => Client::MissingFact,
        Failure::UnknownResource => Client::UnknownResource,
        Failure::ResourceUnavailable => Client::ResourceUnavailable,
        Failure::ResourceTooLarge => Client::ResourceTooLarge,
        Failure::ResourceIntegrity => Client::ResourceIntegrity,
        Failure::InvalidRange => Client::InvalidRange,
    }
}

#[cfg(test)]
mod tests;
