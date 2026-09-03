//! Session-level extension startup, staged continuation, and load-failure notices.
//!
//! This is the native composition boundary corresponding to Hunk's
//! `src/extensions/startup.ts`. Discovery remains inert when extensions are disabled, an
//! unchanged candidate/config prefix can be extended without restarting its processes, and any
//! replaced pass is retired before its replacement starts.

use crate::{
    ExtensionLoadControl, ExtensionLoadIssue, ExtensionLoadResult, ExtensionLoadState, HostError,
    LoadExtensionsOptions, ManifestCandidate, TrustStore, discover_manifests_with_config,
    load_extensions,
};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use workdeck_core::StartupNotice;
use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};
use workdeck_extension_api::ExtensionNotificationHub;

/// Keep one extension failure on one startup footer row, matching Hunk's public limit.
pub const MAX_EXTENSION_ISSUE_MESSAGE_LENGTH: usize = 120;

/// Complete native inputs for one startup discovery/loading pass.
pub struct LoadStartupExtensionsOptions<'a> {
    pub enabled: bool,
    pub cwd: &'a Path,
    pub global_directory: Option<&'a Path>,
    pub repo_root: Option<&'a Path>,
    pub trust: &'a TrustStore,
    pub explicit_paths: &'a [PathBuf],
    pub user_config_paths: &'a [PathBuf],
    pub repo_config_paths: &'a [PathBuf],
    pub host_version: &'a str,
    pub extension_configs: &'a BTreeMap<String, serde_json::Value>,
    pub notifications: Option<ExtensionNotificationHub>,
    /// Provisional pass that may be extended when final discovery only appends candidates.
    pub previous_load: Option<ExtensionLoadResult>,
}

/// Produce the inert result used by disabled and candidate-free startup passes.
#[must_use]
pub fn create_empty_extension_load_result(
    cwd: impl Into<PathBuf>,
    notifications: ExtensionNotificationHub,
) -> ExtensionLoadResult {
    ExtensionLoadResult {
        extensions: Vec::new(),
        issues: Vec::new(),
        notifications,
        pending_trust_repo_root: None,
        load_state: ExtensionLoadState {
            cwd: cwd.into(),
            ..ExtensionLoadState::default()
        },
        control: ExtensionLoadControl::new(),
    }
}

/// Return whether final discovery can append safely to the completed provisional prefix.
fn can_extend_previous_load(
    previous: &ExtensionLoadResult,
    candidates: &[ManifestCandidate],
    extension_configs: &BTreeMap<String, serde_json::Value>,
    cwd: &Path,
) -> bool {
    let prior_candidates = &previous.load_state.candidates;
    if previous.load_state.cwd != cwd || prior_candidates.len() > candidates.len() {
        return false;
    }
    if !prior_candidates
        .iter()
        .zip(candidates)
        .all(|(prior, current)| prior == current)
    {
        return false;
    }

    previous.load_state.claimed_by.iter().all(|(id, path)| {
        !prior_candidates
            .iter()
            .any(|candidate| candidate.path == *path)
            || previous.load_state.extension_configs.get(id) == extension_configs.get(id)
    })
}

/// Discover and load native extensions for one interactive or delegated session.
pub fn load_startup_extensions(
    mut options: LoadStartupExtensionsOptions<'_>,
) -> Result<ExtensionLoadResult, HostError> {
    let notifications = options
        .notifications
        .take()
        .or_else(|| {
            options
                .previous_load
                .as_ref()
                .map(|previous| previous.notifications.clone())
        })
        .unwrap_or_default();

    // This branch intentionally precedes every filesystem and trust-store query.
    if !options.enabled {
        if let Some(mut previous) = options.previous_load.take() {
            previous.retire();
        }
        return Ok(create_empty_extension_load_result(
            options.cwd,
            notifications,
        ));
    }

    let discovery = discover_manifests_with_config(
        options.global_directory,
        options.repo_root,
        options.trust,
        options.explicit_paths,
        options.user_config_paths,
        options.repo_config_paths,
        options.cwd,
    )?;

    if discovery.candidates.is_empty() && discovery.pending_trust_repo_root.is_none() {
        if let Some(mut previous) = options.previous_load.take() {
            previous.retire();
        }
        return Ok(create_empty_extension_load_result(
            options.cwd,
            notifications,
        ));
    }

    let reuse_previous = options.previous_load.as_ref().is_some_and(|previous| {
        can_extend_previous_load(
            previous,
            &discovery.candidates,
            options.extension_configs,
            options.cwd,
        )
    });
    let previous_load = if reuse_previous {
        options.previous_load.take()
    } else {
        if let Some(mut previous) = options.previous_load.take() {
            previous.retire();
        }
        None
    };
    let completed_prefix = previous_load
        .as_ref()
        .map_or(0, |previous| previous.load_state.candidates.len());
    let candidates_to_load = &discovery.candidates[completed_prefix..];
    let mut result = load_extensions(LoadExtensionsOptions {
        candidates: candidates_to_load,
        all_candidates: Some(&discovery.candidates),
        previous_load,
        host_version: options.host_version,
        extension_configs: options.extension_configs,
        notifications: Some(notifications),
        pending_trust_repo_root: discovery.pending_trust_repo_root,
    });
    result.load_state.cwd = options.cwd.to_owned();
    Ok(result)
}

