//! Adapt native extension registrations into the provider-neutral VCS catalog.

use crate::{
    ExtensionRegistrationResolution, HostError, LoadedExtension, NativeVcsDetectionNormalizer,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use workdeck_core::{
    DiffFile, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats, ReviewSide, SourceOrigin,
    SourceSnapshot,
};
use workdeck_diff::parse_single_file_patch;
use workdeck_extension_api::{
    DEFAULT_REQUEST_TIMEOUT_MS, ExtensionNotifyType, ExtensionVcsExtraFile,
    ExtensionVcsFileChangeType, ExtensionVcsFileSide, ExtensionVcsFileSourceRequest,
    ExtensionVcsFileSourceResult, ExtensionVcsOperationKind, ExtensionVcsOperationRequest,
    ExtensionVcsPatchResult, ExtensionVcsRangeEndpoints, ExtensionVcsReviewInput,
    ExtensionVcsReviewOptions, ExtensionVcsSkippedFileReason, ExtensionVcsWatchCoverage,
    ExtensionVcsWatchPlan, ExtensionVcsWatchTarget, ExtensionVcsWatchTargetSource, Registration,
};
use workdeck_vcs::{
    DEFAULT_SOURCE_TEXT_MAX_BYTES, VcsAdapter, VcsCatalogError, VcsDetection, VcsFileSourceRequest,
    VcsFileSourceResult, VcsLoadContext, VcsOperation, VcsOperations, VcsPatchResult,
    VcsReviewInput, VcsReviewOperationKind, VcsWatchCoverage, VcsWatchPlan, VcsWatchTarget,
    VcsWatchTargetSource,
};

const NATIVE_VCS_TIMEOUT: Duration = Duration::from_millis(DEFAULT_REQUEST_TIMEOUT_MS);

/// Convert every VCS declaration from successfully loaded native extensions in registration order.
#[must_use]
pub fn native_vcs_adapters(extensions: &[LoadedExtension]) -> Vec<VcsAdapter> {
    native_vcs_adapters_filtered(extensions, |_, _| true)
}

/// Convert only the VCS declarations accepted by the shared application resolver.
#[must_use]
pub fn resolved_native_vcs_adapters(
    extensions: &[LoadedExtension],
    resolution: &ExtensionRegistrationResolution,
) -> Vec<VcsAdapter> {
    native_vcs_adapters_filtered(extensions, |extension_index, registration_index| {
        resolution.accepts(extension_index, registration_index)
    })
}

fn native_vcs_adapters_filtered(
    extensions: &[LoadedExtension],
    mut accepts: impl FnMut(usize, usize) -> bool,
) -> Vec<VcsAdapter> {
    let mut adapters = Vec::new();
    for (extension_index, extension) in extensions.iter().enumerate() {
        for (registration_index, registration) in
            extension.handshake.registrations.iter().enumerate()
        {
            if !accepts(extension_index, registration_index) {
                continue;
            }
            if let Registration::VcsAdapter(adapter) = registration {
                adapters.push(native_vcs_adapter(extension.clone(), adapter.clone()));
            }
        }
    }
    adapters
}

fn native_vcs_adapter(
    extension: LoadedExtension,
    registration: workdeck_extension_api::ExtensionVcsAdapterRegistration,
) -> VcsAdapter {
    let adapter_id = registration.id.clone();
    let detect_extension = extension.clone();
    let detection_normalizer = Arc::new(Mutex::new(NativeVcsDetectionNormalizer::new(
        adapter_id.clone(),
    )));
    let detect_id = adapter_id.clone();
    let detect_normalizer = Arc::clone(&detection_normalizer);
    let detect = Arc::new(move |cwd: &Path| {
        let mut process = detect_extension.clone();
        let value = process
            .detect_vcs_adapter(&detect_id, cwd.to_owned(), NATIVE_VCS_TIMEOUT)
            .map_err(|error| error.to_string())?;
        let outcome = detect_normalizer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .normalize(&value);
        if let Some(returned_id) = outcome.mismatched_id {
            detect_extension.notifications().notify(
                format!(
                    "VCS adapter {detect_id:?} returned detection id {returned_id:?}; using the registered id"
                ),
                ExtensionNotifyType::Warning,
            );
        }
        Ok(outcome.detection.map(|detection| VcsDetection {
            id: detection.id,
            repo_root: detection.repo_root,
        }))
    });

    let operations = registration
        .operations
        .iter()
        .map(|(kind, callbacks)| {
            let host_kind = *kind;
            let core_kind = to_core_operation_kind(host_kind);
            let load_extension = extension.clone();
            let load_adapter_id = adapter_id.clone();
            let load = Arc::new(move |input: &VcsReviewInput, context: &VcsLoadContext| {
                let request =
                    extension_operation_request(&load_adapter_id, host_kind, input, context)?;
                let mut process = load_extension.clone();
                let result = process
                    .load_vcs_operation(
                        &request.adapter_id,
                        request.operation,
                        request.input,
                        request.context.cwd,
                        NATIVE_VCS_TIMEOUT,
                    )
                    .map_err(host_operation_error)?;
                convert_patch_result(load_extension.clone(), &load_adapter_id, result)
            });

            let watch_signature = callbacks.watch_signature.then(|| {
                let watch_extension = extension.clone();
                let watch_adapter_id = adapter_id.clone();
                Arc::new(move |input: &VcsReviewInput, context: &VcsLoadContext| {
                    let request =
                        extension_operation_request(&watch_adapter_id, host_kind, input, context)?;
                    watch_extension
                        .clone()
                        .vcs_watch_signature(request, NATIVE_VCS_TIMEOUT)
                        .map_err(host_operation_error)
                }) as _
            });

            let watch_plan = callbacks.watch_plan.then(|| {
                let watch_extension = extension.clone();
                let watch_adapter_id = adapter_id.clone();
                Arc::new(move |input: &VcsReviewInput, context: &VcsLoadContext| {
                    let request =
                        extension_operation_request(&watch_adapter_id, host_kind, input, context)?;
                    let plan = watch_extension
                        .clone()
                        .vcs_watch_plan(request, NATIVE_VCS_TIMEOUT)
                        .map_err(host_operation_error)?;
                    Ok(convert_watch_plan(plan))
                }) as _
            });

            (
                core_kind,
                VcsOperation {
                    load,
                    watch_signature,
                    watch_plan,
                },
            )
        })
        .collect::<VcsOperations>();

    VcsAdapter {
        id: adapter_id,
        name: registration.name,
        detect,
        operations,
        detection_priority: registration.detection_priority,
    }
}

fn extension_operation_request(
    adapter_id: &str,
    operation: ExtensionVcsOperationKind,
    input: &VcsReviewInput,
    context: &VcsLoadContext,
) -> Result<ExtensionVcsOperationRequest, VcsCatalogError> {
    let input = match input {
        VcsReviewInput::Diff(input) => ExtensionVcsReviewInput::Vcs {
            range: input.range.clone(),
            range_endpoints: input.range_endpoints.as_ref().map(|range| {
                ExtensionVcsRangeEndpoints {
                    from: range.from.clone(),
                    to: range.to.clone(),
                }
            }),
            staged: input.staged,
            pathspecs: input.pathspecs.clone(),
            options: extension_review_options(&input.options),
        },
        VcsReviewInput::Show(input) => ExtensionVcsReviewInput::Show {
            reference: input.reference.clone(),
            pathspecs: input.pathspecs.clone(),
            options: extension_review_options(&input.options),
        },
        VcsReviewInput::StashShow(input) => ExtensionVcsReviewInput::StashShow {
            reference: input.reference.clone(),
            options: extension_review_options(&input.options),
        },
    };
    let input_kind = match input {
        ExtensionVcsReviewInput::Vcs { .. } => ExtensionVcsOperationKind::WorkingTreeDiff,
        ExtensionVcsReviewInput::Show { .. } => ExtensionVcsOperationKind::RevisionShow,
        ExtensionVcsReviewInput::StashShow { .. } => ExtensionVcsOperationKind::StashShow,
    };
    if input_kind != operation {
        return Err(VcsCatalogError::Operation(format!(
            "native adapter input does not match {operation:?}"
        )));
    }
    Ok(ExtensionVcsOperationRequest {
        adapter_id: adapter_id.to_owned(),
        operation,
        input,
        context: workdeck_extension_api::ExtensionVcsLoadContext {
            cwd: context.cwd.clone(),
        },
    })
}

fn extension_review_options(options: &workdeck_core::CommonOptions) -> ExtensionVcsReviewOptions {
    ExtensionVcsReviewOptions {
        exclude_untracked: options.exclude_untracked,
        color_moved: options.color_moved,
    }
}

fn convert_patch_result(
    extension: LoadedExtension,
    adapter_id: &str,
    result: ExtensionVcsPatchResult,
) -> Result<VcsPatchResult, VcsCatalogError> {
    if result.repo_root.as_os_str().is_empty()
        || result.source_label.is_empty()
        || result.title.is_empty()
    {
        return Err(VcsCatalogError::Operation(
            "native VCS patch results require repoRoot, sourceLabel, and title".into(),
        ));
    }
    let source_reader = if result.read_file_source {
        let load_token = result
            .load_token
            .clone()
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                VcsCatalogError::Operation(
                    "native VCS source readers require a non-empty loadToken".into(),
                )
            })?;
        Some(native_source_reader(
            extension,
            adapter_id.to_owned(),
            load_token,
        ))
    } else {
        None
    };
    let extra_files = result
        .extra_files
        .into_iter()
        .enumerate()
        .map(|(index, entry)| convert_extra_file(entry, index, &result.source_label))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VcsPatchResult {
        repo_root: result.repo_root,
        source_label: result.source_label,
        title: result.title,
        patch_text: result.patch_text,
        untracked_paths: result.untracked_paths,
        source_reader,
        source_cache_key: result.source_cache_key,
        extra_files,
    })
}

