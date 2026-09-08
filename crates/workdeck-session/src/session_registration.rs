//! Broker registration and initial snapshot projection for one published review.

use std::{collections::BTreeMap, io::IsTerminal, process::Command};

use chrono::{SecondsFormat, Utc};
use thiserror::Error;
use workdeck_core::{Changeset, SemanticReviewHunk};
use workdeck_review::{ReviewPublication, ReviewPublicationAddress};

use crate::{
    BrokerCryptoError, SESSION_BROKER_REGISTRATION_VERSION, SessionReviewFile, SessionReviewHunk,
    WorkdeckExperimentalFeature, WorkdeckReviewResourceCatalogV1, WorkdeckSessionInfo,
    WorkdeckSessionInputKind, WorkdeckSessionRegistration, WorkdeckSessionSnapshot,
    WorkdeckSessionState, resolve_session_terminal_metadata, review_process_capability,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRegistrationBootstrap {
    pub input_kind: WorkdeckSessionInputKind,
    pub changeset: Changeset,
    pub source_label: String,
    pub experimental: bool,
    pub initial_show_agent_notes: bool,
}

#[derive(Debug, Error)]
pub enum SessionRegistrationError {
    #[error(transparent)]
    Capability(#[from] BrokerCryptoError),
    #[error("could not resolve the current directory: {0}")]
    CurrentDirectory(#[from] std::io::Error),
}

fn random_uuid() -> Result<String, BrokerCryptoError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| BrokerCryptoError::Random(error.to_string()))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

fn tty_name() -> Option<String> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    Command::new("tty")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty() && !name.starts_with("not a tty"))
}

fn is_vcs_input(input_kind: WorkdeckSessionInputKind) -> bool {
    matches!(
        input_kind,
        WorkdeckSessionInputKind::Vcs
            | WorkdeckSessionInputKind::Show
            | WorkdeckSessionInputKind::StashShow
    )
}

fn normalized_header(hunk: &SemanticReviewHunk) -> String {
    let raw = hunk.hunk_specs.clone().unwrap_or_else(|| {
        format!(
            "@@ -{},{} +{},{} @@{}",
            hunk.deletion_start,
            hunk.deletion_count,
            hunk.addition_start,
            hunk.addition_count,
            hunk.hunk_context
                .as_deref()
                .map(|context| format!(" {context}"))
                .unwrap_or_default()
        )
    });
    let mut header = String::with_capacity(raw.len());
    let mut in_newline = false;
    for character in raw.chars() {
        if matches!(character, '\r' | '\n') {
            if !in_newline {
                header.push(' ');
                in_newline = true;
            }
        } else {
            header.push(character);
            in_newline = false;
        }
    }
    header.trim_end().to_owned()
}

fn inclusive_range(start: u32, count: u32) -> [u64; 2] {
    [
        u64::from(start),
        u64::from(start.saturating_add(count.max(1)).saturating_sub(1)),
    ]
}

#[must_use]
pub fn build_session_files(publication: &ReviewPublication) -> Vec<SessionReviewFile> {
    publication
        .document
        .files
        .iter()
        .map(|file| SessionReviewFile {
            summary: crate::SessionFileSummary {
                id: file.runtime_id.clone(),
                path: file.path.clone(),
                previous_path: file.previous_path.clone(),
                additions: u64::try_from(file.stats.additions).unwrap_or(u64::MAX),
                deletions: u64::try_from(file.stats.deletions).unwrap_or(u64::MAX),
                hunk_count: u64::try_from(file.hunks.len()).unwrap_or(u64::MAX),
            },
            patch: None,
            hunks: file
                .hunks
                .iter()
                .enumerate()
                .map(|(index, hunk)| SessionReviewHunk {
                    index: u64::try_from(index).unwrap_or(u64::MAX),
                    header: normalized_header(hunk),
                    old_range: Some(inclusive_range(hunk.deletion_start, hunk.deletion_count)),
                    new_range: Some(inclusive_range(hunk.addition_start, hunk.addition_count)),
                })
                .collect(),
        })
        .collect()
}

