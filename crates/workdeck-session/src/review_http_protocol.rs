//! Authenticated loopback HTTP routes for one live review publication.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;
use workdeck_review::{
    ReviewPublicationAddress, ReviewResourceDescriptor, parse_review_generation,
};

use crate::WorkdeckReviewClientErrorCodeV1;

pub const WORKDECK_REVIEW_PROTOCOL_VERSION: u32 = 1;
pub const WORKDECK_REVIEW_HTTP_PATH_PREFIX: &str = "/review-api";
pub const WORKDECK_REVIEW_PAGE_PATH_PREFIX: &str = "/review";
pub const WORKDECK_REVIEW_CAPABILITY_HEADER: &str = "workdeck-review-capability";
pub const WORKDECK_REVIEW_CAPABILITY_FRAGMENT_KEY: &str = "capability";
pub const REVIEW_CAPABILITY_ENTROPY_BYTES: usize = 32;
pub const REVIEW_CAPABILITY_TOKEN_LENGTH: usize = (REVIEW_CAPABILITY_ENTROPY_BYTES * 8).div_ceil(6);
pub const MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES: usize = 1024;

#[must_use]
pub fn is_review_capability_token(value: &str) -> bool {
    value.len() == REVIEW_CAPABILITY_TOKEN_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkdeckReviewHttpRoute {
    Publication {
        session_id: String,
    },
    Events {
        session_id: String,
    },
    Actions {
        session_id: String,
    },
    Resource {
        session_id: String,
        generation: String,
        resource_id: String,
    },
}

fn encode_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            )
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn decode_component(value: &str) -> Option<String> {
    if value.len() > MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES * 3 {
        return None;
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let pair = bytes.get(index + 1..index + 3)?;
        let pair = std::str::from_utf8(pair).ok()?;
        decoded.push(u8::from_str_radix(pair, 16).ok()?);
        index += 3;
    }
    String::from_utf8(decoded).ok()
}

fn is_path_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES
        && !value.contains(['/', '\\'])
}

fn decoded_identifier(value: &str) -> Option<String> {
    let decoded = decode_component(value)?;
    is_path_identifier(&decoded).then_some(decoded)
}

#[must_use]
pub fn review_http_path(route: &WorkdeckReviewHttpRoute) -> String {
    let session_id = match route {
        WorkdeckReviewHttpRoute::Publication { session_id }
        | WorkdeckReviewHttpRoute::Events { session_id }
        | WorkdeckReviewHttpRoute::Actions { session_id }
        | WorkdeckReviewHttpRoute::Resource { session_id, .. } => session_id,
    };
    let base = format!(
        "{WORKDECK_REVIEW_HTTP_PATH_PREFIX}/{}",
        encode_component(session_id)
    );
    match route {
        WorkdeckReviewHttpRoute::Publication { .. } => format!("{base}/publication"),
        WorkdeckReviewHttpRoute::Events { .. } => format!("{base}/events"),
        WorkdeckReviewHttpRoute::Actions { .. } => format!("{base}/actions"),
        WorkdeckReviewHttpRoute::Resource {
            generation,
            resource_id,
            ..
        } => format!(
            "{base}/resources/{}/{}",
            encode_component(generation),
            encode_component(resource_id)
        ),
    }
}

/// Strictly parse one review route without guessing at malformed paths.
#[must_use]
pub fn parse_review_http_path(pathname: &str) -> Option<WorkdeckReviewHttpRoute> {
    let parts = pathname.split('/').collect::<Vec<_>>();
    if parts.first().copied() != Some("") || parts.get(1).copied() != Some("review-api") {
        return None;
    }
    let session_id = decoded_identifier(parts.get(2)?)?;
    if parts.len() == 4 {
        return match parts[3] {
            "publication" => Some(WorkdeckReviewHttpRoute::Publication { session_id }),
            "events" => Some(WorkdeckReviewHttpRoute::Events { session_id }),
            "actions" => Some(WorkdeckReviewHttpRoute::Actions { session_id }),
            _ => None,
        };
    }
    if parts.len() == 6 && parts[3] == "resources" {
        return Some(WorkdeckReviewHttpRoute::Resource {
            session_id,
            generation: decoded_identifier(parts[4])?,
            resource_id: decoded_identifier(parts[5])?,
        });
    }
    None
}

