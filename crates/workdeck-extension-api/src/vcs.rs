//! Provider-neutral native extension VCS protocol.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Review operations a native VCS adapter can implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionVcsOperationKind {
    WorkingTreeDiff,
    RevisionShow,
    StashShow,
}

/// Callback set declared for one operation. `load` is implied by registration.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExtensionVcsOperationRegistration {
    pub watch_signature: bool,
    pub watch_plan: bool,
}

/// One native VCS adapter registered during the atomic handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtensionVcsAdapterRegistration {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub operations: BTreeMap<ExtensionVcsOperationKind, ExtensionVcsOperationRegistration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection_priority: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsDetectRequest {
    pub adapter_id: String,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsDetection {
    pub id: String,
    pub repo_root: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExtensionVcsReviewOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_untracked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_moved: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsRangeEndpoints {
    pub from: String,
    pub to: String,
}

/// Resolved review invocation passed to an extension adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExtensionVcsReviewInput {
    Vcs {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        range: Option<String>,
        #[serde(
            default,
            rename = "rangeEndpoints",
            skip_serializing_if = "Option::is_none"
        )]
        range_endpoints: Option<ExtensionVcsRangeEndpoints>,
        staged: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pathspecs: Vec<String>,
        options: ExtensionVcsReviewOptions,
    },
    Show {
        #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pathspecs: Vec<String>,
        options: ExtensionVcsReviewOptions,
    },
    StashShow {
        #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        options: ExtensionVcsReviewOptions,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsLoadContext {
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsOperationRequest {
    pub adapter_id: String,
    pub operation: ExtensionVcsOperationKind,
    pub input: ExtensionVcsReviewInput,
    pub context: ExtensionVcsLoadContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionVcsFileChangeType {
    Change,
    RenamePure,
    RenameChanged,
    New,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionVcsFileSide {
    Old,
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsFileStats {
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExtensionVcsExtraFile {
    Patch {
        path: String,
        #[serde(
            default,
            rename = "previousPath",
            skip_serializing_if = "Option::is_none"
        )]
        previous_path: Option<String>,
        #[serde(rename = "patchText")]
        patch_text: String,
        #[serde(default, rename = "isUntracked")]
        is_untracked: bool,
    },
    Skipped {
        path: String,
        #[serde(
            default,
            rename = "previousPath",
            skip_serializing_if = "Option::is_none"
        )]
        previous_path: Option<String>,
        reason: ExtensionVcsSkippedFileReason,
        #[serde(default, rename = "changeType")]
        change_type: Option<ExtensionVcsFileChangeType>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stats: Option<ExtensionVcsFileStats>,
        #[serde(default, rename = "statsTruncated")]
        stats_truncated: bool,
        #[serde(default, rename = "isUntracked")]
        is_untracked: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionVcsSkippedFileReason {
    TooLarge,
}

/// Public patch payload returned by a native VCS operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsPatchResult {
    pub repo_root: PathBuf,
    pub source_label: String,
    pub title: String,
    pub patch_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untracked_paths: Vec<PathBuf>,
    /// Whether `workdeck/vcs/source/read` is available for this load.
    #[serde(default)]
    pub read_file_source: bool,
    /// Opaque operation-owned source state handed back on subsequent reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_files: Vec<ExtensionVcsExtraFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsFileSourceRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub change_type: ExtensionVcsFileChangeType,
    pub is_untracked: bool,
    pub side: ExtensionVcsFileSide,
}

/// Native routing envelope around Hunk's public per-file source request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionVcsFileSourceInvocation {
    pub adapter_id: String,
    pub load_token: String,
    pub request: ExtensionVcsFileSourceRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionVcsFileSourceResult {
    Source(String),
    Missing,
    TooLarge { max_bytes: Option<usize> },
}

impl Serialize for ExtensionVcsFileSourceResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Source(text) => serializer.serialize_str(text),
            Self::Missing => serializer.serialize_none(),
            Self::TooLarge { max_bytes } => {
                #[derive(Serialize)]
                #[serde(rename_all = "camelCase")]
                struct TooLarge<'a> {
                    kind: &'static str,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    max_bytes: &'a Option<usize>,
                }
                TooLarge {
                    kind: "too-large",
                    max_bytes,
                }
                .serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for ExtensionVcsFileSourceResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(text) => Ok(Self::Source(text)),
            serde_json::Value::Null => Ok(Self::Missing),
            serde_json::Value::Object(object)
                if object.get("kind").and_then(serde_json::Value::as_str) == Some("too-large") =>
            {
                let max_bytes = object
                    .get("maxBytes")
                    .map(|value| {
                        value
                            .as_u64()
                            .and_then(|value| usize::try_from(value).ok())
                            .ok_or_else(|| {
                                serde::de::Error::custom("maxBytes must be an unsigned integer")
                            })
                    })
                    .transpose()?;
                Ok(Self::TooLarge { max_bytes })
            }
            _ => Err(serde::de::Error::custom(
                "VCS source readers must return text, null, or a too-large result",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionVcsWatchCoverage {
    Hybrid,
    PollOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionVcsWatchTargetSource {
    Content,
    Sidecar,
    Worktree,
    VcsMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExtensionVcsWatchTarget {
    DirectoryEntries {
        directory: PathBuf,
        entries: Vec<String>,
        sources: Vec<ExtensionVcsWatchTargetSource>,
    },
    DirectoryTree {
        directory: PathBuf,
        #[serde(default, rename = "ignoredRoots")]
        ignored_roots: Vec<PathBuf>,
        sources: Vec<ExtensionVcsWatchTargetSource>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionVcsWatchPlan {
    pub coverage: ExtensionVcsWatchCoverage,
    pub targets: Vec<ExtensionVcsWatchTarget>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_inputs_preserve_hunks_public_wire_spelling() {
        let input = ExtensionVcsReviewInput::Vcs {
            range: None,
            range_endpoints: Some(ExtensionVcsRangeEndpoints {
                from: "main".into(),
                to: "topic".into(),
            }),
            staged: true,
            pathspecs: vec!["src/lib.rs".into()],
            options: ExtensionVcsReviewOptions {
                exclude_untracked: Some(true),
                color_moved: Some(false),
            },
        };
        let value = serde_json::to_value(input).unwrap();
        assert_eq!(value["kind"], "vcs");
        assert_eq!(value["rangeEndpoints"]["from"], "main");
        assert_eq!(value["pathspecs"], serde_json::json!(["src/lib.rs"]));
        assert_eq!(value["options"]["excludeUntracked"], true);
    }

    #[test]
    fn patch_source_watch_and_extra_file_payloads_round_trip() {
        let result = ExtensionVcsPatchResult {
            repo_root: "/repo".into(),
            source_label: "repo".into(),
            title: "changes".into(),
            patch_text: "diff --git a/a b/a\n".into(),
            untracked_paths: vec!["new.rs".into()],
            read_file_source: true,
            load_token: Some("immutable-pair".into()),
            source_cache_key: Some("pair:1".into()),
            extra_files: vec![ExtensionVcsExtraFile::Skipped {
                path: "huge.bin".into(),
                previous_path: None,
                reason: ExtensionVcsSkippedFileReason::TooLarge,
                change_type: Some(ExtensionVcsFileChangeType::Change),
                stats: Some(ExtensionVcsFileStats {
                    additions: 4,
                    deletions: 2,
                }),
                stats_truncated: true,
                is_untracked: false,
            }],
        };
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(
            serde_json::from_value::<ExtensionVcsPatchResult>(value).unwrap(),
            result
        );

        let plan = ExtensionVcsWatchPlan {
            coverage: ExtensionVcsWatchCoverage::Hybrid,
            targets: vec![ExtensionVcsWatchTarget::DirectoryTree {
                directory: "/repo".into(),
                ignored_roots: vec!["target".into()],
                sources: vec![ExtensionVcsWatchTargetSource::Worktree],
            }],
        };
        assert_eq!(
            serde_json::from_value::<ExtensionVcsWatchPlan>(serde_json::to_value(&plan).unwrap())
                .unwrap(),
            plan
        );

        for source in [
            ExtensionVcsFileSourceResult::Source("hello".into()),
            ExtensionVcsFileSourceResult::Missing,
            ExtensionVcsFileSourceResult::TooLarge {
                max_bytes: Some(4096),
            },
        ] {
            assert_eq!(
                serde_json::from_value::<ExtensionVcsFileSourceResult>(
                    serde_json::to_value(&source).unwrap()
                )
                .unwrap(),
                source
            );
        }
    }
}