#[must_use]
pub fn build_review_catalog(publication: &ReviewPublication) -> WorkdeckReviewResourceCatalogV1 {
    WorkdeckReviewResourceCatalogV1 {
        generation: publication.generation.clone(),
        file_keys_by_runtime_id: publication
            .document
            .files
            .iter()
            .map(|file| (file.runtime_id.clone(), file.key.clone()))
            .collect(),
        resources: publication.resources.clone(),
    }
}

fn registration_info(
    bootstrap: &SessionRegistrationBootstrap,
    publication: &ReviewPublication,
) -> Result<WorkdeckSessionInfo, BrokerCryptoError> {
    Ok(WorkdeckSessionInfo {
        input_kind: bootstrap.input_kind,
        title: bootstrap.changeset.title.clone(),
        source_label: bootstrap.source_label.clone(),
        experimental_features: Some(if bootstrap.experimental {
            vec![WorkdeckExperimentalFeature::Stml]
        } else {
            Vec::new()
        }),
        files: build_session_files(publication),
        review_catalog: Some(build_review_catalog(publication)),
        review_capability_digest: Some(review_process_capability()?.digest.clone()),
    })
}

/// Build one live session registration from the process and terminal environment.
pub fn create_session_registration(
    bootstrap: &SessionRegistrationBootstrap,
    publication: &ReviewPublication,
) -> Result<WorkdeckSessionRegistration, SessionRegistrationError> {
    let env = std::env::vars().collect::<BTreeMap<_, _>>();
    Ok(WorkdeckSessionRegistration {
        registration_version: SESSION_BROKER_REGISTRATION_VERSION,
        session_id: random_uuid()?,
        pid: u64::from(std::process::id()),
        cwd: std::env::current_dir()?.to_string_lossy().into_owned(),
        repo_root: is_vcs_input(bootstrap.input_kind).then(|| bootstrap.source_label.clone()),
        launched_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        terminal: resolve_session_terminal_metadata(&env, tty_name().as_deref()),
        info: registration_info(bootstrap, publication)?,
    })
}

/// Refresh input metadata while retaining the process/session identity and launch facts.
pub fn update_session_registration(
    current: &WorkdeckSessionRegistration,
    bootstrap: &SessionRegistrationBootstrap,
    publication: &ReviewPublication,
) -> Result<WorkdeckSessionRegistration, SessionRegistrationError> {
    let mut updated = current.clone();
    updated.registration_version = SESSION_BROKER_REGISTRATION_VERSION;
    updated.repo_root = is_vcs_input(bootstrap.input_kind).then(|| bootstrap.source_label.clone());
    updated.info = registration_info(bootstrap, publication)?;
    Ok(updated)
}

/// Start with a valid first-hunk selection until the mounted UI reports live state.
#[must_use]
pub fn create_initial_session_snapshot(
    bootstrap: &SessionRegistrationBootstrap,
    publication: &ReviewPublication,
) -> WorkdeckSessionSnapshot {
    let first_file = publication.document.files.first();
    let first_hunk = first_file.and_then(|file| file.hunks.first());
    WorkdeckSessionSnapshot {
        updated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        state: WorkdeckSessionState {
            selected_file_id: first_file.map(|file| file.runtime_id.clone()),
            selected_file_path: first_file.map(|file| file.path.clone()),
            selected_hunk_index: 0,
            selected_hunk_old_range: first_hunk
                .map(|hunk| inclusive_range(hunk.deletion_start, hunk.deletion_count)),
            selected_hunk_new_range: first_hunk
                .map(|hunk| inclusive_range(hunk.addition_start, hunk.addition_count)),
            show_agent_notes: bootstrap.initial_show_agent_notes,
            note_markup_width: None,
            live_comment_count: 0,
            live_comments: Vec::new(),
            review_note_count: Some(0),
            review_notes: Some(Vec::new()),
            review_publication: Some(ReviewPublicationAddress {
                generation: publication.generation.clone(),
                state_revision: 0,
            }),
        },
    }
}

#[cfg(test)]
mod tests {
    use workdeck_core::{
        ChangesetSource, DiffFile, DiffHunk, DiffLine, DiffLineKind, FileChangeKind, FileFlags,
        FileSourceSnapshots, FileStats,
    };
    use workdeck_review::{ReviewResourceDescriptor, build_review_publication};