fn native_source_reader(
    extension: LoadedExtension,
    adapter_id: String,
    load_token: String,
) -> workdeck_vcs::VcsSourceReader {
    let cache = Arc::new(Mutex::new(BTreeMap::<String, VcsFileSourceResult>::new()));
    Arc::new(move |request: &VcsFileSourceRequest| {
        let cache_key = format!(
            "{}\0{}\0{:?}\0{}\0{:?}",
            request.path,
            request.previous_path.as_deref().unwrap_or(""),
            request.change_kind,
            request.is_untracked,
            request.side
        );
        if let Some(result) = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(result);
        }
        let host_request = ExtensionVcsFileSourceRequest {
            path: request.path.clone(),
            previous_path: request.previous_path.clone(),
            change_type: extension_change_type(request.change_kind),
            is_untracked: request.is_untracked,
            side: match request.side {
                ReviewSide::Old => ExtensionVcsFileSide::Old,
                ReviewSide::New => ExtensionVcsFileSide::New,
            },
        };
        let result = match extension.clone().read_vcs_file_source(
            &adapter_id,
            &load_token,
            host_request,
            NATIVE_VCS_TIMEOUT,
        ) {
            Ok(ExtensionVcsFileSourceResult::Source(text)) => {
                VcsFileSourceResult::Source(SourceSnapshot::new(
                    text,
                    SourceOrigin::Revision {
                        revision: format!("extension:{adapter_id}:{:?}", request.side),
                    },
                    true,
                ))
            }
            Ok(ExtensionVcsFileSourceResult::Missing) => VcsFileSourceResult::Missing,
            Ok(ExtensionVcsFileSourceResult::TooLarge { max_bytes }) => {
                VcsFileSourceResult::TooLarge {
                    max_bytes: max_bytes
                        .filter(|max_bytes| *max_bytes > 0)
                        .unwrap_or(DEFAULT_SOURCE_TEXT_MAX_BYTES),
                }
            }
            Err(error) => return Err(host_operation_error(error)),
        };
        cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(cache_key, result.clone());
        Ok(result)
    })
}

