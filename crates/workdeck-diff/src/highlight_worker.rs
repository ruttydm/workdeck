//! Native state machine for Hunk's serialized syntax-highlight worker protocol.
//!
//! Rust builds do not spawn Bun or a JavaScript worker. The same request ordering, protocol
//! identity, stale-response handling, replacement, failure, and disposal semantics are retained
//! around the in-process highlighter and are ready for a native thread-backed executor.

use std::collections::VecDeque;

use workdeck_core::SemanticReviewFile;

use crate::{CompactHighlightedDiff, HighlightAppearance};

pub const HIGHLIGHT_WORKER_PROTOCOL_VERSION: u8 = 3;
pub const HIGHLIGHT_TOKENIZE_MAX_LINE_LENGTH_UTF16: usize = 1_000;
pub const HIGHLIGHT_WORD_DIFF_MAX_LINE_LENGTH_UTF16: usize = 10_000;
const BUILTIN_NATIVE_BACKEND_ID: u64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightWorkerRenderOptions {
    pub use_token_transformer: bool,
    pub tokenize_max_line_length_utf16: usize,
    pub line_diff_type: &'static str,
    pub max_line_diff_length_utf16: usize,
}

/// Fixed render behavior shared by Hunk's main-thread and worker highlighters.
#[must_use]
pub const fn highlight_worker_render_options() -> HighlightWorkerRenderOptions {
    HighlightWorkerRenderOptions {
        use_token_transformer: false,
        tokenize_max_line_length_utf16: HIGHLIGHT_TOKENIZE_MAX_LINE_LENGTH_UTF16,
        line_diff_type: "word-alt",
        max_line_diff_length_utf16: HIGHLIGHT_WORD_DIFF_MAX_LINE_LENGTH_UTF16,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightWorkerInput {
    pub alias_context: bool,
    pub metadata: SemanticReviewFile,
    pub appearance: HighlightAppearance,
    pub language: String,
    pub theme: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightWorkerRequest {
    pub version: u8,
    pub id: u64,
    pub alias_context: bool,
    pub metadata: SemanticReviewFile,
    pub appearance: HighlightAppearance,
    pub language: String,
    pub theme: String,
}

impl HighlightWorkerRequest {
    /// Build the endpoint's structured rejection for an unsupported request version.
    #[must_use]
    pub fn unsupported_version_response(&self) -> Option<HighlightWorkerResponse> {
        (self.version != HIGHLIGHT_WORKER_PROTOCOL_VERSION).then(|| {
            HighlightWorkerResponse::Failure {
                version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                id: self.id,
                message: format!(
                    "Unsupported highlight worker protocol version: {}",
                    self.version
                ),
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightWorkerResponse {
    Success {
        version: u8,
        id: u64,
        code: Box<CompactHighlightedDiff>,
    },
    Failure {
        version: u8,
        id: u64,
        message: String,
    },
}

impl HighlightWorkerResponse {
    const fn version(&self) -> u8 {
        match self {
            Self::Success { version, .. } | Self::Failure { version, .. } => *version,
        }
    }

    const fn id(&self) -> u64 {
        match self {
            Self::Success { id, .. } | Self::Failure { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightWorkerSettlement {
    pub request_id: u64,
    pub result: Result<CompactHighlightedDiff, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightWorkerMessageOutcome {
    pub settlement: HighlightWorkerSettlement,
    /// The next serialized request to post after settling the active request.
    pub next_request: Option<HighlightWorkerRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightWorkerReset {
    pub message: String,
    pub rejected_request_ids: Vec<u64>,
}

/// Queue and lifecycle state shared by the native in-process highlighter boundary.
#[derive(Debug)]
pub struct HighlightWorkerClient {
    backend_id: Option<u64>,
    active_request: Option<HighlightWorkerRequest>,
    queued_requests: VecDeque<HighlightWorkerRequest>,
    next_request_id: u64,
}

impl Default for HighlightWorkerClient {
    fn default() -> Self {
        Self {
            backend_id: None,
            active_request: None,
            queued_requests: VecDeque::new(),
            next_request_id: 1,
        }
    }
}

impl HighlightWorkerClient {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Native binaries support the in-process highlighter on every target, including Windows.
    #[must_use]
    pub const fn supports_offload() -> bool {
        true
    }

    /// Lazily register the compiled Rust backend used by production highlighting.
    pub(crate) fn ensure_builtin_backend(&mut self) {
        if self.backend_id.is_none() {
            self.backend_id = Some(BUILTIN_NATIVE_BACKEND_ID);
        }
    }

    /// Replace a backend and reject all work belonging to the previous instance.
    pub fn register_backend(&mut self, backend_id: u64) -> Option<HighlightWorkerReset> {
        let reset = self
            .backend_id
            .filter(|current| *current != backend_id)
            .map(|_| self.reset("The syntax highlighting worker was replaced."));
        self.backend_id = Some(backend_id);
        reset
    }

    /// Queue one caller request. Only `dispatch_next` can make it active.
    pub fn enqueue(&mut self, input: HighlightWorkerInput) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.queued_requests.push_back(HighlightWorkerRequest {
            version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
            id,
            alias_context: input.alias_context,
            metadata: input.metadata,
            appearance: input.appearance,
            language: input.language,
            theme: input.theme,
        });
        id
    }

    /// Start only the first queued request and keep later jobs serialized behind it.
    pub fn dispatch_next(&mut self) -> Option<HighlightWorkerRequest> {
        if self.active_request.is_some() {
            return None;
        }
        let request = self.queued_requests.pop_front()?;
        self.active_request = Some(request.clone());
        Some(request)
    }

    /// Ignore stale/wrong-version replies; otherwise settle the active job and advance the queue.
    pub fn handle_response(
        &mut self,
        response: HighlightWorkerResponse,
    ) -> Option<HighlightWorkerMessageOutcome> {
        let active = self.active_request.as_ref()?;
        if response.version() != HIGHLIGHT_WORKER_PROTOCOL_VERSION || response.id() != active.id {
            return None;
        }
        let request_id = active.id;
        self.active_request = None;
        let result = match response {
            HighlightWorkerResponse::Success { code, .. } => Ok(*code),
            HighlightWorkerResponse::Failure { message, .. } => Err(message),
        };
        let next_request = self.dispatch_next();
        Some(HighlightWorkerMessageOutcome {
            settlement: HighlightWorkerSettlement { request_id, result },
            next_request,
        })
    }

    /// Fail active and queued work after a posting or runtime error.
    pub fn fail(&mut self, message: impl Into<String>) -> HighlightWorkerReset {
        self.backend_id = None;
        self.reset(message.into())
    }

    /// Terminate the logical backend and reject every outstanding request.
    pub fn dispose(&mut self) -> HighlightWorkerReset {
        self.backend_id = None;
        self.reset("The syntax highlighting worker was disposed.")
    }

    fn reset(&mut self, message: impl Into<String>) -> HighlightWorkerReset {
        let mut rejected_request_ids = Vec::with_capacity(
            usize::from(self.active_request.is_some()) + self.queued_requests.len(),
        );
        if let Some(active) = self.active_request.take() {
            rejected_request_ids.push(active.id);
        }
        rejected_request_ids.extend(self.queued_requests.drain(..).map(|request| request.id));
        HighlightWorkerReset {
            message: message.into(),
            rejected_request_ids,
        }
    }

    #[cfg(test)]
    fn active_id(&self) -> Option<u64> {
        self.active_request.as_ref().map(|request| request.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{COMPACT_HIGHLIGHT_PROTOCOL_VERSION, CompactHighlightSide};
    use workdeck_core::{ChangesetSource, SemanticReviewFile, project_review_file};

    fn metadata() -> SemanticReviewFile {
        let file = crate::parse_patch(
            "diff --git a/example.ts b/example.ts\n--- a/example.ts\n+++ b/example.ts\n@@ -1 +1 @@\n-old\n+new\n",
            "worker",
            "worker",
            ChangesetSource::Patch {
                label: "worker".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        project_review_file(&file, &file.key, 0)
    }

    fn input(alias_context: bool) -> HighlightWorkerInput {
        HighlightWorkerInput {
            alias_context,
            metadata: metadata(),
            appearance: HighlightAppearance::Dark,
            language: "typescript".into(),
            theme: "github-dark-default".into(),
        }
    }

    fn empty_compact_response() -> CompactHighlightedDiff {
        CompactHighlightedDiff {
            version: COMPACT_HIGHLIGHT_PROTOCOL_VERSION,
            foreground_palette: Vec::new(),
            deletion: CompactHighlightSide::default(),
            addition: CompactHighlightSide::default(),
        }
    }

    #[test]
    fn native_backend_is_available_on_every_compiled_target() {
        assert!(HighlightWorkerClient::supports_offload());
    }

    #[test]
    fn endpoint_uses_fixed_pierre_limits_and_rejects_old_protocol_requests() {
        assert_eq!(
            highlight_worker_render_options(),
            HighlightWorkerRenderOptions {
                use_token_transformer: false,
                tokenize_max_line_length_utf16: 1_000,
                line_diff_type: "word-alt",
                max_line_diff_length_utf16: 10_000,
            }
        );
        let mut client = HighlightWorkerClient::new();
        let id = client.enqueue(input(false));
        let mut request = client.dispatch_next().unwrap();
        request.version = 2;
        assert_eq!(
            request.unsupported_version_response(),
            Some(HighlightWorkerResponse::Failure {
                version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                id,
                message: "Unsupported highlight worker protocol version: 2".into(),
            })
        );
    }

    #[test]
    fn serializes_requests_ignores_stale_replies_and_propagates_results() {
        let mut client = HighlightWorkerClient::new();
        client.register_backend(7);
        let first = client.enqueue(input(true));
        let second = client.enqueue(input(false));
        let request = client.dispatch_next().unwrap();
        assert_eq!(request.id, first);
        assert!(request.alias_context);
        assert!(client.dispatch_next().is_none());

        assert!(
            client
                .handle_response(HighlightWorkerResponse::Success {
                    version: 2,
                    id: first,
                    code: Box::new(empty_compact_response()),
                })
                .is_none()
        );
        assert_eq!(client.active_id(), Some(first));
        assert!(
            client
                .handle_response(HighlightWorkerResponse::Success {
                    version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                    id: second,
                    code: Box::new(empty_compact_response()),
                })
                .is_none()
        );
        assert_eq!(client.active_id(), Some(first));

        let first_outcome = client
            .handle_response(HighlightWorkerResponse::Success {
                version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                id: first,
                code: Box::new(empty_compact_response()),
            })
            .unwrap();
        assert_eq!(first_outcome.settlement.request_id, first);
        assert_eq!(
            first_outcome.settlement.result,
            Ok(empty_compact_response())
        );
        assert_eq!(
            first_outcome.next_request.as_ref().map(|job| job.id),
            Some(second)
        );

        let second_outcome = client
            .handle_response(HighlightWorkerResponse::Failure {
                version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                id: second,
                message: "highlight rejected".into(),
            })
            .unwrap();
        assert_eq!(
            second_outcome.settlement.result,
            Err("highlight rejected".into())
        );
    }

    #[test]
    fn replacement_rejects_active_and_queued_work() {
        let mut client = HighlightWorkerClient::new();
        client.register_backend(7);
        let active = client.enqueue(input(false));
        let queued = client.enqueue(input(false));
        client.dispatch_next();

        let reset = client.register_backend(8).unwrap();
        assert!(reset.message.contains("replaced"));
        assert_eq!(reset.rejected_request_ids, [active, queued]);
        assert!(client.dispatch_next().is_none());
    }

    #[test]
    fn posting_failure_rejects_all_work_and_later_backend_recovers() {
        let mut client = HighlightWorkerClient::new();
        client.register_backend(7);
        let active = client.enqueue(input(false));
        let queued = client.enqueue(input(false));
        client.dispatch_next();
        let reset = client.fail("post failed");
        assert_eq!(reset.message, "post failed");
        assert_eq!(reset.rejected_request_ids, [active, queued]);

        client.register_backend(8);
        let recovered = client.enqueue(input(false));
        assert_eq!(client.dispatch_next().unwrap().id, recovered);
        let outcome = client
            .handle_response(HighlightWorkerResponse::Success {
                version: HIGHLIGHT_WORKER_PROTOCOL_VERSION,
                id: recovered,
                code: Box::new(empty_compact_response()),
            })
            .unwrap();
        assert!(outcome.settlement.result.is_ok());
    }

    #[test]
    fn disposal_rejects_active_and_queued_work() {
        let mut client = HighlightWorkerClient::new();
        client.register_backend(7);
        let active = client.enqueue(input(false));
        let queued = client.enqueue(input(false));
        client.dispatch_next();
        let reset = client.dispose();
        assert!(reset.message.contains("disposed"));
        assert_eq!(reset.rejected_request_ids, [active, queued]);
    }
}
