//! Stateful, bounded verification of a resource chunk stream.

use std::sync::Arc;

use crate::{
    REVIEW_RESOURCE_CHUNK_BYTES, ReviewResourceChunk, ReviewResourceErrorCode,
    ReviewResourceFailure, review_resource_failure,
};

pub type ReviewDigestFn = Arc<dyn Fn(&[u8]) -> String + Send + Sync + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewAssemblyStep {
    Accepted { done: bool },
    Failed(ReviewResourceFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewAssemblyResult {
    Assembled { bytes: Vec<u8> },
    Failed(ReviewResourceFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedReviewResource {
    pub byte_length: u64,
    pub digest: String,
}

#[derive(Clone)]
pub struct ReviewChunkAssemblerOptions {
    pub resource_id: String,
    pub generation: String,
    pub digest: ReviewDigestFn,
    pub max_bytes: u64,
    pub expected: Option<ExpectedReviewResource>,
}

pub struct ReviewChunkAssembler {
    options: ReviewChunkAssemblerOptions,
    buffer: Option<Vec<u8>>,
    offset: u64,
    content_size: Option<u64>,
    content_digest: Option<String>,
    complete: bool,
    failed: Option<ReviewResourceFailure>,
}

impl ReviewChunkAssembler {
    #[must_use]
    pub fn new(options: ReviewChunkAssemblerOptions) -> Self {
        let (content_size, content_digest) =
            options.expected.as_ref().map_or((None, None), |value| {
                (Some(value.byte_length), Some(value.digest.clone()))
            });
        Self {
            options,
            buffer: None,
            offset: 0,
            content_size,
            content_digest,
            complete: false,
            failed: None,
        }
    }

    #[must_use]
    pub const fn next_offset(&self) -> u64 {
        self.offset
    }

    #[must_use]
    pub fn remaining_bytes(&self) -> Option<u64> {
        self.content_size
            .map(|size| size.saturating_sub(self.offset))
    }

    #[must_use]
    pub const fn declared_size(&self) -> Option<u64> {
        self.content_size
    }

    #[must_use]
    pub fn accept(&mut self, chunk: &ReviewResourceChunk, bytes: &[u8]) -> ReviewAssemblyStep {
        if let Some(failure) = &self.failed {
            return ReviewAssemblyStep::Failed(failure.clone());
        }
        if self.complete {
            return self.fail(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} received a chunk after end of stream.",
                    self.options.resource_id
                ),
            );
        }
        if chunk.resource_id != self.options.resource_id
            || chunk.generation != self.options.generation
        {
            return self.fail(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} received a chunk for {} in {}.",
                    self.options.resource_id, chunk.resource_id, chunk.generation
                ),
            );
        }
        if chunk.encoding != "base64" {
            return self.fail(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} declared an unsupported chunk encoding.",
                    self.options.resource_id
                ),
            );
        }
        let byte_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if chunk.offset != self.offset || chunk.byte_length != byte_length {
            return self.fail(
                ReviewResourceErrorCode::InvalidRange,
                format!(
                    "Review resource {} returned {} bytes at {}; {} was expected next.",
                    self.options.resource_id, chunk.byte_length, chunk.offset, self.offset
                ),
            );
        }
        if byte_length > REVIEW_RESOURCE_CHUNK_BYTES {
            return self.fail(
                ReviewResourceErrorCode::InvalidRange,
                format!(
                    "Review resource {} returned a chunk over the {}-byte bound.",
                    self.options.resource_id, REVIEW_RESOURCE_CHUNK_BYTES
                ),
            );
        }
        if chunk.content_size > 9_007_199_254_740_991 {
            return self.fail(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} declared an unusable content size.",
                    self.options.resource_id
                ),
            );
        }
        if !is_review_sha256_digest(&chunk.content_digest) {
            return self.fail(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} declared a digest outside the canonical form.",
                    self.options.resource_id
                ),
            );
        }
        match (self.content_size, self.content_digest.as_deref()) {
            (None, None) => {
                self.content_size = Some(chunk.content_size);
                self.content_digest = Some(chunk.content_digest.clone());
            }
            (Some(size), Some(digest))
                if size == chunk.content_size
                    && digest.eq_ignore_ascii_case(&chunk.content_digest) => {}
            _ => {
                return self.fail(
                    ReviewResourceErrorCode::ResourceIntegrity,
                    format!(
                        "Review resource {} changed its declared size or digest mid-stream.",
                        self.options.resource_id
                    ),
                );
            }
        }
        let content_size = self.content_size.expect("chunk established the size");
        if content_size > self.options.max_bytes {
            return self.fail(
                ReviewResourceErrorCode::ResourceTooLarge,
                format!(
                    "Review resource {} declares {} bytes, over the {}-byte limit for its kind.",
                    self.options.resource_id, content_size, self.options.max_bytes
                ),
            );
        }
        if self.offset.saturating_add(byte_length) > content_size {
            return self.fail(
                ReviewResourceErrorCode::InvalidRange,
                format!(
                    "Review resource {} returned more bytes than the {} it declares.",
                    self.options.resource_id, content_size
                ),
            );
        }
        if bytes.is_empty() && !chunk.eof {
            return self.fail(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} made no progress and did not end.",
                    self.options.resource_id
                ),
            );
        }
        if self.buffer.is_none() {
            let Ok(size) = usize::try_from(content_size) else {
                return self.fail(
                    ReviewResourceErrorCode::ResourceTooLarge,
                    format!(
                        "Review resource {} cannot fit in memory.",
                        self.options.resource_id
                    ),
                );
            };
            self.buffer = Some(vec![0; size]);
        }
        let start = usize::try_from(self.offset).expect("allocated buffer implies usize offsets");
        let end = start + bytes.len();
        self.buffer.as_mut().expect("buffer allocated")[start..end].copy_from_slice(bytes);
        self.offset += byte_length;
        if chunk.eof {
            if self.offset != content_size {
                return self.fail(
                    ReviewResourceErrorCode::ResourceIntegrity,
                    format!(
                        "Review resource {} ended at {} of {} bytes.",
                        self.options.resource_id, self.offset, content_size
                    ),
                );
            }
            self.complete = true;
        }
        ReviewAssemblyStep::Accepted {
            done: self.complete,
        }
    }

    #[must_use]
    pub fn finish(mut self) -> ReviewAssemblyResult {
        if let Some(failure) = self.failed {
            return ReviewAssemblyResult::Failed(failure);
        }
        let (Some(content_size), Some(content_digest)) =
            (self.content_size, self.content_digest.as_deref())
        else {
            return self.finish_failure();
        };
        if !self.complete {
            return self.finish_failure();
        }
        let bytes = self.buffer.take().unwrap_or_default();
        debug_assert_eq!(u64::try_from(bytes.len()).ok(), Some(content_size));
        if !(self.options.digest)(&bytes).eq_ignore_ascii_case(content_digest) {
            return ReviewAssemblyResult::Failed(review_resource_failure(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} does not hash to the digest it was served with.",
                    self.options.resource_id
                ),
            ));
        }
        ReviewAssemblyResult::Assembled { bytes }
    }

    fn finish_failure(&mut self) -> ReviewAssemblyResult {
        let failure = review_resource_failure(
            ReviewResourceErrorCode::ResourceIntegrity,
            format!(
                "Review resource {} was assembled before its stream ended.",
                self.options.resource_id
            ),
        );
        self.failed = Some(failure.clone());
        ReviewAssemblyResult::Failed(failure)
    }

    fn fail(&mut self, code: ReviewResourceErrorCode, message: String) -> ReviewAssemblyStep {
        let failure = self
            .failed
            .get_or_insert_with(|| review_resource_failure(code, message))
            .clone();
        ReviewAssemblyStep::Failed(failure)
    }
}