fn convert_extra_file(
    entry: ExtensionVcsExtraFile,
    index: usize,
    source_label: &str,
) -> Result<DiffFile, VcsCatalogError> {
    match entry {
        ExtensionVcsExtraFile::Patch {
            path,
            previous_path,
            patch_text,
            is_untracked,
        } => {
            let mut file = parse_single_file_patch(&patch_text, &path, previous_path.as_deref())
                .map_err(|error| VcsCatalogError::Operation(error.to_string()))?;
            file.runtime_id = format!("{source_label}:{index}:{path}");
            file.flags.untracked = is_untracked;
            file.refresh_identity();
            file.refresh_address(source_label, index);
            Ok(file)
        }
        ExtensionVcsExtraFile::Skipped {
            path,
            previous_path,
            reason: ExtensionVcsSkippedFileReason::TooLarge,
            change_type,
            stats,
            stats_truncated,
            is_untracked,
        } => {
            let mut file = DiffFile {
                key: String::new(),
                runtime_id: format!("{source_label}:{index}:{path}"),
                path,
                previous_path,
                change_kind: core_change_kind(
                    change_type.unwrap_or(ExtensionVcsFileChangeType::Change),
                ),
                language: None,
                stats: stats.map_or_else(
                    || FileStats {
                        truncated: stats_truncated,
                        ..FileStats::default()
                    },
                    |stats| FileStats {
                        additions: stats.additions,
                        deletions: stats.deletions,
                        truncated: stats_truncated,
                    },
                ),
                flags: FileFlags {
                    untracked: is_untracked,
                    too_large: true,
                    ..FileFlags::default()
                },
                patch: String::new(),
                split_row_count: 0,
                stack_row_count: 0,
                hunks: Vec::new(),
                content_identity: String::new(),
                sources: FileSourceSnapshots::default(),
                source_identity: None,
                source_attested: false,
                agent: None,
            };
            file.refresh_identity();
            file.refresh_address(source_label, index);
            Ok(file)
        }
    }
}