    use super::*;

    fn file(path: &str, previous_path: Option<&str>) -> DiffFile {
        let mut file = DiffFile {
            key: String::new(),
            runtime_id: "file-1".into(),
            path: path.into(),
            previous_path: previous_path.map(str::to_owned),
            change_kind: FileChangeKind::Renamed,
            language: Some("typescript".into()),
            stats: FileStats {
                additions: 1,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: "@@ -1 +1 @@\n-export const value = 1;\n+export const value = 2;\n".into(),
            split_row_count: 1,
            stack_row_count: 2,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -1 +1 @@\nfunction name".into(),
                context: Some("function name".into()),
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                split_row_start: 0,
                split_row_count: 1,
                stack_row_start: 0,
                stack_row_count: 2,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        content: "export const value = 1;".into(),
                        old_line: Some(1),
                        new_line: None,
                        moved: false,
                        no_newline_at_eof: false,
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        content: "export const value = 2;".into(),
                        old_line: None,
                        new_line: Some(1),
                        moved: false,
                        no_newline_at_eof: false,
                    },
                ],
            }],
            content_identity: String::new(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: None,
        };
        file.refresh_identity();
        file
    }

    fn bootstrap(files: Vec<DiffFile>) -> SessionRegistrationBootstrap {
        SessionRegistrationBootstrap {
            input_kind: WorkdeckSessionInputKind::Vcs,
            changeset: Changeset {
                id: "changeset-1".into(),
                source_label: "/repo".into(),
                title: "working tree".into(),
                summary: None,
                agent_summary: None,
                source: ChangesetSource::WorkingTree { staged: false },
                files,
            },
            source_label: "/repo".into(),
            experimental: false,
            initial_show_agent_notes: true,
        }
    }

    fn publication(bootstrap: &SessionRegistrationBootstrap) -> ReviewPublication {
        build_review_publication(
            &bootstrap.changeset.files,
            "generation:test:0",
            Some(&bootstrap.source_label),
        )
    }

    #[test]
    fn registration_exports_hunks_repo_selection_and_no_inline_patch_bodies() {
        let bootstrap = bootstrap(vec![file("src/example.ts", Some("src/old-example.ts"))]);
        let registration =
            create_session_registration(&bootstrap, &publication(&bootstrap)).unwrap();
        assert_eq!(
            registration.registration_version,
            SESSION_BROKER_REGISTRATION_VERSION
        );
        assert_eq!(registration.pid, u64::from(std::process::id()));
        assert_eq!(registration.repo_root.as_deref(), Some("/repo"));
        assert!(!registration.session_id.is_empty());
        assert!(!registration.launched_at.is_empty());
        let file = &registration.info.files[0];
        assert_eq!(file.summary.id, "file-1");
        assert_eq!(file.summary.path, "src/example.ts");
        assert_eq!(
            file.summary.previous_path.as_deref(),
            Some("src/old-example.ts")
        );
        assert_eq!(
            (
                file.summary.additions,
                file.summary.deletions,
                file.summary.hunk_count
            ),
            (1, 1, 1)
        );
        assert!(file.patch.is_none());
        assert_eq!(file.hunks[0].old_range, Some([1, 1]));
        assert_eq!(file.hunks[0].new_range, Some([1, 1]));
        assert_eq!(file.hunks[0].header, "@@ -1 +1 @@ function name");
    }

    #[test]
    fn registration_and_initial_selection_preserve_unicode_rename_paths() {
        let bootstrap = bootstrap(vec![file(
            "国際化/한국어-🧪.txt",
            Some("国際化/日本語.txt"),
        )]);
        let publication = publication(&bootstrap);
        let registration = create_session_registration(&bootstrap, &publication).unwrap();
        let snapshot = create_initial_session_snapshot(&bootstrap, &publication);
        assert_eq!(
            registration.info.files[0].summary.path,
            "国際化/한국어-🧪.txt"
        );
        assert_eq!(
            registration.info.files[0].summary.previous_path.as_deref(),
            Some("国際化/日本語.txt")
        );
        assert_eq!(
            snapshot.state.selected_file_path.as_deref(),
            Some("国際化/한국어-🧪.txt")
        );
    }

    #[test]
    fn update_preserves_identity_and_refreshes_non_vcs_metadata_and_capability() {
        let initial = bootstrap(vec![file("src/example.ts", None)]);
        let current = create_session_registration(&initial, &publication(&initial)).unwrap();
        let mut next = bootstrap(Vec::new());
        next.input_kind = WorkdeckSessionInputKind::Patch;
        next.changeset.title = "patch file".into();
        next.source_label = "change.patch".into();
        let updated = update_session_registration(&current, &next, &publication(&next)).unwrap();
        assert_eq!(updated.session_id, current.session_id);
        assert_eq!(updated.pid, current.pid);
        assert!(updated.repo_root.is_none());
        assert_eq!(updated.info.input_kind, WorkdeckSessionInputKind::Patch);
        assert_eq!(updated.info.title, "patch file");
        assert!(updated.info.files.is_empty());
        assert_eq!(
            updated.info.review_capability_digest,
            current.info.review_capability_digest
        );
    }

    #[test]
    fn experimental_feature_is_advertised_only_for_opted_in_launches() {
        let mut bootstrap = bootstrap(Vec::new());
        let regular = create_session_registration(&bootstrap, &publication(&bootstrap)).unwrap();
        assert_eq!(regular.info.experimental_features, Some(Vec::new()));
        bootstrap.experimental = true;
        let experimental =
            create_session_registration(&bootstrap, &publication(&bootstrap)).unwrap();
        assert_eq!(
            experimental.info.experimental_features,
            Some(vec![WorkdeckExperimentalFeature::Stml])
        );
    }

    #[test]
    fn initial_snapshot_focuses_first_hunk_and_empty_review_remains_explicit() {
        let initial = bootstrap(vec![file("src/example.ts", None)]);
        let snapshot = create_initial_session_snapshot(&initial, &publication(&initial));
        assert_eq!(snapshot.state.selected_file_id.as_deref(), Some("file-1"));
        assert_eq!(snapshot.state.selected_hunk_index, 0);
        assert_eq!(snapshot.state.selected_hunk_old_range, Some([1, 1]));
        assert_eq!(snapshot.state.selected_hunk_new_range, Some([1, 1]));
        assert!(snapshot.state.show_agent_notes);
        assert_eq!(snapshot.state.review_note_count, Some(0));
        assert_eq!(
            snapshot
                .state
                .review_publication
                .as_ref()
                .unwrap()
                .state_revision,
            0
        );

        let mut empty = bootstrap(Vec::new());
        empty.initial_show_agent_notes = false;
        let snapshot = create_initial_session_snapshot(&empty, &publication(&empty));
        assert!(snapshot.state.selected_file_id.is_none());
        assert!(snapshot.state.selected_file_path.is_none());
        assert!(snapshot.state.selected_hunk_old_range.is_none());
        assert!(snapshot.state.selected_hunk_new_range.is_none());
        assert!(!snapshot.state.show_agent_notes);
        assert!(snapshot.state.live_comments.is_empty());
        assert_eq!(snapshot.state.review_notes, Some(Vec::new()));
    }

    #[test]
    fn resource_catalog_maps_runtime_ids_and_generation_resources() {
        let bootstrap = bootstrap(vec![file("src/example.ts", None)]);
        let publication = publication(&bootstrap);
        let registration = create_session_registration(&bootstrap, &publication).unwrap();
        let catalog = registration.info.review_catalog.unwrap();
        let file_key = &publication.document.files[0].key;
        assert_eq!(catalog.generation, "generation:test:0");
        assert_eq!(catalog.file_keys_by_runtime_id["file-1"], *file_key);
        assert_eq!(catalog.resources.len(), 2);
        assert!(catalog.resources.iter().all(|resource| {
            resource.base().generation == "generation:test:0"
                && matches!(
                    resource,
                    ReviewResourceDescriptor::CanonicalFile { .. }
                        | ReviewResourceDescriptor::Patch { .. }
                )
        }));
    }
}