#[must_use]
pub fn review_page_path(session_id: &str) -> String {
    format!(
        "{WORKDECK_REVIEW_PAGE_PATH_PREFIX}/{}/",
        encode_component(session_id)
    )
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ReviewUrlError {
    #[error("review origin must be an absolute URL with an authority")]
    InvalidOrigin,
}

fn origin_root(origin: &str) -> Result<&str, ReviewUrlError> {
    let scheme = origin.find("://").ok_or(ReviewUrlError::InvalidOrigin)?;
    if scheme == 0 {
        return Err(ReviewUrlError::InvalidOrigin);
    }
    let authority_start = scheme + 3;
    let authority_end = origin[authority_start..]
        .find(['/', '?', '#'])
        .map_or(origin.len(), |offset| authority_start + offset);
    if authority_end == authority_start {
        return Err(ReviewUrlError::InvalidOrigin);
    }
    Ok(&origin[..authority_end])
}

/// Build the browser URL with the clear capability only in its fragment.
pub fn review_url(
    origin: &str,
    session_id: &str,
    capability: &str,
) -> Result<String, ReviewUrlError> {
    Ok(format!(
        "{}{}#{WORKDECK_REVIEW_CAPABILITY_FRAGMENT_KEY}={capability}",
        origin_root(origin)?,
        review_page_path(session_id)
    ))
}

fn decode_form_component(value: &str) -> Option<String> {
    decode_component(&value.replace('+', " "))
}

/// Read a valid capability from a URL fragment, with or without `#`.
#[must_use]
pub fn parse_review_capability_fragment(fragment: &str) -> Option<String> {
    let fragment = fragment.strip_prefix('#').unwrap_or(fragment);
    for field in fragment.split('&') {
        let (key, value) = field.split_once('=').unwrap_or((field, ""));
        if decode_form_component(key).as_deref() != Some(WORKDECK_REVIEW_CAPABILITY_FRAGMENT_KEY) {
            continue;
        }
        let capability = decode_form_component(value)?;
        return is_review_capability_token(&capability).then_some(capability);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkdeckReviewTransportErrorCode {
    Unauthorized,
    NoPublication,
    PayloadTooLarge,
    MethodNotAllowed,
    UnsupportedMediaType,
    ForbiddenOrigin,
    UnsupportedAction,
    TooManyStreams,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewHttpFailureV1 {
    pub ok: bool,
    pub code: WorkdeckReviewClientErrorCodeV1,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_generation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewResourceCatalogV1 {
    pub generation: String,
    pub file_keys_by_runtime_id: BTreeMap<String, String>,
    pub resources: Vec<ReviewResourceDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewPublicationBodyV1 {
    pub protocol_version: u32,
    pub session_id: String,
    pub publication: ReviewPublicationAddress,
    pub catalog: WorkdeckReviewResourceCatalogV1,
}

#[must_use]
pub fn valid_review_publication_body(body: &WorkdeckReviewPublicationBodyV1) -> bool {
    body.protocol_version == WORKDECK_REVIEW_PROTOCOL_VERSION
        && !body.session_id.is_empty()
        && parse_review_generation(&body.publication.generation).is_some()
        && body.catalog.generation == body.publication.generation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn routes() -> Vec<WorkdeckReviewHttpRoute> {
        vec![
            WorkdeckReviewHttpRoute::Publication {
                session_id: "session-1".into(),
            },
            WorkdeckReviewHttpRoute::Events {
                session_id: "session-1".into(),
            },
            WorkdeckReviewHttpRoute::Actions {
                session_id: "session-1".into(),
            },
            WorkdeckReviewHttpRoute::Resource {
                session_id: "session-1".into(),
                generation: "generation:producer:2".into(),
                resource_id: "resource:patch:file:abcdef01".into(),
            },
        ]
    }

    #[test]
    fn every_constructible_route_round_trips() {
        for route in routes() {
            assert_eq!(
                parse_review_http_path(&review_http_path(&route)),
                Some(route)
            );
        }
        let route = WorkdeckReviewHttpRoute::Events {
            session_id: "セッション 1 #a?b%c".into(),
        };
        let path = review_http_path(&route);
        assert!(!path.contains("セッション"));
        assert_eq!(parse_review_http_path(&path), Some(route));
    }

    #[test]
    fn parser_refuses_traversal_wrong_depth_unknown_leaves_and_encoded_separators() {
        for path in [
            "/review-api/session-1/../../etc",
            "/review-api/session-1",
            "/review-api/session-1/snapshot",
            "/review-api//events",
            "/review-api/session-1/resources/gen",
            "/session-api/session-1/events",
            "/review-api/a%2Fb/events",
            "/review-api/%ZZ/events",
            "/review-api/a%5Cb/events",
        ] {
            assert_eq!(parse_review_http_path(path), None, "{path}");
        }
    }

    #[test]
    fn capability_grammar_accepts_only_the_exact_width_and_alphabet() {
        let token = "a".repeat(REVIEW_CAPABILITY_TOKEN_LENGTH);
        assert!(is_review_capability_token(&token));
        assert!(!is_review_capability_token(&token[..token.len() - 1]));
        assert!(!is_review_capability_token(&(token.clone() + "a")));
        let mut bad = "a".repeat(REVIEW_CAPABILITY_TOKEN_LENGTH - 1);
        bad.push('+');
        assert!(!is_review_capability_token(&bad));
    }

    #[test]
    fn review_url_keeps_capability_out_of_the_request_target_and_round_trips_fragment() {
        let token = "a".repeat(REVIEW_CAPABILITY_TOKEN_LENGTH);
        let url = review_url("http://127.0.0.1:4300/base?q=1", "session-1", &token).unwrap();
        let (request, fragment) = url.split_once('#').unwrap();
        assert_eq!(request, "http://127.0.0.1:4300/review/session-1/");
        assert!(!request.contains(&token));
        assert_eq!(
            parse_review_capability_fragment(fragment),
            Some(token.clone())
        );
        assert_eq!(
            parse_review_capability_fragment(&format!("#{fragment}")),
            Some(token)
        );
        assert_eq!(parse_review_capability_fragment(""), None);
        assert_eq!(parse_review_capability_fragment("#other=value"), None);
        assert_eq!(parse_review_capability_fragment("#capability=short"), None);
    }
}