fn is_review_sha256_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATION: &str = "generation:p1:0";
    const RESOURCE_ID: &str = "resource:patch:file:abcdef";

    fn digest() -> String {
        "a".repeat(64)
    }

    fn chunk(offset: u64, byte_length: u64, content_size: u64, eof: bool) -> ReviewResourceChunk {
        ReviewResourceChunk {
            generation: GENERATION.into(),
            resource_id: RESOURCE_ID.into(),
            offset,
            byte_length,
            encoding: "base64".into(),
            data: String::new(),
            content_digest: digest(),
            content_size,
            eof,
        }
    }

    fn assembler(max_bytes: u64, expected: Option<ExpectedReviewResource>) -> ReviewChunkAssembler {
        ReviewChunkAssembler::new(ReviewChunkAssemblerOptions {
            resource_id: RESOURCE_ID.into(),
            generation: GENERATION.into(),
            digest: Arc::new(|_| digest()),
            max_bytes,
            expected,
        })
    }

    fn failure_code(step: ReviewAssemblyStep) -> ReviewResourceErrorCode {
        match step {
            ReviewAssemblyStep::Failed(failure) => failure.code,
            ReviewAssemblyStep::Accepted { .. } => panic!("expected failure"),
        }
    }

    #[test]
    fn assembles_sequential_chunks_directly_into_the_final_buffer() {
        let mut assembly = assembler(REVIEW_RESOURCE_CHUNK_BYTES * 8, None);
        assert_eq!(
            assembly.accept(&chunk(0, 3, 5, false), &[1, 2, 3]),
            ReviewAssemblyStep::Accepted { done: false }
        );
        assert_eq!(assembly.next_offset(), 3);
        assert_eq!(assembly.remaining_bytes(), Some(2));
        assert_eq!(
            assembly.accept(&chunk(3, 2, 5, true), &[4, 5]),
            ReviewAssemblyStep::Accepted { done: true }
        );
        assert_eq!(
            assembly.finish(),
            ReviewAssemblyResult::Assembled {
                bytes: vec![1, 2, 3, 4, 5]
            }
        );
    }

    #[test]
    fn accepts_only_the_eof_marked_empty_chunk_for_zero_length_resources() {
        let mut assembly = assembler(REVIEW_RESOURCE_CHUNK_BYTES * 8, None);
        assert_eq!(
            assembly.accept(&chunk(0, 0, 0, true), &[]),
            ReviewAssemblyStep::Accepted { done: true }
        );
        assert_eq!(
            assembly.finish(),
            ReviewAssemblyResult::Assembled { bytes: Vec::new() }
        );
        let mut stalled = assembler(REVIEW_RESOURCE_CHUNK_BYTES * 8, None);
        assert_eq!(
            failure_code(stalled.accept(&chunk(0, 0, 4, false), &[])),
            ReviewResourceErrorCode::ResourceIntegrity
        );
    }

    #[test]
    fn rejects_skipped_ranges_and_chunks_over_the_shared_bound() {
        let mut assembly = assembler(REVIEW_RESOURCE_CHUNK_BYTES * 8, None);
        assert_eq!(
            assembly.accept(&chunk(0, 2, 4, false), &[1, 2]),
            ReviewAssemblyStep::Accepted { done: false }
        );
        assert_eq!(
            failure_code(assembly.accept(&chunk(3, 1, 4, true), &[4])),
            ReviewResourceErrorCode::InvalidRange
        );
        let mut oversized = assembler(REVIEW_RESOURCE_CHUNK_BYTES * 8, None);
        let bytes = vec![0; usize::try_from(REVIEW_RESOURCE_CHUNK_BYTES + 1).unwrap()];
        assert_eq!(
            failure_code(oversized.accept(
                &chunk(
                    0,
                    REVIEW_RESOURCE_CHUNK_BYTES + 1,
                    REVIEW_RESOURCE_CHUNK_BYTES + 1,
                    true
                ),
                &bytes
            )),
            ReviewResourceErrorCode::InvalidRange
        );
    }

    #[test]
    fn rejects_changed_or_noncanonical_declarations_and_expected_mismatch() {
        let mut assembly = assembler(REVIEW_RESOURCE_CHUNK_BYTES * 8, None);
        let _ = assembly.accept(&chunk(0, 2, 4, false), &[1, 2]);
        let mut changed = chunk(2, 2, 4, true);
        changed.content_digest = "b".repeat(64);
        assert_eq!(
            failure_code(assembly.accept(&changed, &[3, 4])),
            ReviewResourceErrorCode::ResourceIntegrity
        );

        let mut invalid = assembler(REVIEW_RESOURCE_CHUNK_BYTES * 8, None);
        let mut invalid_chunk = chunk(0, 1, 1, true);
        invalid_chunk.content_digest = "A".repeat(64);
        assert_eq!(
            failure_code(invalid.accept(&invalid_chunk, &[1])),
            ReviewResourceErrorCode::ResourceIntegrity
        );

        let mut expected = assembler(
            REVIEW_RESOURCE_CHUNK_BYTES * 8,
            Some(ExpectedReviewResource {
                byte_length: 2,
                digest: digest(),
            }),
        );
        assert_eq!(expected.declared_size(), Some(2));
        assert_eq!(
            failure_code(expected.accept(&chunk(0, 3, 3, true), &[1, 2, 3])),
            ReviewResourceErrorCode::ResourceIntegrity
        );
    }

    #[test]
    fn refuses_declared_size_before_retaining_bytes_and_bytes_past_total() {
        let mut too_large = assembler(3, None);
        assert_eq!(
            failure_code(too_large.accept(&chunk(0, 2, 9, false), &[1, 2])),
            ReviewResourceErrorCode::ResourceTooLarge
        );
        let mut past_end = assembler(10, None);
        assert_eq!(
            failure_code(past_end.accept(&chunk(0, 2, 1, true), &[1, 2])),
            ReviewResourceErrorCode::InvalidRange
        );
    }

    #[test]
    fn recomputes_digest_and_rejects_unterminated_streams() {
        let mut bad_digest = ReviewChunkAssembler::new(ReviewChunkAssemblerOptions {
            resource_id: RESOURCE_ID.into(),
            generation: GENERATION.into(),
            digest: Arc::new(|_| "c".repeat(64)),
            max_bytes: 1024,
            expected: None,
        });
        let _ = bad_digest.accept(&chunk(0, 1, 1, true), &[7]);
        assert!(matches!(
            bad_digest.finish(),
            ReviewAssemblyResult::Failed(ReviewResourceFailure {
                code: ReviewResourceErrorCode::ResourceIntegrity,
                ..
            })
        ));
        let mut unfinished = assembler(1024, None);
        let _ = unfinished.accept(&chunk(0, 1, 4, false), &[1]);
        assert!(matches!(
            unfinished.finish(),
            ReviewAssemblyResult::Failed(ReviewResourceFailure {
                code: ReviewResourceErrorCode::ResourceIntegrity,
                ..
            })
        ));
    }

    #[test]
    fn rejects_routing_encoding_and_post_eof_chunks() {
        let mut routed = assembler(1024, None);
        let mut wrong = chunk(0, 1, 1, true);
        wrong.resource_id = "resource:patch:file:999999".into();
        assert_eq!(
            failure_code(routed.accept(&wrong, &[1])),
            ReviewResourceErrorCode::ResourceIntegrity
        );
        let mut encoding = assembler(1024, None);
        let mut wrong = chunk(0, 1, 1, true);
        wrong.encoding = "raw".into();
        assert_eq!(
            failure_code(encoding.accept(&wrong, &[1])),
            ReviewResourceErrorCode::ResourceIntegrity
        );
        let mut complete = assembler(1024, None);
        let done = chunk(0, 0, 0, true);
        let _ = complete.accept(&done, &[]);
        assert_eq!(
            failure_code(complete.accept(&done, &[])),
            ReviewResourceErrorCode::ResourceIntegrity
        );
    }

    #[test]
    fn first_failure_remains_authoritative() {
        let mut assembly = assembler(1, None);
        let first = assembly.accept(&chunk(0, 1, 5, false), &[1]);
        let expected = match first {
            ReviewAssemblyStep::Failed(failure) => failure,
            ReviewAssemblyStep::Accepted { .. } => panic!("expected failure"),
        };
        assert_eq!(
            assembly.accept(&chunk(0, 0, 0, true), &[]),
            ReviewAssemblyStep::Failed(expected.clone())
        );
        assert_eq!(assembly.finish(), ReviewAssemblyResult::Failed(expected));
    }
}