fn issue_extension_id(issue: &ExtensionLoadIssue) -> String {
    issue.extension_id.clone().unwrap_or_else(|| {
        issue
            .path
            .parent()
            .and_then(Path::file_name)
            .or_else(|| issue.path.file_stem())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("unknown")
            .to_owned()
    })
}

fn truncate_issue_message(message: &str) -> String {
    let sanitized = sanitize_terminal_text(message, SanitizeOptions::default());
    let single_line = sanitized.split('\n').next().unwrap_or_default().trim();
    if single_line.chars().count() <= MAX_EXTENSION_ISSUE_MESSAGE_LENGTH {
        return single_line.to_owned();
    }
    let mut truncated = single_line
        .chars()
        .take(MAX_EXTENSION_ISSUE_MESSAGE_LENGTH.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

/// Convert contained extension load failures into transient startup notices.
#[must_use]
pub fn create_extension_load_notices(issues: &[ExtensionLoadIssue]) -> Vec<StartupNotice> {
    issues
        .iter()
        .map(|issue| {
            StartupNotice::new(
                format!("extension:{}", issue.path.display()),
                format!(
                    "Extension {} failed to load • {}",
                    issue_extension_id(issue),
                    truncate_issue_message(&issue.message)
                ),
            )
        })
        .collect()
}

/// Append load failures while borrowing the original notice slice when there is nothing to add.
#[must_use]
pub fn merge_startup_notices<'a>(
    notices: Option<&'a [StartupNotice]>,
    extension_result: &ExtensionLoadResult,
) -> Option<Cow<'a, [StartupNotice]>> {
    let extension_notices = create_extension_load_notices(&extension_result.issues);
    if extension_notices.is_empty() {
        return notices.map(Cow::Borrowed);
    }
    let mut merged = notices.map_or_else(Vec::new, <[StartupNotice]>::to_vec);
    merged.extend(extension_notices);
    Some(Cow::Owned(merged))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExtensionEventBusPhase, ManifestOrigin};
    use tempfile::TempDir;

    fn options<'a>(
        cwd: &'a Path,
        explicit_paths: &'a [PathBuf],
        configs: &'a BTreeMap<String, serde_json::Value>,
        trust: &'a TrustStore,
    ) -> LoadStartupExtensionsOptions<'a> {
        LoadStartupExtensionsOptions {
            enabled: true,
            cwd,
            global_directory: None,
            repo_root: None,
            trust,
            explicit_paths,
            user_config_paths: &[],
            repo_config_paths: &[],
            host_version: "test",
            extension_configs: configs,
            notifications: None,
            previous_load: None,
        }
    }

    #[test]
    fn disabled_startup_returns_empty_without_reading_an_explicit_manifest() {
        let root = TempDir::new().unwrap();
        let explicit = [root.path().join("would-fail-if-read")];
        let configs = BTreeMap::new();
        let trust = TrustStore::default();
        let mut input = options(root.path(), &explicit, &configs, &trust);
        input.enabled = false;
        let result = load_startup_extensions(input).unwrap();
        assert!(result.extensions.is_empty());
        assert!(result.issues.is_empty());
        assert!(result.load_state.candidates.is_empty());
        assert_eq!(result.load_state.cwd, root.path());
    }

    #[test]
    fn changed_cwd_candidate_prefix_or_loaded_config_refuses_continuation() {
        let root = TempDir::new().unwrap();
        let candidate = ManifestCandidate {
            path: root.path().join("one/workdeck-extension.toml"),
            origin: ManifestOrigin::Global,
        };
        let notifications = ExtensionNotificationHub::new();
        let mut previous = create_empty_extension_load_result(root.path(), notifications);
        previous.load_state.candidates = vec![candidate.clone()];
        previous
            .load_state
            .claimed_by
            .insert("one".into(), candidate.path.clone());
        previous
            .load_state
            .extension_configs
            .insert("one".into(), serde_json::json!({"value": 1}));
        assert!(can_extend_previous_load(
            &previous,
            &[
                candidate.clone(),
                ManifestCandidate {
                    path: root.path().join("two/workdeck-extension.toml"),
                    origin: ManifestOrigin::Repository,
                }
            ],
            &BTreeMap::from([("one".into(), serde_json::json!({"value": 1}))]),
            root.path(),
        ));
        assert!(!can_extend_previous_load(
            &previous,
            std::slice::from_ref(&candidate),
            &BTreeMap::from([("one".into(), serde_json::json!({"value": 2}))]),
            root.path(),
        ));
        assert!(!can_extend_previous_load(
            &previous,
            std::slice::from_ref(&candidate),
            &BTreeMap::from([("one".into(), serde_json::json!({"value": 1}))]),
            &root.path().join("other"),
        ));
        let changed = ManifestCandidate {
            path: root.path().join("changed/workdeck-extension.toml"),
            origin: ManifestOrigin::Global,
        };
        assert!(!can_extend_previous_load(
            &previous,
            &[changed],
            &previous.load_state.extension_configs,
            root.path(),
        ));
        assert_eq!(previous.control.phase(), ExtensionEventBusPhase::Loading);
    }

    #[test]
    fn load_failures_become_sanitized_single_line_bounded_notices() {
        let issue = ExtensionLoadIssue {
            extension_id: Some("broken".into()),
            path: PathBuf::from("ext/broken/workdeck-extension.toml"),
            origin: ManifestOrigin::Repository,
            message: format!(
                "Cannot find '\u{1b}[2J\u{1b}]0;pwned\u{7}{}'\nstack",
                "x".repeat(150)
            ),
        };
        let notice = create_extension_load_notices(&[issue])
            .pop()
            .expect("one notice");
        assert_eq!(notice.key, "extension:ext/broken/workdeck-extension.toml");
        assert!(
            notice
                .message
                .starts_with("Extension broken failed to load • Cannot find '")
        );
        let detail = notice.message.split_once(" • ").unwrap().1;
        assert_eq!(detail.chars().count(), MAX_EXTENSION_ISSUE_MESSAGE_LENGTH);
        assert!(detail.ends_with('…'));
        assert!(!notice.message.contains('\u{1b}'));
        assert!(!notice.message.contains("stack"));
    }

    #[test]
    fn merge_borrows_original_notices_when_nothing_failed() {
        let notifications = ExtensionNotificationHub::new();
        let result = create_empty_extension_load_result(".", notifications);
        let notices = [StartupNotice::new("legacy", "legacy syntax")];
        let merged = merge_startup_notices(Some(&notices), &result).unwrap();
        assert!(matches!(merged, Cow::Borrowed(_)));
        assert!(std::ptr::eq(merged.as_ptr(), notices.as_ptr()));
        assert!(merge_startup_notices(None, &result).is_none());
    }

    #[test]
    fn merge_appends_failures_after_existing_config_notices() {
        let notifications = ExtensionNotificationHub::new();
        let mut result = create_empty_extension_load_result(".", notifications);
        result.issues.push(ExtensionLoadIssue {
            extension_id: Some("broken".into()),
            path: PathBuf::from("ext/broken/workdeck-extension.toml"),
            origin: ManifestOrigin::Global,
            message: "boom\nstack line".into(),
        });
        let notices = [StartupNotice::new("legacy", "legacy syntax")];
        let merged = merge_startup_notices(Some(&notices), &result).unwrap();
        assert!(matches!(merged, Cow::Owned(_)));
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], notices[0]);
        assert_eq!(merged[1].message, "Extension broken failed to load • boom");
    }

    #[test]
    fn frozen_hunk_startup_oracle_covers_both_pins_and_every_source_test() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../port/hunk/oracles/extension-startup.json");
        let oracle: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let baselines = oracle["baselines"].as_array().unwrap();
        assert_eq!(baselines.len(), 2);
        assert!(baselines.iter().all(|baseline| {
            baseline["source_blob"] == "c46d8e19baed706a4d2591f0c60be7f8a7dc48c7"
                && baseline["test_blob"] == "1e12af04c27c4157c7f83ffbacd8b170d86b8e74"
                && baseline["tests"] == 7
                && baseline["passed"] == 7
                && baseline["failed"] == 0
                && baseline["expect_calls"] == 16
        }));
        assert_eq!(oracle["test_mapping"].as_array().unwrap().len(), 7);
        assert!(
            oracle["test_mapping"]
                .as_array()
                .unwrap()
                .iter()
                .all(|mapping| {
                    mapping["source_test"]
                        .as_str()
                        .is_some_and(|name| !name.is_empty())
                        && mapping["rust_tests"]
                            .as_array()
                            .is_some_and(|tests| !tests.is_empty())
                })
        );
    }
}
