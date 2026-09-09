//! Runtime-independent launch policies shared by CLI, sessions, and extensions.

use std::borrow::Cow;

use crate::DiffFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewInputKind {
    VersionControl,
    Files,
    Patch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadableReviewInput {
    pub kind: ReviewInputKind,
    pub input_file: Option<String>,
    pub agent_context_file: Option<String>,
}

/// Whether an input can be rebuilt without consuming standard input again.
pub fn can_reload_input(input: &ReloadableReviewInput) -> bool {
    if input.agent_context_file.as_deref() == Some("-") {
        return false;
    }
    input.kind != ReviewInputKind::Patch
        || input.input_file.as_deref().is_some_and(|file| file != "-")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperimentalFeature {
    Stml,
}

pub const EXPERIMENTAL_FEATURES: &[ExperimentalFeature] = &[ExperimentalFeature::Stml];

pub fn resolve_experimental_features(enabled: bool) -> &'static [ExperimentalFeature] {
    if enabled { EXPERIMENTAL_FEATURES } else { &[] }
}

pub fn experimental_feature_enabled(enabled: bool, feature: ExperimentalFeature) -> bool {
    enabled && EXPERIMENTAL_FEATURES.contains(&feature)
}

/// Strip disabled structured annotation bodies while keeping their text fallbacks.
/// The borrowed result preserves identity when no projection is required.
pub fn resolve_experimental_diff_files(
    files: &[DiffFile],
    experimental: bool,
) -> Cow<'_, [DiffFile]> {
    if experimental_feature_enabled(experimental, ExperimentalFeature::Stml) {
        return Cow::Borrowed(files);
    }
    Cow::Owned(
        files
            .iter()
            .cloned()
            .map(|mut file| {
                if let Some(agent) = &mut file.agent {
                    for annotation in &mut agent.annotations {
                        annotation.markup = None;
                    }
                }
                file
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentAnnotation, AgentFileContext, FileChangeKind, FileFlags, FileSourceSnapshots,
        FileStats,
    };

    #[test]
    fn stdin_patch_and_agent_context_inputs_cannot_reload() {
        let vcs = ReloadableReviewInput {
            kind: ReviewInputKind::VersionControl,
            input_file: None,
            agent_context_file: None,
        };
        assert!(can_reload_input(&vcs));
        assert!(!can_reload_input(&ReloadableReviewInput {
            kind: ReviewInputKind::Patch,
            input_file: None,
            agent_context_file: None,
        }));
        assert!(!can_reload_input(&ReloadableReviewInput {
            kind: ReviewInputKind::Patch,
            input_file: Some("-".into()),
            agent_context_file: None,
        }));
        assert!(can_reload_input(&ReloadableReviewInput {
            kind: ReviewInputKind::Patch,
            input_file: Some("change.patch".into()),
            agent_context_file: None,
        }));
        assert!(!can_reload_input(&ReloadableReviewInput {
            agent_context_file: Some("-".into()),
            ..vcs
        }));
    }

    fn annotated_file() -> DiffFile {
        DiffFile {
            key: "file:key".into(),
            runtime_id: "runtime".into(),
            path: "example.rs".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("rust".into()),
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: Vec::new(),
            content_identity: "content".into(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: Some(AgentFileContext {
                path: "example.rs".into(),
                summary: None,
                annotations: vec![AgentAnnotation {
                    extra: Default::default(),
                    id: None,
                    old_range: None,
                    new_range: None,
                    summary: "Updated the answer.".into(),
                    rationale: None,
                    markup: Some("<badge>42</badge>".into()),
                    tags: Vec::new(),
                    confidence: None,
                    source: None,
                    title: None,
                    author: None,
                    created_at: None,
                    updated_at: None,
                    editable: false,
                }],
            }),
        }
    }

    #[test]
    fn experimental_projection_preserves_fallbacks_without_mutating_input() {
        let files = [annotated_file()];
        let resolved = resolve_experimental_diff_files(&files, false);
        assert_eq!(
            resolved[0].agent.as_ref().unwrap().annotations[0].summary,
            "Updated the answer."
        );
        assert_eq!(
            resolved[0].agent.as_ref().unwrap().annotations[0].markup,
            None
        );
        assert_eq!(
            files[0].agent.as_ref().unwrap().annotations[0]
                .markup
                .as_deref(),
            Some("<badge>42</badge>")
        );

        let enabled = resolve_experimental_diff_files(&files, true);
        assert!(matches!(enabled, Cow::Borrowed(_)));
        assert_eq!(
            resolve_experimental_features(true),
            [ExperimentalFeature::Stml]
        );
        assert!(resolve_experimental_features(false).is_empty());
    }
}
