//! Source worker integration for the live review canvas.

use super::*;
use workdeck_review::ReviewSourceLoader;

struct VcsSourceLoader(Arc<workdeck_vcs::VcsFileSourceCapability>);

struct PublicationSourceLoader(workdeck_vcs::VcsSourceCapabilities);

impl ReviewSourceLoader for PublicationSourceLoader {
    fn get_full_text(
        &self,
        file: &DiffFile,
        side: ReviewSide,
    ) -> std::result::Result<Option<String>, workdeck_review::ReviewSourceLoadError> {
        if let Some(capability) = self.0.get(file) {
            VcsSourceLoader(capability).get_full_text(file, side)
        } else {
            // Existing immutable snapshots remain usable without executable
            // authority. This fallback cannot open a path or invoke a provider.
            workdeck_review::SnapshotReviewSourceLoader.get_full_text(file, side)
        }
    }
}

pub(crate) fn publication_source_loader(
    capabilities: Option<workdeck_vcs::VcsSourceCapabilities>,
) -> Arc<dyn ReviewSourceLoader> {
    match capabilities {
        Some(capabilities) => Arc::new(PublicationSourceLoader(capabilities)),
        None => Arc::new(workdeck_review::SnapshotReviewSourceLoader),
    }
}

impl ReviewSourceLoader for VcsSourceLoader {
    fn get_full_text(
        &self,
        _: &DiffFile,
        side: ReviewSide,
    ) -> std::result::Result<Option<String>, workdeck_review::ReviewSourceLoadError> {
        self.0
            .read(side)
            .map(|result| match result {
                workdeck_vcs::VcsFileSourceResult::Source(source) => Ok(Some(source.content)),
                workdeck_vcs::VcsFileSourceResult::Missing => Ok(None),
                workdeck_vcs::VcsFileSourceResult::TooLarge { .. } => {
                    Err(workdeck_review::ReviewSourceLoadError::TooLarge)
                }
            })
            .map_err(|error| {
                workdeck_review::ReviewSourceLoadError::Unavailable(error.to_string())
            })?
    }
}

pub(super) struct SourceLoaderBinding {
    identity: Option<String>,
    vcs_runtime_identity: Option<u64>,
    loader: Arc<dyn ReviewSourceLoader>,
}

#[derive(Debug)]
pub(super) struct PendingSourceReveal {
    pub(super) runtime_id: String,
    pub(super) gap: (String, usize),
    pub(super) target: ReviewNoteTarget,
}

