//! Renderer- and provider-neutral review models.
//!
//! These value types are the semantic boundary shared by the VCS adapters, review state,
//! session protocol, extension host, and Ratatui renderer. They intentionally contain no
//! terminal widgets, Git handles, or process objects.

mod bootstrap;
mod command_inputs;
mod identity;
mod keybindings;
mod paths;
mod run;
mod run_errors;
mod semantic;
mod source_capability;
mod startup_notice;
mod theme;
mod view_preferences;

pub use bootstrap::*;
pub use command_inputs::*;
pub use identity::{review_content_digest, review_file_key, review_source_identity};
pub use keybindings::*;
pub use paths::*;
pub use run::*;
pub use run_errors::*;
pub use semantic::*;
pub use source_capability::*;
pub use startup_notice::*;
pub use theme::*;
pub use view_preferences::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum AgentContextError {
    #[error("agent context JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSide {
    Old,
    New,
}

/// Position of a collapsed unchanged-source gap relative to its owning hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewGapPosition {
    Before,
    Trailing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn contains(self, line: u32) -> bool {
        self.start <= line && line <= self.end
    }

    pub fn intersects(self, other: Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileChangeKind {
    Modified,
    Renamed,
    Added,
    Deleted,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    /// Git classified this addition/deletion as moved before terminal control was sanitized.
    #[serde(default)]
    pub moved: bool,
    #[serde(default)]
    pub no_newline_at_eof: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub index: usize,
    pub header: String,
    pub context: Option<String>,
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub split_row_start: usize,
    pub split_row_count: usize,
    pub stack_row_start: usize,
    pub stack_row_count: usize,
    pub lines: Vec<DiffLine>,
}

impl DiffHunk {
    pub fn formatted_header(&self) -> String {
        if !self.header.is_empty() {
            return self.header.clone();
        }
        let mut header = format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_count, self.new_start, self.new_count
        );
        if let Some(context) = self
            .context
            .as_deref()
            .filter(|context| !context.is_empty())
        {
            header.push(' ');
            header.push_str(context);
        }
        header
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStats {
    pub additions: usize,
    pub deletions: usize,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFlags {
    #[serde(default)]
    pub untracked: bool,
    #[serde(default)]
    pub binary: bool,
    #[serde(default)]
    pub too_large: bool,
    #[serde(default)]
    pub partial: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentAnnotationConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAnnotation {
    pub id: Option<String>,
    pub old_range: Option<LineRange>,
    pub new_range: Option<LineRange>,
    pub summary: String,
    pub rationale: Option<String>,
    pub markup: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub confidence: Option<AgentAnnotationConfidence>,
    pub source: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFileContext {
    pub path: String,
    pub summary: Option<String>,
    #[serde(default)]
    pub annotations: Vec<AgentAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentContext {
    pub version: u64,
    pub summary: Option<String>,
    pub files: Vec<AgentFileContext>,
}

impl AgentContext {
    pub fn from_json(source: &str) -> Result<Self, AgentContextError> {
        let value: Value = serde_json::from_str(source)?;
        let object = value.as_object().ok_or_else(|| {
            AgentContextError::Invalid("Agent context must be a JSON object.".into())
        })?;
        let version = object.get("version").and_then(Value::as_u64).unwrap_or(1);
        let summary = optional_string(object.get("summary"));
        let files = object
            .get("files")
            .and_then(Value::as_array)
            .map(|files| files.iter().map(parse_agent_file).collect())
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            version,
            summary,
            files,
        })
    }

    /// Attach annotations by current path first and previous path second, then order files to
    /// follow the sidecar's narrative while retaining the provider order for unmatched files.
    pub fn apply_to(&self, changeset: &mut Changeset) {
        changeset.agent_summary.clone_from(&self.summary);
        for file in &mut changeset.files {
            file.agent = self
                .files
                .iter()
                .find(|context| {
                    context.path == file.path
                        || file
                            .previous_path
                            .as_deref()
                            .is_some_and(|path| context.path == path)
                })
                .cloned();
        }
        changeset.files.sort_by_key(|file| {
            self.files
                .iter()
                .position(|context| {
                    context.path == file.path
                        || file
                            .previous_path
                            .as_deref()
                            .is_some_and(|path| context.path == path)
                })
                .unwrap_or(usize::MAX)
        });
    }
}

fn parse_agent_file(value: &Value) -> Result<AgentFileContext, AgentContextError> {
    let object = value
        .as_object()
        .ok_or_else(|| AgentContextError::Invalid("Agent context files must be objects.".into()))?;
    let path = optional_non_empty_string(object.get("path")).ok_or_else(|| {
        AgentContextError::Invalid("Agent context file entries require a non-empty path.".into())
    })?;
    let annotations = object
        .get("annotations")
        .and_then(Value::as_array)
        .map(|annotations| annotations.iter().map(parse_agent_annotation).collect())
        .transpose()?
        .unwrap_or_default();
    Ok(AgentFileContext {
        path,
        summary: optional_string(object.get("summary")),
        annotations,
    })
}

fn parse_agent_annotation(value: &Value) -> Result<AgentAnnotation, AgentContextError> {
    let object = value
        .as_object()
        .ok_or_else(|| AgentContextError::Invalid("Agent annotations must be objects.".into()))?;
    let summary = optional_non_empty_string(object.get("summary")).ok_or_else(|| {
        AgentContextError::Invalid("Each agent annotation requires a summary.".into())
    })?;
    Ok(AgentAnnotation {
        id: optional_string(object.get("id")),
        old_range: parse_agent_range(object.get("oldRange"))?,
        new_range: parse_agent_range(object.get("newRange"))?,
        summary,
        rationale: optional_string(object.get("rationale")),
        markup: optional_non_empty_string(object.get("markup")),
        tags: object
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        confidence: match object.get("confidence").and_then(Value::as_str) {
            Some("low") => Some(AgentAnnotationConfidence::Low),
            Some("medium") => Some(AgentAnnotationConfidence::Medium),
            Some("high") => Some(AgentAnnotationConfidence::High),
            _ => None,
        },
        source: optional_string(object.get("source")),
        title: optional_string(object.get("title")),
        author: optional_string(object.get("author")),
        created_at: optional_string(object.get("createdAt")),
        updated_at: optional_string(object.get("updatedAt")),
        editable: object
            .get("editable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_agent_range(value: Option<&Value>) -> Result<Option<LineRange>, AgentContextError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(range) = value.as_array() else {
        return Ok(None);
    };
    if range.len() != 2 {
        return Ok(None);
    }
    let Some(start) = range[0].as_u64() else {
        return Err(AgentContextError::Invalid(
            "Annotation ranges must be integer tuples.".into(),
        ));
    };
    let Some(end) = range[1].as_u64() else {
        return Err(AgentContextError::Invalid(
            "Annotation ranges must be integer tuples.".into(),
        ));
    };
    if start == 0 || end == 0 {
        return Err(AgentContextError::Invalid(
            "Annotation ranges must use positive 1-based line numbers.".into(),
        ));
    }
    if end < start {
        return Err(AgentContextError::Invalid(
            "Annotation ranges must be ordered start..end tuples.".into(),
        ));
    }
    let start = u32::try_from(start).map_err(|_| {
        AgentContextError::Invalid("Annotation ranges exceed supported line numbers.".into())
    })?;
    let end = u32::try_from(end).map_err(|_| {
        AgentContextError::Invalid("Annotation ranges exceed supported line numbers.".into())
    })?;
    Ok(Some(LineRange { start, end }))
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

fn optional_non_empty_string(value: Option<&Value>) -> Option<String> {
    optional_string(value).filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffFile {
    /// Stable address derived from provider-neutral file content.
    pub key: String,
    /// Invocation-local identity used by mounted UI state.
    pub runtime_id: String,
    pub path: String,
    pub previous_path: Option<String>,
    pub change_kind: FileChangeKind,
    pub language: Option<String>,
    pub stats: FileStats,
    pub flags: FileFlags,
    pub patch: String,
    pub split_row_count: usize,
    pub stack_row_count: usize,
    pub hunks: Vec<DiffHunk>,
    pub content_identity: String,
    #[serde(default)]
    pub sources: FileSourceSnapshots,
    /// Identity metadata only; deserialization does not grant source access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_capability: Option<SourceCapabilityIdentity>,
    pub source_identity: Option<String>,
    #[serde(default)]
    pub source_attested: bool,
    pub agent: Option<AgentFileContext>,
}

impl DiffFile {
    pub fn refresh_identity(&mut self) {
        self.content_identity = identity::review_file_content_identity(self);
        self.refresh_source_identity();
    }

    pub fn refresh_address(&mut self, source_label: &str, duplicate_index: usize) {
        self.key = review_file_key(
            source_label,
            &self.path,
            self.previous_path.as_deref(),
            duplicate_index,
        );
    }

    pub fn hunk_at_line(&self, side: ReviewSide, line: u32) -> Option<usize> {
        self.hunks.iter().position(|hunk| {
            let (start, count) = match side {
                ReviewSide::Old => (hunk.old_start, hunk.old_count),
                ReviewSide::New => (hunk.new_start, hunk.new_count),
            };
            count > 0 && line >= start && line < start.saturating_add(count)
        })
    }

    pub fn set_sources(&mut self, sources: FileSourceSnapshots) {
        let has_sources = sources.old.is_some() || sources.new.is_some();
        self.source_attested = has_sources
            && sources
                .old
                .iter()
                .chain(sources.new.iter())
                .all(|snapshot| snapshot.attested);
        self.sources = sources;
        self.refresh_source_identity();
    }

    fn refresh_source_identity(&mut self) {
        if let Some(capability) = &self.source_capability {
            self.source_identity =
                Some(capability.source_identity(&self.path, &self.content_identity));
            self.source_attested = capability.attested();
            return;
        }
        self.source_identity =
            self.sources
                .new
                .as_ref()
                .or(self.sources.old.as_ref())
                .map(|snapshot| {
                    review_source_identity(
                        &self.path,
                        &self.content_identity,
                        Some(&snapshot.content_identity),
                    )
                });
    }

    pub fn set_source_capability(&mut self, capability: Option<SourceCapabilityIdentity>) {
        self.source_capability = capability;
        self.source_attested = (self.sources.old.is_some() || self.sources.new.is_some())
            && self
                .sources
                .old
                .iter()
                .chain(self.sources.new.iter())
                .all(|source| source.attested);
        self.refresh_source_identity();
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSourceSnapshots {
    pub old: Option<SourceSnapshot>,
    pub new: Option<SourceSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub content: String,
    pub content_identity: String,
    pub origin: SourceOrigin,
    #[serde(default)]
    pub attested: bool,
}

impl SourceSnapshot {
    pub fn new(content: String, origin: SourceOrigin, attested: bool) -> Self {
        let content_identity = review_digest(content.as_bytes());
        Self {
            content,
            content_identity,
            origin,
            attested,
        }
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.content.lines()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SourceOrigin {
    WorkingTree,
    Index,
    Revision { revision: String },
    File { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changeset {
    pub id: String,
    #[serde(default)]
    pub source_label: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_summary: Option<String>,
    pub source: ChangesetSource,
    pub files: Vec<DiffFile>,
}

impl Changeset {
    /// Stable producer label used for review addresses and public projections.
    #[must_use]
    pub fn effective_source_label(&self) -> &str {
        if self.source_label.is_empty() {
            &self.id
        } else {
            &self.source_label
        }
    }

    /// Re-project content identities and stable review addresses after a provider or extension
    /// changes file facts. Duplicate paths remain separately addressable in producer order.
    pub fn refresh_review_identities(&mut self) {
        let source_label = self.effective_source_label().to_owned();
        let mut occurrences = std::collections::HashMap::<String, usize>::new();
        for file in &mut self.files {
            file.refresh_identity();
            let occurrence = occurrences.entry(file.path.clone()).or_default();
            file.refresh_address(&source_label, *occurrence);
            *occurrence += 1;
        }
    }

    pub fn stats(&self) -> FileStats {
        self.files
            .iter()
            .fold(FileStats::default(), |mut stats, file| {
                stats.additions += file.stats.additions;
                stats.deletions += file.stats.deletions;
                stats.truncated |= file.stats.truncated;
                stats
            })
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ChangesetSource {
    WorkingTree { staged: bool },
    Revision { from: Option<String>, to: String },
    Stash { reference: String },
    Patch { label: String },
    Files { left: String, right: String },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSelection {
    pub file_index: usize,
    pub hunk_index: Option<usize>,
    pub side: Option<ReviewSide>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSnapshot {
    pub generation: u64,
    pub changeset: Changeset,
    pub selection: ReviewSelection,
}

pub const REVIEW_DIGEST_ALGORITHM: &str = "sha256";

pub fn utf8_byte_length(value: &str) -> usize {
    value.len()
}

pub fn as_record(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value.as_object()
}

pub fn has_exact_keys(record: &serde_json::Map<String, Value>, allowed: &[&str]) -> bool {
    record.len() == allowed.len()
        && record
            .keys()
            .all(|key| allowed.iter().any(|allowed| key == allowed))
}

/// Canonical lowercase SHA-256 used for immutable review resources and semantic file keys.
pub fn review_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Hash serialized JSON directly, without retaining a second copy of the payload.
pub fn review_serialized_digest(value: &impl Serialize) -> Result<String, serde_json::Error> {
    struct DigestWriter(Sha256);
    impl std::io::Write for DigestWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = DigestWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value)?;
    Ok(format!("{:x}", writer.0.finalize()))
}

pub fn is_review_sha256_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

pub fn review_digests_equal(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> DiffFile {
        DiffFile {
            key: String::new(),
            runtime_id: "run:0".into(),
            path: "src/main.rs".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("rust".into()),
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
            split_row_count: 1,
            stack_row_count: 2,
            hunks: vec![],
            content_identity: String::new(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: None,
        }
    }

    #[test]
    fn content_identity_is_stable_and_content_sensitive() {
        let mut first = file();
        first.refresh_identity();
        let mut second = file();
        second.refresh_identity();
        assert_eq!(first.content_identity, second.content_identity);
        second.patch.push_str("+later\n");
        second.refresh_identity();
        assert_ne!(first.content_identity, second.content_identity);
        assert_eq!(first.content_identity.len(), 32);
        assert!(
            first
                .content_identity
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        );
    }

    #[test]
    fn streamed_json_digest_matches_exact_serialized_bytes() {
        for value in [
            serde_json::json!(null),
            serde_json::json!({"notes":["雪", "a".repeat(100_000)], "enabled":true}),
        ] {
            assert_eq!(
                review_serialized_digest(&value).unwrap(),
                review_digest(&serde_json::to_vec(&value).unwrap())
            );
        }
        let invalid = std::collections::BTreeMap::from([(vec![1, 2], "invalid JSON key")]);
        assert!(review_serialized_digest(&invalid).is_err());
    }

    #[test]
    fn review_digest_is_canonical_sha256() {
        assert_eq!(REVIEW_DIGEST_ALGORITHM, "sha256");
        assert_eq!(
            review_digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(is_review_sha256_digest(&review_digest(b"abc")));
        assert!(!is_review_sha256_digest(
            &review_digest(b"abc").to_uppercase()
        ));
        assert!(review_digests_equal("aa", "AA"));
    }

    #[test]
    fn review_digest_preserves_empty_and_multiblock_sha256_vectors() {
        assert_eq!(
            review_digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            review_digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(
            review_digest(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn review_boundary_validation_uses_utf8_bytes_and_exact_object_keys() {
        assert_eq!(utf8_byte_length(""), 0);
        assert_eq!(utf8_byte_length("abc"), 3);
        assert_eq!(utf8_byte_length("é"), 2);
        assert_eq!(utf8_byte_length("日"), 3);
        assert_eq!(utf8_byte_length("🧪"), 4);
        assert_eq!(utf8_byte_length("a🧪日é"), 10);
        // Rust strings cannot contain unpaired UTF-16 surrogates; the decoded replacement
        // character has the same three-byte boundary representation Hunk validates.
        assert_eq!(utf8_byte_length("�"), 3);

        let value = serde_json::json!({ "b": 2, "a": 1 });
        let record = as_record(&value).unwrap();
        assert!(has_exact_keys(record, &["a", "b"]));
        assert!(!has_exact_keys(record, &["a"]));
        assert!(!has_exact_keys(record, &["a", "b", "c"]));
        assert!(as_record(&serde_json::json!([])).is_none());
        assert!(as_record(&Value::Null).is_none());
    }

    #[test]
    fn source_snapshots_are_content_addressed_and_attestation_propagates() {
        let mut file = file();
        file.refresh_identity();
        let old = SourceSnapshot::new(
            "before\n".into(),
            SourceOrigin::Revision {
                revision: "HEAD".into(),
            },
            true,
        );
        let new = SourceSnapshot::new("after\n".into(), SourceOrigin::WorkingTree, false);
        assert_ne!(old.content_identity, new.content_identity);
        file.set_sources(FileSourceSnapshots {
            old: Some(old),
            new: Some(new.clone()),
        });
        assert_eq!(
            file.source_identity,
            Some(review_source_identity(
                &file.path,
                &file.content_identity,
                Some(&new.content_identity)
            ))
        );
        assert!(!file.source_attested);
    }

    #[test]
    fn file_capability_projection_matches_frozen_oracles_before_and_after_loading() {
        let oracle: Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/source-capability-identity.json"
        ))
        .unwrap();
        for run in oracle["runs"].as_array().unwrap() {
            for case in run["cases"].as_array().unwrap() {
                let input = &case["input"];
                let mut file = file();
                file.path = input["path"].as_str().unwrap_or("source.ts").into();
                file.runtime_id = input["runtimeId"].as_str().unwrap_or("one").into();
                file.language = Some("typescript".into());
                file.patch = input["patch"].as_str().unwrap_or("").into();
                file.split_row_count = 0;
                file.stack_row_count = 0;
                file.flags.partial = true;
                file.set_source_capability(input.get("capability").map(|capability| {
                    SourceCapabilityIdentity {
                        cache_key: capability["cacheKey"].as_str().map(str::to_owned),
                    }
                }));
                file.refresh_identity();
                assert_eq!(
                    file.content_identity, case["contentIdentity"],
                    "{}",
                    input["name"]
                );
                assert_eq!(
                    file.source_identity.as_deref(),
                    case["sourceIdentity"].as_str()
                );
                assert_eq!(
                    file.source_attested,
                    case["sourceAttested"].as_bool().unwrap_or(false)
                );
                let projected = project_review_file(&file, "source-capability", 0);
                assert_eq!(projected.content_identity, case["contentIdentity"]);
                assert_eq!(projected.source_identity, file.source_identity);
                assert_eq!(projected.source_attested, case["sourceAttested"].as_bool());
                let serialized = serde_json::to_value(&file).unwrap();
                assert_eq!(
                    serialized.get("source_capability").is_some(),
                    input.get("capability").is_some()
                );
                assert_eq!(
                    serde_json::from_value::<DiffFile>(serialized).unwrap(),
                    file
                );
                if file.source_capability.is_some() {
                    let identity = file.source_identity.clone();
                    let attested = file.source_attested;
                    for content in ["first", "replacement"] {
                        file.set_sources(FileSourceSnapshots {
                            old: None,
                            new: Some(SourceSnapshot::new(
                                content.into(),
                                SourceOrigin::WorkingTree,
                                !attested,
                            )),
                        });
                        file.refresh_identity();
                        assert_eq!(file.source_identity, identity);
                        assert_eq!(file.source_attested, attested);
                    }
                    file.set_source_capability(None);
                    assert_ne!(file.source_identity, identity);
                    assert_eq!(file.source_attested, !attested);
                    file.set_sources(FileSourceSnapshots::default());
                    assert_eq!(file.source_identity, None);
                    assert!(!file.source_attested);
                }
            }
        }
    }

    #[test]
    fn locates_hunks_by_old_and_new_line() {
        let mut file = file();
        file.hunks.push(DiffHunk {
            index: 0,
            header: "@@ -10,2 +20,3 @@".into(),
            context: None,
            old_start: 10,
            old_count: 2,
            new_start: 20,
            new_count: 3,
            split_row_start: 0,
            split_row_count: 3,
            stack_row_start: 0,
            stack_row_count: 4,
            lines: vec![],
        });
        assert_eq!(file.hunk_at_line(ReviewSide::Old, 11), Some(0));
        assert_eq!(file.hunk_at_line(ReviewSide::New, 22), Some(0));
        assert_eq!(file.hunk_at_line(ReviewSide::New, 23), None);
    }

    #[test]
    fn formats_synthetic_hunk_headers_from_per_side_counts() {
        let mut hunk = DiffHunk {
            index: 0,
            header: String::new(),
            context: None,
            old_start: 10,
            old_count: 4,
            new_start: 10,
            new_count: 4,
            split_row_start: 0,
            split_row_count: 0,
            stack_row_start: 0,
            stack_row_count: 0,
            lines: Vec::new(),
        };
        assert_eq!(hunk.formatted_header(), "@@ -10,4 +10,4 @@");
        hunk.old_start = 0;
        hunk.old_count = 0;
        hunk.new_start = 1;
        hunk.new_count = 3;
        assert_eq!(hunk.formatted_header(), "@@ -0,0 +1,3 @@");
        hunk.context = Some("function name()".into());
        assert_eq!(hunk.formatted_header(), "@@ -0,0 +1,3 @@ function name()");
    }

    #[test]
    fn agent_context_validates_matches_renames_and_orders_files() {
        let context = AgentContext::from_json(
            r#"{
              "version": 1,
              "summary": "Agent summary",
              "files": [{
                "path": "old.rs",
                "summary": "Explains the change",
                "annotations": [{
                  "newRange": [4, 8],
                  "summary": "Added a helper",
                  "confidence": "high",
                  "tags": ["review", 7]
                }]
              }]
            }"#,
        )
        .unwrap();
        let mut renamed = file();
        renamed.path = "new.rs".into();
        renamed.previous_path = Some("old.rs".into());
        let mut untouched = file();
        untouched.path = "unmatched.rs".into();
        let mut changeset = Changeset {
            id: "test".into(),
            source_label: "test".into(),
            title: "Test".into(),
            summary: None,
            agent_summary: None,
            source: ChangesetSource::Patch {
                label: "test".into(),
            },
            files: vec![untouched, renamed],
        };
        context.apply_to(&mut changeset);
        assert_eq!(changeset.agent_summary.as_deref(), Some("Agent summary"));
        assert_eq!(changeset.files[0].path, "new.rs");
        let annotation = &changeset.files[0].agent.as_ref().unwrap().annotations[0];
        assert_eq!(annotation.new_range, Some(LineRange { start: 4, end: 8 }));
        assert_eq!(annotation.tags, ["review"]);
        assert_eq!(annotation.confidence, Some(AgentAnnotationConfidence::High));
    }

    #[test]
    fn agent_context_rejects_invalid_ranges() {
        let error = AgentContext::from_json(
            r#"{"files":[{"path":"a.rs","annotations":[{"summary":"bad","newRange":[4,2]}]}]}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("ordered start..end"));
    }

    #[test]
    fn legacy_changesets_default_new_hunk_fields_and_keep_id_as_effective_label() {
        let changeset: Changeset = serde_json::from_value(serde_json::json!({
            "id": "legacy-id",
            "title": "Legacy",
            "source": { "kind": "patch", "label": "legacy" },
            "files": []
        }))
        .unwrap();

        assert_eq!(changeset.source_label, "");
        assert_eq!(changeset.summary, None);
        assert_eq!(changeset.agent_summary, None);
        assert_eq!(changeset.effective_source_label(), "legacy-id");
    }
}
