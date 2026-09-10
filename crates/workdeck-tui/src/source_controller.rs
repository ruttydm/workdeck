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
            let loader: Arc<dyn ReviewSourceLoader> = Arc::new(VcsSourceLoader(capability));
            let retained = file.source_attested
                && self
                    .source_loaders
                    .get(&file.key)
                    .is_some_and(|binding| binding.identity == file.source_identity);
            if retained {
                self.source_loaders.insert(
                    file.key.clone(),
                    SourceLoaderBinding {
                        identity: file.source_identity.clone(),
                        loader,
                    },
                );
            } else {
                self.bind_source_loader(file, loader);
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
        self.bind_source_loader(file, loader);
        true
    }

    fn bind_source_loader(&mut self, file: &DiffFile, loader: Arc<dyn ReviewSourceLoader>) {
        self.source_requests
            .retire(&BTreeSet::from([file.key.clone()]));
        self.source_loaders.insert(
            file.key.clone(),
            SourceLoaderBinding {
                identity: file.source_identity.clone(),
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
        let mut changeset = workdeck_diff::changeset_from_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3 +3 @@\n-old\n+new\n",
            "lazy",
            "lazy",
            "lazy",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        );
        changeset.files[0].set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: Some("one".into()),
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
        app.toggle_source_gap_for_file(&key, 0);
        app.toggle_source_gap_for_file(&key, 1);
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
}