impl std::fmt::Debug for SourceLoaderBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceLoaderBinding")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl ReviewApp {
    /// Replace runtime source ownership after a successful input/publication commit.
    pub fn install_vcs_source_capabilities(
        &mut self,
        capabilities: &workdeck_vcs::VcsSourceCapabilities,
    ) {
        self.options.source_capabilities = Some(capabilities.clone());
        let files = self.with_state(|state| state.changeset_snapshot());
        let mut installed = BTreeSet::new();
        for file in &files.files {
            let Some(capability) = capabilities.get(file) else {
                continue;
            };
            installed.insert(file.key.clone());
            let runtime_identity = capability.runtime_identity();
            let loader: Arc<dyn ReviewSourceLoader> = Arc::new(VcsSourceLoader(capability));
            let retained = self.source_loaders.get(&file.key).is_some_and(|binding| {
                binding.identity == file.source_identity
                    && (file.source_attested
                        || binding.vcs_runtime_identity == Some(runtime_identity))
            });
            if retained {
                self.source_loaders.insert(
                    file.key.clone(),
                    SourceLoaderBinding {
                        identity: file.source_identity.clone(),
                        vcs_runtime_identity: Some(runtime_identity),
                        loader,
                    },
                );
            } else {
                self.bind_source_loader(file, loader, Some(runtime_identity));
            }
        }
        let retired = self
            .source_loaders
            .keys()
            .filter(|key| !installed.contains(*key))
            .cloned()
            .collect();
        self.source_requests.retire(&retired);
        self.options.source_presentation.retire(&retired);
        self.source_loaders.retain(|key, _| installed.contains(key));
        let expanded_files: BTreeSet<_> = self
            .expanded_gaps
            .iter()
            .map(|(key, _)| key.clone())
            .collect();
        for file in &files.files {
            if expanded_files.contains(&file.key) {
                self.start_source_load_for_file(file, review_expansion_side(file.change_kind));
            }
        }
    }
    /// Install host-owned source authority for one currently mounted file.
    /// A serialized source descriptor alone never registers an executable reader.
    pub fn install_source_loader(
        &mut self,
        file_key: &str,
        loader: Arc<dyn ReviewSourceLoader>,
    ) -> bool {
        let changeset = self.with_state(|state| state.changeset_snapshot());
        let file = changeset.files.iter().find(|file| file.key == file_key);
        let Some(file) = file else {
            return false;
        };
        self.bind_source_loader(file, loader, None);
        true
    }

    fn bind_source_loader(
        &mut self,
        file: &DiffFile,
        loader: Arc<dyn ReviewSourceLoader>,
        vcs_runtime_identity: Option<u64>,
    ) {
        self.source_requests
            .retire(&BTreeSet::from([file.key.clone()]));
        self.source_loaders.insert(
            file.key.clone(),
            SourceLoaderBinding {
                identity: file.source_identity.clone(),
                vcs_runtime_identity,
                loader,
            },
        );
        self.options.source_presentation.pending(file);
    }

    pub(super) fn start_source_load(&mut self, file_key: &str, side: ReviewSide) {
        let changeset = self.with_state(|state| state.changeset_snapshot());
        let file = changeset.files.iter().find(|file| file.key == file_key);
        let Some(file) = file else {
            return;
        };
        self.start_source_load_for_file(file, side);
    }

    fn start_source_load_for_file(&mut self, file: &DiffFile, side: ReviewSide) {
        let Some(binding) = self
            .source_loaders
            .get(&file.key)
            .filter(|binding| binding.identity == file.source_identity)
        else {
            return;
        };
        if let Some(update) = self.source_requests.start(
            file,
            side,
            Arc::clone(&binding.loader),
            self.options.source_presentation.status(file),
        ) {
            self.options
                .source_presentation
                .set_status(file, update.status);
        }
    }

    /// Called on the UI thread; workers never mutate the mounted review directly.
    pub fn poll_source_requests(&mut self) -> usize {
        let completions = self.source_requests.poll();
        let count = completions.len();
        for completion in completions {
            if let Some(diagnostic) = completion.diagnostic {
                eprintln!("{diagnostic}");
            }
            let Some(update) = completion.update else {
                continue;
            };
            let file = self.with_state(|state| {
                state
                    .changeset()
                    .files
                    .iter()
                    .find(|file| file.key == update.file_key)
                    .cloned()
            });
            if let Some(file) = file {
                self.options
                    .source_presentation
                    .set_status(&file, update.status);
            }
        }
        if count > 0 {
            self.reveal_pending_source_cursor();
        }
        count
    }

    fn reveal_pending_source_cursor(&mut self) {
        let Some(pending) = &self.pending_source_reveal else {
            return;
        };
        if !self.expanded_gaps.contains(&pending.gap) {
            return;
        }
        let Some(file_index) =
            self.with_state(|state| {
                state.changeset().files.iter().position(|file| {
                    file.runtime_id == pending.runtime_id && file.key == pending.gap.0
                })
            })
        else {
            return;
        };
        let mut target = pending.target;
        target.file_index = file_index;
        let rows = self.current_review_rows();
        let Some(cursor) = review_line_cursors(&rows)
            .into_iter()
            .find(|cursor| cursor.target == target)
        else {
            return;
        };
        self.pending_source_reveal = None;
        self.apply_review_line_cursor(cursor);
        let viewport = usize::from(
            self.review_height
                .get()
                .saturating_sub(self.review_reserved_rows())
                .max(1),
        );
        self.keep_current_line_visible(viewport, rows.lines.len().saturating_sub(1));
    }

    pub(super) fn reconcile_source_loaders(&mut self, changeset: &Changeset) {
        let attested_files: BTreeSet<_> = changeset
            .files
            .iter()
            .filter(|file| file.source_attested)
            .map(|file| (file.key.as_str(), file.source_identity.as_deref()))
            .collect();
        let retired: BTreeSet<_> = self
            .source_loaders
            .iter()
            .filter_map(|(key, binding)| {
                let retained =
                    attested_files.contains(&(key.as_str(), binding.identity.as_deref()));
                (!retained).then(|| key.clone())
            })
            .collect();
        self.source_requests.retire(&retired);
        self.source_loaders.retain(|key, _| !retired.contains(key));
        if let Some(capabilities) = &mut self.options.source_capabilities {
            capabilities.retire(&retired);
        }
        self.options.source_presentation.reconcile(&changeset.files);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::sync::mpsc;
    use workdeck_review::ReviewSourceLoadError;

    struct Loader(Mutex<mpsc::Receiver<String>>);
    impl ReviewSourceLoader for Loader {
        fn get_full_text(
            &self,
            _: &DiffFile,
            _: ReviewSide,
        ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
            Ok(Some(
                self.0
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap(),
            ))
        }
    }

    fn setup() -> (ReviewApp, mpsc::Sender<String>) {
        setup_with_cache_key(Some("one"))
    }

    fn setup_with_cache_key(cache_key: Option<&str>) -> (ReviewApp, mpsc::Sender<String>) {
        let mut changeset = workdeck_diff::changeset_from_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3 +3 @@\n-old\n+new\n",
            "lazy",
            "lazy",
            "lazy",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        );
        changeset.files[0].set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: cache_key.map(str::to_owned),
        }));
        let key = changeset.files[0].key.clone();
        let mut app = ReviewApp::new(
            changeset,
            ReviewOptions {
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        let (sender, receiver) = mpsc::channel();
        assert!(app.install_source_loader(&key, Arc::new(Loader(Mutex::new(receiver)))));
        (app, sender)
    }

    #[test]
    fn bulk_binding_and_reconciliation_preserve_authority_without_source_reads() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let reads = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&reads);
        let patch = (0..256).map(|index| format!(
            "diff --git a/file{index}.rs b/file{index}.rs\n--- a/file{index}.rs\n+++ b/file{index}.rs\n@@ -3 +3 @@\n-old\n+new\n"
        )).collect::<String>();
        let (changeset, capabilities) = workdeck_vcs::materialize_vcs_patch_result_deferred(
            workdeck_vcs::VcsPatchResult {
                repo_root: ".".into(),
                source_label: "bulk".into(),
                title: "bulk".into(),
                patch_text: patch,
                untracked_paths: vec![],
                extra_files: vec![],
                source_cache_key: Some("pinned".into()),
                source_reader: Some(Arc::new(move |_| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(workdeck_vcs::VcsFileSourceResult::Missing)
                })),
            },
            "bulk",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let mut app = ReviewApp::new(
            changeset,
            ReviewOptions {
                highlight: false,
                source_capabilities: Some(capabilities.clone()),
                ..ReviewOptions::default()
            },
        );
        let snapshot = app.with_state(|state| state.changeset_snapshot());
        assert_eq!(app.source_loaders.len(), 256);
        app.install_vcs_source_capabilities(&capabilities);
        assert!(Arc::ptr_eq(
            &snapshot,
            &app.with_state(|state| state.changeset_snapshot())
        ));
        assert!(
            snapshot
                .files
                .iter()
                .all(|file| app.options.source_presentation.available(file))
        );
        let mut next = (*snapshot).clone();
        next.files.truncate(128);
        next.files[0].source_attested = false;
        next.files[1].source_identity = Some("changed".into());
        app.reconcile_source_loaders(&next);
        assert_eq!(app.source_loaders.len(), 126);
        assert!(!app.options.source_presentation.available(&next.files[0]));
        assert!(!app.options.source_presentation.available(&next.files[1]));
        assert!(
            next.files[2..]
                .iter()
                .all(|file| app.options.source_presentation.available(file))
        );
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn reinstalling_same_unversioned_vcs_reader_keeps_loaded_presentation() {
        fn provider(text: &'static str) -> (Changeset, workdeck_vcs::VcsSourceCapabilities) {
            workdeck_vcs::materialize_vcs_patch_result_deferred(
                workdeck_vcs::VcsPatchResult {
                    repo_root: ".".into(), source_label: "repo".into(), title: "working tree".into(),
                    patch_text: "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3 +3 @@\n-old\n+new\n".into(),
                    untracked_paths: vec![], extra_files: vec![], source_cache_key: None,
                    source_reader: Some(Arc::new(move |_| Ok(workdeck_vcs::VcsFileSourceResult::Source(
                        workdeck_core::SourceSnapshot::new(text.into(), workdeck_core::SourceOrigin::WorkingTree, false)
                    )))),
                }, "repo", workdeck_core::ChangesetSource::WorkingTree { staged: false },
            ).unwrap()
        }
        let (review, capabilities) = provider("initial-one\ninitial-two\nnew\n");
        assert!(!review.files[0].source_attested);
        let mut app = ReviewApp::new(
            review.clone(),
            ReviewOptions {
                highlight: false,
                source_capabilities: Some(capabilities.clone()),
                ..Default::default()
            },
        );
        app.toggle_source_gap();
        // Reinstalling while the worker is pending must not retire its completion.
        app.install_vcs_source_capabilities(&capabilities);
        drain_one(&mut app);
        let selected = app.with_state(|state| state.selection());
        let before = rows(&app);
        app.reload(review.clone());
        app.install_vcs_source_capabilities(&capabilities);
        assert!(
            matches!(app.options.source_presentation.status(&review.files[0]),
            Some(workdeck_review::ReviewSourceStatus::Loaded { text }) if text.starts_with("initial-one"))
        );
        assert_eq!(rows(&app), before);
        assert_eq!(app.with_state(|state| state.selection()), selected);

        let (replacement, fresh_capabilities) = provider("fresh-one\nfresh-two\nnew\n");
        assert_eq!(replacement, review);
        app.install_vcs_source_capabilities(&fresh_capabilities);
        assert!(matches!(
            app.options.source_presentation.status(&review.files[0]),
            Some(workdeck_review::ReviewSourceStatus::Loading)
        ));
        drain_one(&mut app);
        assert!(rows(&app).contains("fresh-one"));
        assert!(!rows(&app).contains("initial-one"));
    }

    pub(crate) fn drain_one(app: &mut ReviewApp) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.poll_source_requests() == 0 {
            assert!(Instant::now() < deadline, "source worker did not finish");
            std::thread::yield_now();
        }
    }

    pub(crate) fn rows(app: &ReviewApp) -> String {
        app.current_review_rows()
            .lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn gap_toggle_starts_a_worker_and_completion_reaches_the_live_rows() {
        let (mut app, sender) = setup();
        let original = app.with_state(|state| state.changeset().clone());
        let original_selection = app.with_state(|state| state.selection());
        app.toggle_source_gap();
        assert!(rows(&app).contains("Loading 2 unchanged lines"));
        assert_eq!(app.poll_source_requests(), 0);
        sender
            .send("first-source-line\nsecond-source-line\nnew\n".into())
            .unwrap();
        drain_one(&mut app);
        let text = rows(&app);
        assert!(text.contains("first-source-line"), "{text}");
        assert!(text.contains("Hide 2 unchanged lines"));
        assert_eq!(app.with_state(|state| state.selection().line), Some(1));
        assert!(app.pending_source_reveal.is_none());
        let cursor = app
            .current_review_rows()
            .line_cursors
            .into_iter()
            .find(|cursor| cursor.target.line == 1)
            .expect("loaded source line has a cursor");
        app.apply_review_line_cursor(cursor);
        assert_eq!(app.with_state(|state| state.selection().line), Some(1));
        assert_eq!(app.with_state(|state| state.changeset().clone()), original);
        app.toggle_source_gap();
        assert_eq!(
            app.with_state(|state| state.selection()),
            original_selection
        );
    }

    #[test]
    fn collapse_before_completion_cancels_the_pending_cursor_reveal() {
        let (mut app, sender) = setup();
        let selection = app.with_state(|state| state.selection());
        app.toggle_source_gap();
        assert!(app.pending_source_reveal.is_some());
        app.toggle_source_gap();
        assert!(app.pending_source_reveal.is_none());
        sender
            .send("hidden-source-one\nhidden-source-two\nnew\n".into())
            .unwrap();
        drain_one(&mut app);
        assert!(!rows(&app).contains("hidden-source-one"));
        assert_eq!(app.with_state(|state| state.selection()), selection);
    }

    #[test]
    fn addressed_gap_on_fully_deleted_file_errors_without_source_load() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct DeletedSource(AtomicUsize);
        impl ReviewSourceLoader for DeletedSource {
            fn get_full_text(
                &self,
                _: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok((side == ReviewSide::Old).then(|| "removed\n".into()))
            }
        }
        let mut file = workdeck_diff::diff_from_file_snapshots(
            workdeck_diff::FileSnapshot {
                cache_key: "removed:before",
                contents: "removed\n",
                name: "removed.ts",
            },
            workdeck_diff::FileSnapshot {
                cache_key: "removed:after",
                contents: "",
                name: "removed.ts",
            },
            workdeck_diff::FileComparisonOptions { context_radius: 3 },
        )
        .unwrap();
        file.runtime_id = "removed".into();
        file.language = Some("typescript".into());
        file.patch.clear();
        for source in file
            .sources
            .old
            .iter_mut()
            .chain(file.sources.new.iter_mut())
        {
            source.origin = workdeck_core::SourceOrigin::DiffMetadata;
            source.attested = false;
        }
        file.set_sources(file.sources.clone());
        file.set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: None,
        }));
        let mut review = workdeck_core::Changeset {
            id: "deleted".into(),
            source_label: "repo".into(),
            title: "repo working tree".into(),
            summary: None,
            agent_summary: None,
            source: workdeck_core::ChangesetSource::WorkingTree { staged: false },
            files: vec![file],
        };
        review.refresh_review_identities();
        let key = review.files[0].key.clone();
        assert_eq!(review.files[0].hunks.len(), 1);
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                highlight: false,
                ..Default::default()
            },
        );
        let loader = Arc::new(DeletedSource(AtomicUsize::new(0)));
        assert!(app.install_source_loader(&key, loader.clone()));
        let selection = app.with_state(|state| state.selection());
        let error = app.toggle_source_gap_for_file(&key, 1).unwrap_err();
        assert_eq!(
            error.code,
            workdeck_review::ReviewIntentPlanningErrorCode::GapNotFound
        );
        assert_eq!(
            error.message,
            "Review gap trailing:0 does not exist in removed.ts."
        );
        assert_eq!(loader.0.load(Ordering::SeqCst), 0);
        app.with_state(|state| {
            assert!(
                app.options
                    .source_presentation
                    .status(&state.changeset().files[0])
                    .is_none()
            )
        });
        assert!(app.expanded_gaps.is_empty());
        assert!(app.pending_source_reveal.is_none());
        assert_eq!(app.with_state(|state| state.selection()), selection);
        app.toggle_source_gap();
        assert!(app.status.is_none());
        assert_eq!(loader.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn failed_source_loads_render_reason_retry_and_cache_the_recovery() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct RecoveringLoader {
            failure: u8,
            calls: AtomicUsize,
        }
        impl ReviewSourceLoader for RecoveringLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                assert_eq!(side, ReviewSide::New);
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return match self.failure {
                        0 => Ok(None),
                        1 => Err(ReviewSourceLoadError::Unavailable(
                            "source unavailable".into(),
                        )),
                        _ => Err(ReviewSourceLoadError::TooLarge),
                    };
                }
                Ok(Some("recovered-first\nrecovered-second\nnew\n".into()))
            }
        }
        for failure in 0..3 {
            let (mut app, _unused_sender) = setup();
            let key = app.with_state(|state| state.changeset().files[0].key.clone());
            let loader = Arc::new(RecoveringLoader {
                failure,
                calls: AtomicUsize::new(0),
            });
            assert!(app.install_source_loader(&key, loader.clone()));
            let selection = app.with_state(|state| state.selection());
            app.toggle_source_gap();
            drain_one(&mut app);
            let expected_reason =
                (failure == 2).then_some(workdeck_review::ReviewSourceErrorReason::TooLarge);
            app.with_state(|state| {
                assert_eq!(
                    app.options
                        .source_presentation
                        .status(&state.changeset().files[0]),
                    Some(&workdeck_review::ReviewSourceStatus::Error {
                        reason: expected_reason
                    })
                )
            });
            let label = if failure == 2 {
                "Source too large to expand 2 unchanged lines"
            } else {
                "Could not load 2 unchanged lines"
            };
            assert!(rows(&app).contains(label), "{}", rows(&app));
            assert_eq!(app.with_state(|state| state.selection()), selection);
            assert_eq!(loader.calls.load(Ordering::SeqCst), 1);

            // Hunk starts the loader after either toggle direction, so collapsing
            // an errored gap retries while cancelling its pending cursor reveal.
            app.toggle_source_gap();
            assert!(app.expanded_gaps.is_empty());
            assert!(app.pending_source_reveal.is_none());
            drain_one(&mut app);
            assert_eq!(loader.calls.load(Ordering::SeqCst), 2);
            assert!(!rows(&app).contains("recovered-first"));
            assert_eq!(app.with_state(|state| state.selection()), selection);
            app.toggle_source_gap();
            assert!(rows(&app).contains("recovered-first"));
            assert_eq!(app.with_state(|state| state.selection().line), Some(1));
            app.toggle_source_gap();
            app.toggle_source_gap();
            assert_eq!(loader.calls.load(Ordering::SeqCst), 2);
            assert!(rows(&app).contains("recovered-second"));
        }
    }

    #[test]
    fn latest_gap_receives_cursor_when_one_source_load_reveals_two_gaps() {
        struct RecordingLoader {
            calls: mpsc::Sender<ReviewSide>,
            source: Loader,
        }
        impl ReviewSourceLoader for RecordingLoader {
            fn get_full_text(
                &self,
                file: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                self.calls.send(side).unwrap();
                self.source.get_full_text(file, side)
            }
        }
        let before = (1..=50)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before
            .replace("line 10\n", "line 10 changed\n")
            .replace("line 40\n", "line 40 changed\n");
        let mut file = workdeck_diff::diff_from_file_snapshots(
            workdeck_diff::FileSnapshot {
                cache_key: "alpha:before",
                contents: &before,
                name: "alpha.ts",
            },
            workdeck_diff::FileSnapshot {
                cache_key: "alpha:after",
                contents: &after,
                name: "alpha.ts",
            },
            workdeck_diff::FileComparisonOptions { context_radius: 3 },
        )
        .unwrap();
        file.runtime_id = "alpha".into();
        file.language = Some("typescript".into());
        file.patch.clear();
        for source in file
            .sources
            .old
            .iter_mut()
            .chain(file.sources.new.iter_mut())
        {
            source.origin = workdeck_core::SourceOrigin::DiffMetadata;
            source.attested = false;
        }
        file.set_sources(file.sources.clone());
        file.set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: None,
        }));
        let mut review = workdeck_core::Changeset {
            id: "two-gaps".into(),
            source_label: "repo".into(),
            title: "repo working tree".into(),
            summary: None,
            agent_summary: None,
            source: workdeck_core::ChangesetSource::WorkingTree { staged: false },
            files: vec![file],
        };
        review.refresh_review_identities();
        assert_eq!(review.files[0].hunks.len(), 2);
        let key = review.files[0].key.clone();
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                highlight: false,
                ..Default::default()
            },
        );
        let (source_tx, source_rx) = mpsc::channel();
        let (calls_tx, calls_rx) = mpsc::channel();
        assert!(app.install_source_loader(
            &key,
            Arc::new(RecordingLoader {
                calls: calls_tx,
                source: Loader(Mutex::new(source_rx)),
            })
        ));
        app.toggle_source_gap_for_file(&key, 0).unwrap();
        app.toggle_source_gap_for_file(&key, 1).unwrap();
        assert_eq!(
            calls_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ReviewSide::New
        );
        app.with_state(|state| {
            assert!(matches!(
                app.options
                    .source_presentation
                    .status(&state.changeset().files[0]),
                Some(workdeck_review::ReviewSourceStatus::Loading)
            ))
        });
        source_tx.send(after).unwrap();
        drain_one(&mut app);
        assert!(app.expanded_gaps.contains(&(key.clone(), 0)));
        assert!(app.expanded_gaps.contains(&(key.clone(), 1)));
        let gap = app.with_state(|state| {
            workdeck_review::review_gap_geometry_for_file(&state.changeset().files[0])
                .leading_gap(1)
                .unwrap()
        });
        let cursor = app.current_review_line_cursor().unwrap().target;
        assert_eq!(cursor.file_index, 0);
        assert_eq!(cursor.hunk_index, 1);
        assert_eq!(cursor.side, ReviewSide::New);
        assert_eq!(cursor.line, gap.new_range.start);
        assert!(gap.new_range.start <= cursor.line && cursor.line <= gap.new_range.end);
        assert!(calls_rx.try_recv().is_err());
    }

    #[test]
    fn attested_loaded_selection_survives_reload_but_not_source_retirement() {
        let (mut app, sender) = setup();
        app.toggle_source_gap();
        sender
            .send("retained-one\nretained-two\nnew\n".into())
            .unwrap();
        drain_one(&mut app);
        assert_eq!(app.with_state(|state| state.selection().line), Some(1));
        let mut replacement = app.with_state(|state| state.changeset().clone());
        replacement.files[0].runtime_id = "replacement-runtime".into();
        app.reload(replacement.clone());
        assert_eq!(app.with_state(|state| state.selection().line), Some(1));
        assert!(rows(&app).contains("retained-one"));
        replacement.files[0].set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: Some("changed".into()),
        }));
        app.reload(replacement);
        assert_ne!(app.with_state(|state| state.selection().line), Some(1));
        assert!(!rows(&app).contains("retained-one"));
    }

    #[test]
    fn identical_soft_reload_preserves_unattested_loaded_source_and_cursor() {
        let (mut app, sender) = setup_with_cache_key(None);
        app.toggle_source_gap();
        sender
            .send("retained-one\nretained-two\nnew\n".into())
            .unwrap();
        drain_one(&mut app);
        let review = app.with_state(|state| state.changeset().clone());
        assert!(!review.files[0].source_attested);
        let selection = app.with_state(|state| state.selection());
        let cursor = app.current_review_line_cursor();
        let expanded = app.expanded_gaps.clone();
        let before = rows(&app);
        app.reload(review.clone());
        assert!(
            matches!(app.options.source_presentation.status(&review.files[0]),
            Some(workdeck_review::ReviewSourceStatus::Loaded { text }) if text == "retained-one\nretained-two\nnew\n")
        );
        assert_eq!(app.source_loaders.len(), 1);
        assert_eq!(app.expanded_gaps, expanded);
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert_eq!(app.current_review_line_cursor(), cursor);
        assert_eq!(rows(&app), before);

        // Real content replacement still retires unattested text and expansion.
        let mut changed = review.clone();
        changed.files[0].hunks[0].lines[1].content = "changed".into();
        changed.refresh_review_identities();
        assert_ne!(
            changed.files[0].source_identity,
            review.files[0].source_identity
        );
        app.reload(changed.clone());
        assert!(app.source_loaders.is_empty());
        assert!(app.expanded_gaps.is_empty());
        assert!(
            app.options
                .source_presentation
                .status(&changed.files[0])
                .is_none()
        );
        assert!(!rows(&app).contains("retained-one"));
        let (next_sender, next_receiver) = mpsc::channel();
        assert!(app.install_source_loader(
            &changed.files[0].key,
            Arc::new(Loader(Mutex::new(next_receiver)))
        ));
        app.toggle_source_gap();
        next_sender
            .send("fresh-one\nfresh-two\nchanged\n".into())
            .unwrap();
        drain_one(&mut app);
        assert!(rows(&app).contains("fresh-one"));
        assert!(!rows(&app).contains("retained-one"));
    }

    #[test]
    fn changed_source_reload_retires_a_blocked_worker_before_it_completes() {
        let (mut app, sender) = setup();
        app.toggle_source_gap();
        let mut replacement = app.with_state(|state| state.changeset().clone());
        replacement.files[0].set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: Some("two".into()),
        }));
        app.reload(replacement);
        sender
            .send("obsolete-source\nobsolete-source\nnew\n".into())
            .unwrap();
        drain_one(&mut app);
        assert!(!rows(&app).contains("obsolete-source"));
        assert!(app.source_loaders.is_empty());
        assert!(app.expanded_gaps.is_empty());
    }

    fn pinned_alpha_source_review(value: u32) -> Changeset {
        let before = (1..=12)
            .map(|line| format!("export const alpha{line} = {line};\n"))
            .collect::<String>();
        let after = before.replace("alpha8 = 8;", &format!("alpha8 = {value};"));
        pinned_alpha_review_from_text(&before, &after)
    }

    fn pinned_alpha_review_from_text(before: &str, after: &str) -> Changeset {
        let mut file = workdeck_diff::diff_from_file_snapshots(
            workdeck_diff::FileSnapshot {
                cache_key: "alpha:before",
                contents: before,
                name: "alpha.ts",
            },
            workdeck_diff::FileSnapshot {
                cache_key: "alpha:after",
                contents: after,
                name: "alpha.ts",
            },
            workdeck_diff::FileComparisonOptions { context_radius: 3 },
        )
        .unwrap();
        file.runtime_id = "alpha".into();
        file.language = Some("typescript".into());
        file.patch.clear();
        for source in file
            .sources
            .old
            .iter_mut()
            .chain(file.sources.new.iter_mut())
        {
            source.origin = workdeck_core::SourceOrigin::DiffMetadata;
            source.attested = false;
        }
        file.set_sources(file.sources.clone());
        file.set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: None,
        }));
        let mut review = Changeset {
            id: "alpha-review".into(),
            source_label: "repo".into(),
            title: "repo working tree".into(),
            summary: None,
            agent_summary: None,
            source: workdeck_core::ChangesetSource::WorkingTree { staged: false },
            files: vec![file],
        };
        review.refresh_review_identities();
        review
    }

    #[test]
    fn alpha_cursor_steps_one_row_and_clamps_at_stream_start() {
        let before = (1..=12)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let after = before
            .replace("line1 = 1;", "line1 = 100;")
            .replace("line12 = 12;", "line12 = 1200;");
        let mut review = pinned_alpha_review_from_text(&before, &after);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        assert_eq!(review.files[0].hunks.len(), 2);
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let first = app.current_review_line_cursor().unwrap();
        app.step_diff_line(1);
        assert_ne!(app.current_review_line_cursor().unwrap(), first);
        app.step_diff_line(-1);
        assert_eq!(app.current_review_line_cursor(), Some(first));
        app.step_diff_line(-1);
        assert_eq!(app.current_review_line_cursor(), Some(first));
    }

    #[test]
    fn initial_alpha_cursor_is_seeded_at_selected_hunk_without_source_reader() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let app = ReviewApp::new(review, ReviewOptions::default());
        let cursor = app.current_review_line_cursor().unwrap().target;
        app.with_state(|state| {
            assert_eq!(
                state.changeset().files[cursor.file_index].runtime_id,
                "alpha"
            );
        });
        assert_eq!(cursor.hunk_index, 0);
    }

    #[test]
    fn pending_source_cannot_repopulate_reloaded_alpha_review() {
        struct TrackedLoader {
            calls: Mutex<Vec<ReviewSide>>,
            source: Loader,
        }
        impl ReviewSourceLoader for TrackedLoader {
            fn get_full_text(
                &self,
                file: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                self.calls.lock().unwrap().push(side);
                self.source.get_full_text(file, side)
            }
        }
        let (first_tx, first_rx) = mpsc::channel();
        let (second_tx, second_rx) = mpsc::channel();
        let first = Arc::new(TrackedLoader {
            calls: Mutex::new(vec![]),
            source: Loader(Mutex::new(first_rx)),
        });
        let second = Arc::new(TrackedLoader {
            calls: Mutex::new(vec![]),
            source: Loader(Mutex::new(second_rx)),
        });
        let review = pinned_alpha_source_review(800);
        let key = review.files[0].key.clone();
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                highlight: false,
                ..Default::default()
            },
        );
        assert!(app.install_source_loader(&key, first.clone()));
        app.toggle_source_gap_for_file(&key, 0).unwrap();
        app.with_state(|state| {
            assert!(matches!(
                app.options
                    .source_presentation
                    .status(&state.changeset().files[0]),
                Some(workdeck_review::ReviewSourceStatus::Loading)
            ))
        });

        app.reload(pinned_alpha_source_review(900));
        assert!(app.install_source_loader(&key, second.clone()));
        let selection = app.with_state(|state| state.selection());
        assert!(app.expanded_gaps.is_empty());
        app.with_state(|state| {
            assert!(
                app.options
                    .source_presentation
                    .status(&state.changeset().files[0])
                    .is_none()
            )
        });
        first_tx.send("first\n".into()).unwrap();
        drain_one(&mut app);
        app.with_state(|state| {
            assert!(
                app.options
                    .source_presentation
                    .status(&state.changeset().files[0])
                    .is_none()
            )
        });
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert!(app.pending_source_reveal.is_none());
        app.toggle_source_gap_for_file(&key, 0).unwrap();
        second_tx.send("second\n".into()).unwrap();
        drain_one(&mut app);
        app.with_state(|state| {
            assert_eq!(
                app.options
                    .source_presentation
                    .status(&state.changeset().files[0]),
                Some(&workdeck_review::ReviewSourceStatus::Loaded {
                    text: "second\n".into()
                })
            )
        });
        assert_eq!(*first.calls.lock().unwrap(), [ReviewSide::New]);
        assert_eq!(*second.calls.lock().unwrap(), [ReviewSide::New]);
    }

    #[test]
    fn stale_alpha_source_rejection_logs_context_without_repopulating_review() {
        const CHILD: &str = "WORKDECK_TEST_STALE_ALPHA_SOURCE_REJECTION";
        if std::env::var_os(CHILD).is_none() {
            // Capture the real controller's stderr without redirecting the
            // process-wide output of other concurrently running tests.
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "source_controller::tests::stale_alpha_source_rejection_logs_context_without_repopulating_review", "--nocapture"])
                .env(CHILD, "1").output().unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                output.status.success(),
                "{}\n{stderr}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert!(
                stderr.contains("ignored stale new source load failure"),
                "{stderr}"
            );
            assert!(stderr.contains("alpha.ts"), "{stderr}");
            assert!(stderr.contains("(alpha)"), "{stderr}");
            assert!(stderr.contains("stale failure"), "{stderr}");
            assert_eq!(
                stderr
                    .lines()
                    .filter(|line| line.contains("ignored stale new source load failure"))
                    .count(),
                1
            );
            return;
        }
        struct RejectedLoader(Mutex<mpsc::Receiver<()>>);
        impl ReviewSourceLoader for RejectedLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                assert_eq!(side, ReviewSide::New);
                self.0
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                Err(ReviewSourceLoadError::Unavailable("stale failure".into()))
            }
        }
        let (reject, deferred) = mpsc::channel();
        let review = pinned_alpha_source_review(800);
        let key = review.files[0].key.clone();
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                highlight: false,
                ..Default::default()
            },
        );
        assert!(app.install_source_loader(&key, Arc::new(RejectedLoader(Mutex::new(deferred)))));
        app.toggle_source_gap_for_file(&key, 0).unwrap();
        app.reload(pinned_alpha_source_review(900));
        let (next_sender, next_receiver) = mpsc::channel();
        next_sender.send("second\n".into()).unwrap();
        assert!(app.install_source_loader(&key, Arc::new(Loader(Mutex::new(next_receiver)))));
        let selection = app.with_state(|state| state.selection());
        reject.send(()).unwrap();
        drain_one(&mut app);
        app.with_state(|state| {
            assert!(
                app.options
                    .source_presentation
                    .status(&state.changeset().files[0])
                    .is_none()
            )
        });
        assert!(app.expanded_gaps.is_empty());
        assert!(app.pending_source_reveal.is_none());
        assert_eq!(app.with_state(|state| state.selection()), selection);
    }
}
