//! Bounded Server-Sent Event framing for live review publications.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use workdeck_core::{has_exact_keys, is_review_sha256_digest};
use workdeck_review::{
    ExpectedReviewResource, REVIEW_RESOURCE_CHUNK_BYTES, ReviewAssemblyResult, ReviewAssemblyStep,
    ReviewChunkAssembler, ReviewChunkAssemblerOptions, ReviewDigestFn, ReviewPublicationAddress,
    ReviewResourceChunk, ReviewResourceErrorCode, parse_review_generation, review_resource_failure,
};

use crate::{MAX_WORKDECK_REVIEW_ENVELOPE_BYTES, MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES};

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewEventTypeV1 {
    Publication,
    Disconnect,
}

pub const REVIEW_EVENT_TYPES: [ReviewEventTypeV1; 2] = [
    ReviewEventTypeV1::Publication,
    ReviewEventTypeV1::Disconnect,
];

impl ReviewEventTypeV1 {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Publication => "publication",
            Self::Disconnect => "disconnect",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewEventFramePhase {
    Begin,
    Chunk,
    End,
}

const REVIEW_EVENT_FRAME_PHASES: [ReviewEventFramePhase; 3] = [
    ReviewEventFramePhase::Begin,
    ReviewEventFramePhase::Chunk,
    ReviewEventFramePhase::End,
];

impl ReviewEventFramePhase {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Chunk => "chunk",
            Self::End => "end",
        }
    }
}

pub const MAX_REVIEW_EVENT_PAYLOAD_BYTES: u64 = MAX_WORKDECK_REVIEW_ENVELOPE_BYTES;
pub const REVIEW_EVENT_CHUNK_BYTES: u64 = REVIEW_RESOURCE_CHUNK_BYTES;
pub const MAX_REVIEW_EVENT_CHUNKS: u64 =
    MAX_REVIEW_EVENT_PAYLOAD_BYTES.div_ceil(REVIEW_EVENT_CHUNK_BYTES);
pub const MAX_REVIEW_EVENT_ID_BYTES: usize = MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES;
pub const REVIEW_EVENT_STREAM_CONTENT_TYPE: &str = "text/event-stream; charset=utf-8";
pub const REVIEW_EVENT_HEARTBEAT_FRAME: &str = ": workdeck-review-heartbeat\n\n";