const fn to_core_operation_kind(kind: ExtensionVcsOperationKind) -> VcsReviewOperationKind {
    match kind {
        ExtensionVcsOperationKind::WorkingTreeDiff => VcsReviewOperationKind::WorkingTreeDiff,
        ExtensionVcsOperationKind::RevisionShow => VcsReviewOperationKind::RevisionShow,
        ExtensionVcsOperationKind::StashShow => VcsReviewOperationKind::StashShow,
    }
}

const fn extension_change_type(kind: FileChangeKind) -> ExtensionVcsFileChangeType {
    match kind {
        FileChangeKind::Renamed => ExtensionVcsFileChangeType::RenameChanged,
        FileChangeKind::Added | FileChangeKind::Untracked => ExtensionVcsFileChangeType::New,
        FileChangeKind::Deleted => ExtensionVcsFileChangeType::Deleted,
        FileChangeKind::Modified
        | FileChangeKind::Copied
        | FileChangeKind::TypeChanged
        | FileChangeKind::Conflicted => ExtensionVcsFileChangeType::Change,
    }
}

const fn core_change_kind(kind: ExtensionVcsFileChangeType) -> FileChangeKind {
    match kind {
        ExtensionVcsFileChangeType::Change => FileChangeKind::Modified,
        ExtensionVcsFileChangeType::RenamePure | ExtensionVcsFileChangeType::RenameChanged => {
            FileChangeKind::Renamed
        }
        ExtensionVcsFileChangeType::New => FileChangeKind::Added,
        ExtensionVcsFileChangeType::Deleted => FileChangeKind::Deleted,
    }
}

fn convert_watch_plan(plan: ExtensionVcsWatchPlan) -> VcsWatchPlan {
    VcsWatchPlan {
        coverage: match plan.coverage {
            ExtensionVcsWatchCoverage::Hybrid => VcsWatchCoverage::Hybrid,
            ExtensionVcsWatchCoverage::PollOnly => VcsWatchCoverage::PollOnly,
        },
        targets: plan
            .targets
            .into_iter()
            .map(|target| match target {
                ExtensionVcsWatchTarget::DirectoryEntries {
                    directory,
                    entries,
                    sources,
                } => VcsWatchTarget::DirectoryEntries {
                    directory,
                    entries,
                    sources: sources.into_iter().map(convert_watch_source).collect(),
                },
                ExtensionVcsWatchTarget::DirectoryTree {
                    directory,
                    ignored_roots,
                    sources,
                } => VcsWatchTarget::DirectoryTree {
                    directory,
                    ignored_roots,
                    sources: sources.into_iter().map(convert_watch_source).collect(),
                },
            })
            .collect(),
    }
}

const fn convert_watch_source(source: ExtensionVcsWatchTargetSource) -> VcsWatchTargetSource {
    match source {
        ExtensionVcsWatchTargetSource::Content => VcsWatchTargetSource::Content,
        ExtensionVcsWatchTargetSource::Sidecar => VcsWatchTargetSource::Sidecar,
        ExtensionVcsWatchTargetSource::Worktree => VcsWatchTargetSource::Worktree,
        ExtensionVcsWatchTargetSource::VcsMetadata => VcsWatchTargetSource::VcsMetadata,
    }
}

