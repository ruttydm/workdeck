//! Bounded, content-addressed resource vocabulary for bulky review data.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use workdeck_core::ReviewSide;

use crate::parse_review_generation;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewResourceKind {
    CanonicalFile,
    Patch,
    Source,
}

pub const REVIEW_RESOURCE_CHUNK_BYTES: u64 = 256 * 1024;
pub const MAX_REVIEW_SOURCE_RESOURCE_BYTES: u64 = 1_000_000;
pub const REVIEW_RESOURCE_LOAD_CONCURRENCY: usize = 4;
pub const MAX_REVIEW_RESOURCE_BYTES: u64 = 32 * 1024 * 1024;

#[must_use]
pub const fn review_resource_ceiling(kind: ReviewResourceKind) -> u64 {
    match kind {
        ReviewResourceKind::Source => MAX_REVIEW_SOURCE_RESOURCE_BYTES,
        ReviewResourceKind::CanonicalFile | ReviewResourceKind::Patch => MAX_REVIEW_RESOURCE_BYTES,
    }
}

pub const REVIEW_CANONICAL_FILE_CONTENT_TYPE: &str =
    "application/vnd.workdeck.review-file+json; charset=utf-8";
pub const REVIEW_PATCH_CONTENT_TYPE: &str = "text/x-diff; charset=utf-8";
pub const REVIEW_SOURCE_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResourceDescriptorBase {
    pub id: String,
    pub generation: String,
    pub file_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ReviewResourceDescriptor {
    CanonicalFile {
        #[serde(flatten)]
        descriptor: ReviewResourceDescriptorBase,
        content_type: String,
    },
    Patch {
        #[serde(flatten)]
        descriptor: ReviewResourceDescriptorBase,
        content_type: String,
    },
    Source {
        #[serde(flatten)]
        descriptor: ReviewResourceDescriptorBase,
        content_type: String,
        side: ReviewSide,
        source_identity: String,
    },
}

impl ReviewResourceDescriptor {
    #[must_use]
    pub const fn base(&self) -> &ReviewResourceDescriptorBase {
        match self {
            Self::CanonicalFile { descriptor, .. }
            | Self::Patch { descriptor, .. }
            | Self::Source { descriptor, .. } => descriptor,
        }
    }

    pub const fn base_mut(&mut self) -> &mut ReviewResourceDescriptorBase {
        match self {
            Self::CanonicalFile { descriptor, .. }
            | Self::Patch { descriptor, .. }
            | Self::Source { descriptor, .. } => descriptor,
        }
    }

    #[must_use]
    pub fn is_materialized(&self) -> bool {
        self.base().byte_length.is_some() && self.base().digest.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewResourceAddress {
    pub kind: ReviewResourceKind,
    pub file_key: String,
    pub side: Option<ReviewSide>,
}

#[must_use]
pub fn review_resource_id(address: &ReviewResourceAddress) -> String {
    let kind = match address.kind {
        ReviewResourceKind::CanonicalFile => "canonical-file",
        ReviewResourceKind::Patch => "patch",
        ReviewResourceKind::Source => "source",
    };
    match address.side {
        Some(ReviewSide::Old) => format!("resource:{kind}:old:{}", address.file_key),
        Some(ReviewSide::New) => format!("resource:{kind}:new:{}", address.file_key),
        None => format!("resource:{kind}:{}", address.file_key),
    }
}

fn valid_file_key(prefix: Option<&str>, hexadecimal: Option<&str>) -> Option<String> {
    let hexadecimal = hexadecimal?;
    (prefix == Some("file")
        && !hexadecimal.is_empty()
        && hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| format!("file:{hexadecimal}"))
}

#[must_use]
pub fn parse_review_resource_id(value: &str) -> Option<ReviewResourceAddress> {
    let fields = value.split(':').collect::<Vec<_>>();
    if fields.first().copied() != Some("resource") {
        return None;
    }
    match fields.as_slice() {
        [_, "canonical-file", prefix, hexadecimal] => Some(ReviewResourceAddress {
            kind: ReviewResourceKind::CanonicalFile,
            file_key: valid_file_key(Some(prefix), Some(hexadecimal))?,
            side: None,
        }),
        [_, "patch", prefix, hexadecimal] => Some(ReviewResourceAddress {
            kind: ReviewResourceKind::Patch,
            file_key: valid_file_key(Some(prefix), Some(hexadecimal))?,
            side: None,
        }),
        [_, "source", side, prefix, hexadecimal] => Some(ReviewResourceAddress {
            kind: ReviewResourceKind::Source,
            file_key: valid_file_key(Some(prefix), Some(hexadecimal))?,
            side: Some(match *side {
                "old" => ReviewSide::Old,
                "new" => ReviewSide::New,
                _ => return None,
            }),
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewResourceRange {
    pub offset: u64,
    pub length: u64,
}

#[must_use]
pub const fn is_review_resource_range(range: ReviewResourceRange) -> bool {
    range.offset <= MAX_SAFE_INTEGER
        && range.length > 0
        && range.length <= REVIEW_RESOURCE_CHUNK_BYTES
        && range.length <= MAX_SAFE_INTEGER
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadReviewResourceRequest {
    pub generation: String,
    pub resource_id: String,
    pub offset: u64,
    pub length: u64,
}

#[must_use]
pub fn parse_read_review_resource_request(value: &Value) -> Option<ReadReviewResourceRequest> {
    let object = value.as_object()?;
    let expected = ["generation", "resourceId", "offset", "length"];
    if object.len() != expected.len() || !expected.iter().all(|key| object.contains_key(*key)) {
        return None;
    }
    let generation = object.get("generation")?.as_str()?;
    parse_review_generation(generation)?;
    let resource_id = object.get("resourceId")?.as_str()?;
    if resource_id.is_empty() {
        return None;
    }
    let range = ReviewResourceRange {
        offset: object.get("offset")?.as_u64()?,
        length: object.get("length")?.as_u64()?,
    };
    is_review_resource_range(range).then(|| ReadReviewResourceRequest {
        generation: generation.to_owned(),
        resource_id: resource_id.to_owned(),
        offset: range.offset,
        length: range.length,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResourceChunk {
    pub generation: String,
    pub resource_id: String,
    pub offset: u64,
    pub byte_length: u64,
    pub encoding: String,
    pub data: String,
    pub content_digest: String,
    pub content_size: u64,
    pub eof: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewResourceErrorCode {
    UnknownResource,
    ResourceUnavailable,
    ResourceTooLarge,
    ResourceIntegrity,
    InvalidRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewRequestErrorCode {
    StaleGeneration,
    InvalidRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewResourceFailure {
    pub ok: bool,
    pub code: ReviewResourceErrorCode,
    pub message: String,
}

#[must_use]
pub fn review_resource_failure(
    code: ReviewResourceErrorCode,
    message: impl Into<String>,
) -> ReviewResourceFailure {
    ReviewResourceFailure {
        ok: false,
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE_KEY: &str = "file:0123456789abcdef";

    #[test]
    fn ids_round_trip_and_reject_kind_side_disagreement_or_invalid_grammar() {
        for address in [
            ReviewResourceAddress {
                kind: ReviewResourceKind::Patch,
                file_key: FILE_KEY.into(),
                side: None,
            },
            ReviewResourceAddress {
                kind: ReviewResourceKind::CanonicalFile,
                file_key: FILE_KEY.into(),
                side: None,
            },
            ReviewResourceAddress {
                kind: ReviewResourceKind::Source,
                file_key: FILE_KEY.into(),
                side: Some(ReviewSide::Old),
            },
            ReviewResourceAddress {
                kind: ReviewResourceKind::Source,
                file_key: FILE_KEY.into(),
                side: Some(ReviewSide::New),
            },
        ] {
            assert_eq!(
                parse_review_resource_id(&review_resource_id(&address)),
                Some(address)
            );
        }
        for invalid in [
            format!("resource:patch:new:{FILE_KEY}"),
            format!("resource:source:{FILE_KEY}"),
            "resource:unknown:file:abc".into(),
            format!("resource:patch:{FILE_KEY}:extra"),
            "patch:file:abc".into(),
            "resource:patch:file:ABC".into(),
        ] {
            assert!(parse_review_resource_id(&invalid).is_none(), "{invalid}");
        }
    }

    fn patch_descriptor(
        byte_length: Option<u64>,
        digest: Option<&str>,
    ) -> ReviewResourceDescriptor {
        ReviewResourceDescriptor::Patch {
            descriptor: ReviewResourceDescriptorBase {
                id: format!("resource:patch:{FILE_KEY}"),
                generation: "generation:p1:0".into(),
                file_key: FILE_KEY.into(),
                byte_length,
                digest: digest.map(str::to_owned),
            },
            content_type: REVIEW_PATCH_CONTENT_TYPE.into(),
        }
    }

    #[test]
    fn descriptor_is_materialized_only_with_both_measurements() {
        assert!(!patch_descriptor(None, None).is_materialized());
        assert!(!patch_descriptor(Some(10), None).is_materialized());
        assert!(!patch_descriptor(None, Some(&"a".repeat(64))).is_materialized());
        assert!(patch_descriptor(Some(10), Some(&"a".repeat(64))).is_materialized());
    }

    #[test]
    fn range_accepts_only_positive_safe_bounded_integer_windows() {
        assert!(is_review_resource_range(ReviewResourceRange {
            offset: 0,
            length: 1
        }));
        assert!(is_review_resource_range(ReviewResourceRange {
            offset: 10,
            length: REVIEW_RESOURCE_CHUNK_BYTES
        }));
        assert!(!is_review_resource_range(ReviewResourceRange {
            offset: 0,
            length: 0
        }));
        assert!(!is_review_resource_range(ReviewResourceRange {
            offset: 0,
            length: REVIEW_RESOURCE_CHUNK_BYTES + 1
        }));
        assert!(!is_review_resource_range(ReviewResourceRange {
            offset: MAX_SAFE_INTEGER + 1,
            length: 5
        }));
    }

    #[test]
    fn read_requests_require_exact_fields_generation_and_range() {
        let valid = serde_json::json!({
            "generation":"generation:p1:0",
            "resourceId":"resource:patch:file:abc",
            "offset":0,
            "length":1
        });
        assert!(parse_read_review_resource_request(&valid).is_some());
        for invalid in [
            serde_json::json!({"generation":"bad","resourceId":"x","offset":0,"length":1}),
            serde_json::json!({"generation":"generation:p1:0","resourceId":"","offset":0,"length":1}),
            serde_json::json!({"generation":"generation:p1:0","resourceId":"x","offset":0.5,"length":1}),
            serde_json::json!({"generation":"generation:p1:0","resourceId":"x","offset":0,"length":1,"extra":true}),
        ] {
            assert!(parse_read_review_resource_request(&invalid).is_none());
        }
    }

    #[test]
    fn ceilings_and_failures_use_the_shared_vocabulary() {
        assert_eq!(
            review_resource_ceiling(ReviewResourceKind::Source),
            MAX_REVIEW_SOURCE_RESOURCE_BYTES
        );
        assert_eq!(
            review_resource_ceiling(ReviewResourceKind::Patch),
            MAX_REVIEW_RESOURCE_BYTES
        );
        assert_eq!(
            review_resource_failure(ReviewResourceErrorCode::ResourceIntegrity, "bad"),
            ReviewResourceFailure {
                ok: false,
                code: ReviewResourceErrorCode::ResourceIntegrity,
                message: "bad".into()
            }
        );
    }
}