#[must_use]
pub fn review_event_frame_name(
    event_type: ReviewEventTypeV1,
    phase: Option<ReviewEventFramePhase>,
) -> String {
    phase.map_or_else(
        || event_type.as_str().to_owned(),
        |phase| format!("{}-{}", event_type.as_str(), phase.as_str()),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewEventFrameIdentity {
    pub event_type: ReviewEventTypeV1,
    pub phase: Option<ReviewEventFramePhase>,
}

#[must_use]
pub fn parse_review_event_frame_name(name: &str) -> Option<ReviewEventFrameIdentity> {
    for event_type in REVIEW_EVENT_TYPES {
        if name == event_type.as_str() {
            return Some(ReviewEventFrameIdentity {
                event_type,
                phase: None,
            });
        }
        for phase in REVIEW_EVENT_FRAME_PHASES {
            if name == review_event_frame_name(event_type, Some(phase)) {
                return Some(ReviewEventFrameIdentity {
                    event_type,
                    phase: Some(phase),
                });
            }
        }
    }
    None
}

#[must_use]
pub fn review_event_id(
    event_type: ReviewEventTypeV1,
    address: &ReviewPublicationAddress,
) -> String {
    format!(
        "revent:{}:{}@{}",
        event_type.as_str(),
        address.generation,
        address.state_revision
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewEventIdentity {
    pub event_type: ReviewEventTypeV1,
    pub address: ReviewPublicationAddress,
}

#[must_use]
pub fn parse_review_event_id(value: &str) -> Option<ReviewEventIdentity> {
    if value.len() > MAX_REVIEW_EVENT_ID_BYTES {
        return None;
    }
    let value = value.strip_prefix("revent:")?;
    let (event_type, position) = value.split_once(':')?;
    let event_type = REVIEW_EVENT_TYPES
        .into_iter()
        .find(|candidate| candidate.as_str() == event_type)?;
    let (generation, state_revision) = position.rsplit_once('@')?;
    if state_revision.is_empty()
        || state_revision.len() > 15
        || !state_revision.bytes().all(|byte| byte.is_ascii_digit())
        || parse_review_generation(generation).is_none()
    {
        return None;
    }
    let state_revision = state_revision.parse::<u64>().ok()?;
    (state_revision <= MAX_SAFE_INTEGER).then(|| ReviewEventIdentity {
        event_type,
        address: ReviewPublicationAddress {
            generation: generation.to_owned(),
            state_revision,
        },
    })
}

#[must_use]
pub fn is_review_event_id(value: &str) -> bool {
    parse_review_event_id(value).is_some()
}

/// Parse the dynamically typed wire boundary without coercing non-string values.
#[must_use]
pub fn parse_review_event_id_value(value: &Value) -> Option<ReviewEventIdentity> {
    parse_review_event_id(value.as_str()?)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEventFrameV1 {
    pub event_id: String,
    pub generation: String,
    pub state_revision: u64,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEventBeginV1 {
    pub event_id: String,
    pub generation: String,
    pub state_revision: u64,
    pub encoding: String,
    pub content_size: u64,
    pub content_digest: String,
    pub chunk_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEventChunkV1 {
    pub event_id: String,
    pub generation: String,
    pub offset: u64,
    pub byte_length: u64,
    pub encoding: String,
    pub data: String,
    pub content_digest: String,
    pub content_size: u64,
    pub eof: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewEventEndV1 {
    pub event_id: String,
    pub generation: String,
    pub content_size: u64,
    pub content_digest: String,
    pub chunk_count: u64,
}

#[must_use]
pub fn review_event_chunk_as_resource_chunk(chunk: &ReviewEventChunkV1) -> ReviewResourceChunk {
    ReviewResourceChunk {
        generation: chunk.generation.clone(),
        resource_id: chunk.event_id.clone(),
        offset: chunk.offset,
        byte_length: chunk.byte_length,
        encoding: chunk.encoding.clone(),
        data: chunk.data.clone(),
        content_digest: chunk.content_digest.clone(),
        content_size: chunk.content_size,
        eof: chunk.eof,
    }
}

fn exact_object<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a serde_json::Map<String, Value>> {
    let object = value.as_object()?;
    has_exact_keys(object, keys).then_some(object)
}

fn count(value: &Value) -> Option<u64> {
    value.as_u64().filter(|value| *value <= MAX_SAFE_INTEGER)
}

fn parse_exact<T: for<'de> Deserialize<'de>>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

#[must_use]
pub fn parse_review_event_frame(value: &Value) -> Option<ReviewEventFrameV1> {
    let object = exact_object(
        value,
        &["eventId", "generation", "stateRevision", "payload"],
    )?;
    if !is_review_event_id(object.get("eventId")?.as_str()?)
        || parse_review_generation(object.get("generation")?.as_str()?).is_none()
        || count(object.get("stateRevision")?).is_none()
    {
        return None;
    }
    parse_exact(value)
}

#[must_use]
pub fn parse_review_event_begin(value: &Value) -> Option<ReviewEventBeginV1> {
    let object = exact_object(
        value,
        &[
            "eventId",
            "generation",
            "stateRevision",
            "encoding",
            "contentSize",
            "contentDigest",
            "chunkCount",
        ],
    )?;
    if !is_review_event_id(object.get("eventId")?.as_str()?)
        || parse_review_generation(object.get("generation")?.as_str()?).is_none()
        || count(object.get("stateRevision")?).is_none()
        || object.get("encoding")?.as_str()? != "base64"
        || count(object.get("contentSize")?)? > MAX_REVIEW_EVENT_PAYLOAD_BYTES
        || !is_review_sha256_digest(object.get("contentDigest")?.as_str()?)
        || !(1..=MAX_REVIEW_EVENT_CHUNKS).contains(&count(object.get("chunkCount")?)?)
    {
        return None;
    }
    parse_exact(value)
}

#[must_use]
pub fn parse_review_event_chunk(value: &Value) -> Option<ReviewEventChunkV1> {
    let object = exact_object(
        value,
        &[
            "eventId",
            "generation",
            "offset",
            "byteLength",
            "encoding",
            "data",
            "contentDigest",
            "contentSize",
            "eof",
        ],
    )?;
    if !is_review_event_id(object.get("eventId")?.as_str()?)
        || parse_review_generation(object.get("generation")?.as_str()?).is_none()
        || count(object.get("offset")?).is_none()
        || count(object.get("byteLength")?)? > REVIEW_EVENT_CHUNK_BYTES
        || object.get("encoding")?.as_str()? != "base64"
        || !object.get("data")?.is_string()
        || !is_review_sha256_digest(object.get("contentDigest")?.as_str()?)
        || count(object.get("contentSize")?)? > MAX_REVIEW_EVENT_PAYLOAD_BYTES
        || !object.get("eof")?.is_boolean()
    {
        return None;
    }
    parse_exact(value)
}

#[must_use]
pub fn parse_review_event_end(value: &Value) -> Option<ReviewEventEndV1> {
    let object = exact_object(
        value,
        &[
            "eventId",
            "generation",
            "contentSize",
            "contentDigest",
            "chunkCount",
        ],
    )?;
    if !is_review_event_id(object.get("eventId")?.as_str()?)
        || parse_review_generation(object.get("generation")?.as_str()?).is_none()
        || count(object.get("contentSize")?)? > MAX_REVIEW_EVENT_PAYLOAD_BYTES
        || !is_review_sha256_digest(object.get("contentDigest")?.as_str()?)
        || !(1..=MAX_REVIEW_EVENT_CHUNKS).contains(&count(object.get("chunkCount")?)?)
    {
        return None;
    }
    parse_exact(value)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewEventSseFrame {
    pub id: Option<String>,
    pub event: String,
    pub data: Value,
}

pub fn encode_review_event_frame(frame: &ReviewEventSseFrame) -> Result<String, serde_json::Error> {
    let id = frame
        .id
        .as_ref()
        .map_or_else(String::new, |id| format!("id: {id}\n"));
    Ok(format!(
        "{id}event: {}\ndata: {}\n\n",
        frame.event,
        serde_json::to_string(&frame.data)?
    ))
}

#[must_use]
pub const fn review_event_chunk_count(content_size: u64) -> u64 {
    let count = content_size.div_ceil(REVIEW_EVENT_CHUNK_BYTES);
    if count == 0 { 1 } else { count }
}

pub struct PlanReviewEventInput<'a, EncodeChunk> {
    pub event_type: ReviewEventTypeV1,
    pub address: &'a ReviewPublicationAddress,
    pub body: Value,
    pub payload: &'a [u8],
    pub content_digest: &'a str,
    pub encode_chunk: EncodeChunk,
    pub chunk_bytes: Option<u64>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Review event payload of {actual_bytes} bytes exceeds the {max_bytes}-byte stream bound.")]
pub struct ReviewEventTooLargeError {
    pub actual_bytes: u64,
    pub max_bytes: u64,
}

pub fn plan_review_event_frames<EncodeChunk>(
    input: PlanReviewEventInput<'_, EncodeChunk>,
) -> Result<Vec<ReviewEventSseFrame>, ReviewEventTooLargeError>
where
    EncodeChunk: Fn(&[u8]) -> String,
{
    let content_size = u64::try_from(input.payload.len()).unwrap_or(u64::MAX);
    if content_size > MAX_REVIEW_EVENT_PAYLOAD_BYTES {
        return Err(ReviewEventTooLargeError {
            actual_bytes: content_size,
            max_bytes: MAX_REVIEW_EVENT_PAYLOAD_BYTES,
        });
    }
    let requested = input
        .chunk_bytes
        .unwrap_or(REVIEW_EVENT_CHUNK_BYTES)
        .min(REVIEW_EVENT_CHUNK_BYTES);
    let chunk_bytes = requested.max(content_size.div_ceil(MAX_REVIEW_EVENT_CHUNKS));
    let event_id = review_event_id(input.event_type, input.address);

    if content_size <= chunk_bytes {
        return Ok(vec![ReviewEventSseFrame {
            id: Some(event_id.clone()),
            event: review_event_frame_name(input.event_type, None),
            data: serde_json::to_value(ReviewEventFrameV1 {
                event_id,
                generation: input.address.generation.clone(),
                state_revision: input.address.state_revision,
                payload: input.body,
            })
            .expect("review event frame is serializable"),
        }]);
    }

    let chunk_count = content_size.div_ceil(chunk_bytes);
    let mut frames = Vec::with_capacity(usize::try_from(chunk_count + 2).unwrap_or(usize::MAX));
    frames.push(ReviewEventSseFrame {
        id: None,
        event: review_event_frame_name(input.event_type, Some(ReviewEventFramePhase::Begin)),
        data: serde_json::to_value(ReviewEventBeginV1 {
            event_id: event_id.clone(),
            generation: input.address.generation.clone(),
            state_revision: input.address.state_revision,
            encoding: "base64".into(),
            content_size,
            content_digest: input.content_digest.into(),
            chunk_count,
        })
        .expect("review event begin is serializable"),
    });
    for index in 0..chunk_count {
        let offset = index * chunk_bytes;
        let end = (offset + chunk_bytes).min(content_size);
        let slice = &input.payload[usize::try_from(offset).expect("payload offset fits usize")
            ..usize::try_from(end).expect("payload end fits usize")];
        frames.push(ReviewEventSseFrame {
            id: None,
            event: review_event_frame_name(input.event_type, Some(ReviewEventFramePhase::Chunk)),
            data: serde_json::to_value(ReviewEventChunkV1 {
                event_id: event_id.clone(),
                generation: input.address.generation.clone(),
                offset,
                byte_length: u64::try_from(slice.len()).expect("slice length fits u64"),
                encoding: "base64".into(),
                data: (input.encode_chunk)(slice),
                content_digest: input.content_digest.into(),
                content_size,
                eof: index == chunk_count - 1,
            })
            .expect("review event chunk is serializable"),
        });
    }
    frames.push(ReviewEventSseFrame {
        id: Some(event_id.clone()),
        event: review_event_frame_name(input.event_type, Some(ReviewEventFramePhase::End)),
        data: serde_json::to_value(ReviewEventEndV1 {
            event_id,
            generation: input.address.generation.clone(),
            content_size,
            content_digest: input.content_digest.into(),
            chunk_count,
        })
        .expect("review event end is serializable"),
    });
    Ok(frames)
}

pub struct ReviewEventAssembler {
    assembler: Option<ReviewChunkAssembler>,
    begin: ReviewEventBeginV1,
    accepted: u64,
}

impl ReviewEventAssembler {
    #[must_use]
    pub fn new(begin: ReviewEventBeginV1, digest: ReviewDigestFn) -> Self {
        let assembler = ReviewChunkAssembler::new(ReviewChunkAssemblerOptions {
            resource_id: begin.event_id.clone(),
            generation: begin.generation.clone(),
            digest,
            max_bytes: MAX_REVIEW_EVENT_PAYLOAD_BYTES,
            expected: Some(ExpectedReviewResource {
                byte_length: begin.content_size,
                digest: begin.content_digest.clone(),
            }),
        });
        Self {
            assembler: Some(assembler),
            begin,
            accepted: 0,
        }
    }

    #[must_use]
    pub const fn chunk_count(&self) -> u64 {
        self.accepted
    }

    #[must_use]
    pub fn accept(&mut self, chunk: &ReviewEventChunkV1, bytes: &[u8]) -> ReviewAssemblyStep {
        if chunk.event_id != self.begin.event_id {
            return ReviewAssemblyStep::Failed(review_resource_failure(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review event {} received a chunk for {}.",
                    self.begin.event_id, chunk.event_id
                ),
            ));
        }
        if self.accepted >= self.begin.chunk_count {
            return ReviewAssemblyStep::Failed(review_resource_failure(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review event {} sent more than the {} chunks it declared.",
                    self.begin.event_id, self.begin.chunk_count
                ),
            ));
        }
        let step = self
            .assembler
            .as_mut()
            .expect("event assembler is available until finish")
            .accept(&review_event_chunk_as_resource_chunk(chunk), bytes);
        if matches!(step, ReviewAssemblyStep::Accepted { .. }) {
            self.accepted += 1;
        }
        step
    }

    #[must_use]
    pub fn finish(mut self, end: &ReviewEventEndV1) -> ReviewAssemblyResult {
        if end.event_id != self.begin.event_id
            || end.content_size != self.begin.content_size
            || end.content_digest != self.begin.content_digest
            || end.chunk_count != self.begin.chunk_count
            || end.chunk_count != self.accepted
        {
            return ReviewAssemblyResult::Failed(review_resource_failure(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review event {} ended with a declaration it did not begin with.",
                    self.begin.event_id
                ),
            ));
        }
        self.assembler
            .take()
            .expect("event assembler is consumed exactly once")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde_json::json;
    use workdeck_core::review_digest;

    use super::*;

    fn address() -> ReviewPublicationAddress {
        ReviewPublicationAddress {
            generation: "generation:test:3".into(),
            state_revision: 7,
        }
    }

    fn frame(body: Value, chunk_bytes: u64) -> Vec<ReviewEventSseFrame> {
        let payload = serde_json::to_vec(&body).unwrap();
        let digest = review_digest(&payload);
        plan_review_event_frames(PlanReviewEventInput {
            event_type: ReviewEventTypeV1::Publication,
            address: &address(),
            body,
            payload: &payload,
            content_digest: &digest,
            encode_chunk: encode_standard,
            chunk_bytes: Some(chunk_bytes),
        })
        .unwrap()
    }

    fn parse_frame_data(frame: &ReviewEventSseFrame) -> ReviewEventFrameV1 {
        parse_review_event_frame(&frame.data).unwrap()
    }

    fn parse_begin(frame: &ReviewEventSseFrame) -> ReviewEventBeginV1 {
        parse_review_event_begin(&frame.data).unwrap()
    }

    fn parse_chunk(frame: &ReviewEventSseFrame) -> ReviewEventChunkV1 {
        parse_review_event_chunk(&frame.data).unwrap()
    }

    fn parse_end(frame: &ReviewEventSseFrame) -> ReviewEventEndV1 {
        parse_review_event_end(&frame.data).unwrap()
    }

    fn digest_fn() -> ReviewDigestFn {
        Arc::new(review_digest)
    }

    fn encode_standard(bytes: &[u8]) -> String {
        STANDARD.encode(bytes)
    }

    #[test]
    fn every_frame_name_round_trips_and_unknown_names_are_refused() {
        for event_type in REVIEW_EVENT_TYPES {
            assert_eq!(
                parse_review_event_frame_name(&review_event_frame_name(event_type, None)),
                Some(ReviewEventFrameIdentity {
                    event_type,
                    phase: None,
                })
            );
            for phase in REVIEW_EVENT_FRAME_PHASES {
                assert_eq!(
                    parse_review_event_frame_name(&review_event_frame_name(
                        event_type,
                        Some(phase)
                    )),
                    Some(ReviewEventFrameIdentity {
                        event_type,
                        phase: Some(phase),
                    })
                );
            }
        }
        for name in ["publication-middle", "state", ""] {
            assert_eq!(parse_review_event_frame_name(name), None);
        }
    }

    #[test]
    fn event_ids_round_trip_positions_and_reject_attacker_controlled_shapes() {
        let id = review_event_id(ReviewEventTypeV1::Publication, &address());
        assert_eq!(
            parse_review_event_id(&id),
            Some(ReviewEventIdentity {
                event_type: ReviewEventTypeV1::Publication,
                address: address(),
            })
        );
        for invalid in [
            "revent:publication:not-a-generation@1",
            "revent:state:generation:test:3@1",
            "generation:test:3@1",
            "revent:publication:generation:test:3@9999999999999999999999999999999999999999",
        ] {
            assert_eq!(parse_review_event_id(invalid), None);
        }
        assert_eq!(parse_review_event_id_value(&json!(42)), None);
    }

    #[test]
    fn bounds_are_derived_and_empty_payloads_still_count_as_one_chunk() {
        assert_eq!(
            MAX_REVIEW_EVENT_PAYLOAD_BYTES,
            MAX_WORKDECK_REVIEW_ENVELOPE_BYTES
        );
        assert!(std::hint::black_box(MAX_REVIEW_EVENT_CHUNKS) > 0);
        assert_eq!(review_event_chunk_count(0), 1);
        assert_eq!(review_event_chunk_count(1), 1);
    }

    #[test]
    fn payload_past_the_shared_stream_bound_is_refused() {
        let payload = vec![0; usize::try_from(MAX_REVIEW_EVENT_PAYLOAD_BYTES + 1).unwrap()];
        let error = plan_review_event_frames(PlanReviewEventInput {
            event_type: ReviewEventTypeV1::Publication,
            address: &address(),
            body: json!({}),
            payload: &payload,
            content_digest: &review_digest(&payload),
            encode_chunk: encode_standard,
            chunk_bytes: None,
        })
        .unwrap_err();
        assert_eq!(error.actual_bytes, MAX_REVIEW_EVENT_PAYLOAD_BYTES + 1);
    }

    #[test]
    fn small_event_is_one_complete_frame_carrying_the_body() {
        let body = json!({"hello": "world"});
        let frames = frame(body.clone(), 4096);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "publication");
        assert_eq!(
            frames[0].id,
            Some(review_event_id(ReviewEventTypeV1::Publication, &address()))
        );
        assert_eq!(parse_frame_data(&frames[0]).payload, body);
    }

    #[test]
    fn only_the_completing_chunked_frame_has_an_event_id() {
        let frames = frame(json!({"value": "x".repeat(200)}), 32);
        assert_eq!(frames.first().unwrap().event, "publication-begin");
        assert_eq!(frames.last().unwrap().event, "publication-end");
        assert!(
            frames[1..frames.len() - 1]
                .iter()
                .all(|entry| entry.event == "publication-chunk")
        );
        assert!(
            frames[..frames.len() - 1]
                .iter()
                .all(|entry| entry.id.is_none())
        );
        assert_eq!(
            frames.last().unwrap().id,
            Some(review_event_id(ReviewEventTypeV1::Publication, &address()))
        );
    }

    #[test]
    fn chunked_begin_and_end_repeat_size_digest_and_count() {
        let frames = frame(json!({"value": "y".repeat(200)}), 64);
        let begin = parse_begin(&frames[0]);
        let end = parse_end(frames.last().unwrap());
        assert_eq!(begin.chunk_count, u64::try_from(frames.len() - 2).unwrap());
        assert_eq!(end.chunk_count, begin.chunk_count);
        assert_eq!(end.content_size, begin.content_size);
        assert_eq!(end.content_digest, begin.content_digest);
    }

    #[test]
    fn sse_frame_is_one_record_and_payload_newlines_cannot_split_it() {
        let text = encode_review_event_frame(&ReviewEventSseFrame {
            id: Some("revent:x".into()),
            event: "publication".into(),
            data: json!({"a": 1}),
        })
        .unwrap();
        assert_eq!(
            text,
            "id: revent:x\nevent: publication\ndata: {\"a\":1}\n\n"
        );
        let text = encode_review_event_frame(&ReviewEventSseFrame {
            id: None,
            event: "publication".into(),
            data: json!({"a": "one\ntwo\n\nthree"}),
        })
        .unwrap();
        assert_eq!(text.split("\n\n").count(), 2);
    }

    fn assemble(frames: &[ReviewEventSseFrame]) -> ReviewAssemblyResult {
        let mut assembler = ReviewEventAssembler::new(parse_begin(&frames[0]), digest_fn());
        for entry in &frames[1..frames.len() - 1] {
            let chunk = parse_chunk(entry);
            let bytes = STANDARD.decode(&chunk.data).unwrap();
            if let ReviewAssemblyStep::Failed(failure) = assembler.accept(&chunk, &bytes) {
                return ReviewAssemblyResult::Failed(failure);
            }
        }
        assembler.finish(&parse_end(frames.last().unwrap()))
    }

    #[test]
    fn chunked_payload_reassembles_byte_for_byte() {
        let body = json!({
            "files": (0..40).map(|index| format!("file-{index}")).collect::<Vec<_>>()
        });
        match assemble(&frame(body.clone(), 48)) {
            ReviewAssemblyResult::Assembled { bytes } => {
                assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), body);
            }
            ReviewAssemblyResult::Failed(failure) => panic!("{failure:?}"),
        }
    }

    #[test]
    fn altered_bytes_are_reported_as_integrity_failure() {
        let frames = frame(json!({"value": "z".repeat(200)}), 64);
        let mut assembler = ReviewEventAssembler::new(parse_begin(&frames[0]), digest_fn());
        for (index, entry) in frames[1..frames.len() - 1].iter().enumerate() {
            let chunk = parse_chunk(entry);
            let mut bytes = STANDARD.decode(&chunk.data).unwrap();
            if index == 0 {
                bytes[0] ^= 0xff;
            }
            assert!(matches!(
                assembler.accept(&chunk, &bytes),
                ReviewAssemblyStep::Accepted { .. }
            ));
        }
        assert!(matches!(
            assembler.finish(&parse_end(frames.last().unwrap())),
            ReviewAssemblyResult::Failed(failure)
                if failure.code == ReviewResourceErrorCode::ResourceIntegrity
        ));
    }

    #[test]
    fn chunk_for_another_event_and_disagreeing_end_are_refused() {
        let frames = frame(json!({"value": "q".repeat(200)}), 64);
        let begin = parse_begin(&frames[0]);
        let mut assembler = ReviewEventAssembler::new(begin.clone(), digest_fn());
        let mut chunk = parse_chunk(&frames[1]);
        let bytes = STANDARD.decode(&chunk.data).unwrap();
        chunk.event_id = review_event_id(ReviewEventTypeV1::Disconnect, &address());
        assert!(matches!(
            assembler.accept(&chunk, &bytes),
            ReviewAssemblyStep::Failed(failure)
                if failure.code == ReviewResourceErrorCode::ResourceIntegrity
        ));

        let mut assembler = ReviewEventAssembler::new(begin, digest_fn());
        for entry in &frames[1..frames.len() - 1] {
            let chunk = parse_chunk(entry);
            let bytes = STANDARD.decode(&chunk.data).unwrap();
            let _ = assembler.accept(&chunk, &bytes);
        }
        let mut end = parse_end(frames.last().unwrap());
        end.chunk_count += 1;
        assert!(matches!(
            assembler.finish(&end),
            ReviewAssemblyResult::Failed(failure)
                if failure.code == ReviewResourceErrorCode::ResourceIntegrity
        ));
    }

    fn valid_begin_value() -> Value {
        serde_json::to_value(ReviewEventBeginV1 {
            event_id: review_event_id(ReviewEventTypeV1::Publication, &address()),
            generation: address().generation,
            state_revision: address().state_revision,
            encoding: "base64".into(),
            content_size: 10,
            content_digest: "a".repeat(64),
            chunk_count: 1,
        })
        .unwrap()
    }

    #[test]
    fn begin_parser_is_exact_and_enforces_encoding_digest_and_shared_bounds() {
        let begin = valid_begin_value();
        assert_eq!(
            parse_review_event_begin(&begin),
            serde_json::from_value(begin.clone()).ok()
        );
        let mut extra = begin.clone();
        extra
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(1));
        assert_eq!(parse_review_event_begin(&extra), None);
        let mut encoding = begin.clone();
        encoding["encoding"] = json!("hex");
        assert_eq!(parse_review_event_begin(&encoding), None);
        let mut digest = begin.clone();
        digest["contentDigest"] = json!("A".repeat(64));
        assert_eq!(parse_review_event_begin(&digest), None);
        let mut size = begin.clone();
        size["contentSize"] = json!(MAX_REVIEW_EVENT_PAYLOAD_BYTES + 1);
        assert_eq!(parse_review_event_begin(&size), None);
        let mut chunks = begin.clone();
        chunks["chunkCount"] = json!(MAX_REVIEW_EVENT_CHUNKS + 1);
        assert_eq!(parse_review_event_begin(&chunks), None);
        chunks["chunkCount"] = json!(0);
        assert_eq!(parse_review_event_begin(&chunks), None);
    }

    #[test]
    fn chunk_parser_enforces_one_window_and_end_parser_requires_every_field() {
        let begin = parse_review_event_begin(&valid_begin_value()).unwrap();
        let mut chunk = serde_json::to_value(ReviewEventChunkV1 {
            event_id: begin.event_id.clone(),
            generation: begin.generation.clone(),
            offset: 0,
            byte_length: MAX_REVIEW_EVENT_PAYLOAD_BYTES,
            encoding: "base64".into(),
            data: String::new(),
            content_digest: begin.content_digest.clone(),
            content_size: 10,
            eof: true,
        })
        .unwrap();
        assert_eq!(parse_review_event_chunk(&chunk), None);
        chunk["byteLength"] = json!(10);
        assert!(parse_review_event_chunk(&chunk).is_some());

        let mut end = serde_json::to_value(ReviewEventEndV1 {
            event_id: begin.event_id,
            generation: begin.generation,
            content_size: 10,
            content_digest: begin.content_digest,
            chunk_count: 1,
        })
        .unwrap();
        assert!(parse_review_event_end(&end).is_some());
        end.as_object_mut().unwrap().remove("chunkCount");
        assert_eq!(parse_review_event_end(&end), None);
    }
}