fn host_operation_error(error: HostError) -> VcsCatalogError {
    VcsCatalogError::Operation(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use workdeck_core::{CommonOptions, VcsDiffCommandInput, VcsRangeEndpoints};

    #[test]
    fn operation_request_keeps_only_public_content_options() {
        let input = VcsReviewInput::Diff(VcsDiffCommandInput {
            range: None,
            range_endpoints: Some(VcsRangeEndpoints {
                from: "main".into(),
                to: "topic".into(),
            }),
            staged: true,
            pathspecs: vec!["src/lib.rs".into()],
            options: CommonOptions {
                exclude_untracked: Some(true),
                color_moved: Some(false),
                theme: Some("ignored".into()),
                ..CommonOptions::default()
            },
        });
        let request = extension_operation_request(
            "fossil",
            ExtensionVcsOperationKind::WorkingTreeDiff,
            &input,
            &VcsLoadContext {
                cwd: "/repo".into(),
            },
        )
        .unwrap();
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["input"]["rangeEndpoints"]["from"], "main");
        assert_eq!(value["input"]["options"]["excludeUntracked"], true);
        assert!(value["input"]["options"].get("theme").is_none());
    }

    #[test]
    fn watch_plan_deduplicates_sources_like_the_core_contract() {
        let plan = convert_watch_plan(ExtensionVcsWatchPlan {
            coverage: ExtensionVcsWatchCoverage::Hybrid,
            targets: vec![ExtensionVcsWatchTarget::DirectoryEntries {
                directory: "/repo/.fossil".into(),
                entries: vec!["checkout".into()],
                sources: vec![
                    ExtensionVcsWatchTargetSource::VcsMetadata,
                    ExtensionVcsWatchTargetSource::VcsMetadata,
                ],
            }],
        });
        let VcsWatchTarget::DirectoryEntries { sources, .. } = &plan.targets[0] else {
            panic!("expected entries target");
        };
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn extra_patch_and_skipped_files_retain_declared_metadata() {
        let patch = concat!(
            "diff --git a/old.txt b/new.txt\n",
            "--- a/old.txt\n",
            "+++ b/new.txt\n",
            "@@ -1 +1 @@\n",
            "-old\n",
            "+new\n"
        );
        let file = convert_extra_file(
            ExtensionVcsExtraFile::Patch {
                path: "new.txt".into(),
                previous_path: Some("old.txt".into()),
                patch_text: patch.into(),
                is_untracked: true,
            },
            0,
            "repo",
        )
        .unwrap();
        assert_eq!(file.previous_path.as_deref(), Some("old.txt"));
        assert!(file.flags.untracked);
        assert_eq!(file.runtime_id, "repo:0:new.txt");

        let skipped = convert_extra_file(
            ExtensionVcsExtraFile::Skipped {
                path: "huge.dat".into(),
                previous_path: None,
                reason: ExtensionVcsSkippedFileReason::TooLarge,
                change_type: None,
                stats: None,
                stats_truncated: true,
                is_untracked: false,
            },
            1,
            "repo",
        )
        .unwrap();
        assert!(skipped.flags.too_large);
        assert_eq!(skipped.change_kind, FileChangeKind::Modified);
        assert_eq!(skipped.runtime_id, "repo:1:huge.dat");
    }

    #[test]
    fn frozen_vcs_patch_result_oracle_maps_every_test_at_both_pins() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-vcs-patch-result.json"
        ))
        .unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        for baseline in baselines {
            assert_eq!(
                baseline["source_blob"],
                "5406973581d998188a55a8456ced7ad1ccc674fc"
            );
            assert_eq!(
                baseline["test_blob"],
                "fa22cc56a0ff7930c2053d13fcbb6230373dc7e3"
            );
            assert_eq!(baseline["tests"], 14);
            assert_eq!(baseline["passed"], 14);
            assert_eq!(baseline["failed"], 0);
            assert_eq!(baseline["expect_calls"], 39);
        }
        let mappings = oracle["test_mapping"].as_array().unwrap();
        assert_eq!(mappings.len(), 14);
        let names = mappings
            .iter()
            .map(|mapping| mapping["source_test"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), 14);
        assert!(mappings.iter().all(|mapping| {
            mapping["rust_tests"]
                .as_array()
                .is_some_and(|tests| !tests.is_empty())
        }));
    }
}
