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
        // A fresh host reader does not invalidate completed text for the same
        // content identity. VCS bindings have their own authority checks and
        // deliberately do not use this retention path.
        let retain_loaded = self.source_loaders.get(file_key).is_some_and(|binding| {
            binding.vcs_runtime_identity.is_none()
                && binding.identity.is_some()
                && binding.identity == file.source_identity
        }) && matches!(
            self.options.source_presentation.status(file),
            Some(workdeck_review::ReviewSourceStatus::Loaded { .. })
        );
        if retain_loaded {
            self.source_loaders.insert(
                file.key.clone(),
                SourceLoaderBinding {
                    identity: file.source_identity.clone(),
                    vcs_runtime_identity: None,
                    loader,
                },
            );
            return true;
        }
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
    fn extension_reveal_reaches_loaded_source_outside_original_hunk() {
        let (mut app, sender) = setup();
        app.toggle_source_gap();
        sender
            .send("first-source-line\nsecond-source-line\nnew\n".into())
            .unwrap();
        drain_one(&mut app);
        let id = app.with_state(|state| state.changeset().files[0].runtime_id.clone());
        assert!(app.has_measured_review_line(0, ReviewSide::New, 1));
        app.reveal_extension_review_line("probe", &id, ReviewSide::New, 3);
        app.status = None;
        app.reveal_extension_review_line("probe", &id, ReviewSide::New, 1);
        assert!(app.status.is_none(), "{:?}", app.status);
        let cursor = app.current_review_line_cursor().unwrap().target;
        assert_eq!(cursor.side, ReviewSide::New);
        assert_eq!(cursor.line, 1);
        app.reveal_extension_review_line("probe", &id, ReviewSide::New, 3);
        app.toggle_source_gap();
        assert!(!app.has_measured_review_line(0, ReviewSide::New, 1));
        let selection = app.with_state(|state| state.selection());
        let scroll = app.scroll;
        app.reveal_extension_review_line("probe", &id, ReviewSide::New, 1);
        assert!(app.status.is_some());
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert_eq!(app.scroll, scroll);
        assert!(app.expanded_gaps.is_empty());
    }

    #[test]
    fn alpha_catalog_commands_toggle_notes_start_draft_and_expand_next_gap() {
        let mut review = pinned_two_hunk_alpha_review();
        review.files[0].set_source_capability(Some(workdeck_core::SourceCapabilityIdentity {
            cache_key: None,
        }));
        review.refresh_review_identities();
        let key = review.files[0].key.clone();
        let source = (1..=12)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>()
            .replace("line1 = 1;", "line1 = 100;")
            .replace("line12 = 12;", "line12 = 1200;");
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                agent_notes: false,
                ..Default::default()
            },
        );
        let (sender, receiver) = mpsc::channel();
        assert!(app.install_source_loader(&key, Arc::new(Loader(Mutex::new(receiver)))));
        assert!(app.execute_extension_review_command("workdeck.view.toggleAgentNotes", 1));
        assert!(app.options.agent_notes);
        assert!(app.execute_extension_review_command("workdeck.view.toggleAgentNotes", 1));
        assert!(!app.options.agent_notes);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(0)
        );
        assert!(app.execute_extension_review_command("workdeck.review.startNote", 1));
        let target = app.note_composer.as_ref().unwrap().target;
        assert_eq!(target.file_index, 0);
        assert_eq!(target.hunk_index, 0);
        assert_eq!(target.side, ReviewSide::New);
        assert_eq!(target.line, 1);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.note_composer.is_none());
        assert_eq!(app.focus, Focus::Review);
        assert!(app.with_state(|state| state.comments().is_empty()));
        assert!(app.execute_extension_review_command("workdeck.review.toggleHunkGap", 1));
        sender.send(source).unwrap();
        drain_one(&mut app);
        assert_eq!(app.expanded_gaps, BTreeSet::from([(key, 1)]));
    }

    #[test]
    fn alpha_null_source_sets_error_status() {
        assert_alpha_source_error(false);
    }

    #[test]
    fn alpha_rejected_source_sets_error_and_logs_file_context() {
        const CHILD: &str = "WORKDECK_TEST_ALPHA_SOURCE_REJECTION";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "source_controller::tests::alpha_rejected_source_sets_error_and_logs_file_context", "--nocapture"])
                .env(CHILD, "1").output().unwrap();
            let stderr = String::from_utf8(output.stderr).unwrap();
            assert!(
                output.status.success(),
                "{}\n{stderr}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert!(stderr.contains("alpha.ts"), "{stderr}");
            assert!(stderr.contains("(alpha)"), "{stderr}");
            assert!(stderr.contains("source unavailable"), "{stderr}");
            return;
        }
        assert_alpha_source_error(true);
    }

    fn assert_alpha_source_error(reject: bool) {
        struct ErrorLoader(bool);
        impl ReviewSourceLoader for ErrorLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                _: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                if self.0 {
                    Err(ReviewSourceLoadError::Unavailable(
                        "source unavailable".into(),
                    ))
                } else {
                    Ok(None)
                }
            }
        }
        let review = pinned_alpha_source_review(800);
        let file = review.files[0].clone();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        assert!(app.install_source_loader(&file.key, Arc::new(ErrorLoader(reject))));
        app.toggle_source_gap_for_file(&file.key, 0).unwrap();
        drain_one(&mut app);
        assert_eq!(
            app.options.source_presentation.status(&file),
            Some(&workdeck_review::ReviewSourceStatus::Error { reason: None })
        );
    }

    #[test]
    fn selected_alpha_gap_reads_new_side_once() {
        struct SideLoader {
            calls: Mutex<Vec<ReviewSide>>,
            text: String,
        }
        impl ReviewSourceLoader for SideLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                self.calls.lock().unwrap().push(side);
                Ok((side == ReviewSide::New).then(|| self.text.clone()))
            }
        }
        let before = (1..=30)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let after = before.replace("line 5\n", "line 5 changed\n");
        let mut review = pinned_review_from_text("alpha", "alpha.ts", &before, &after);
        review.files[0].language = None;
        review.refresh_review_identities();
        let key = review.files[0].key.clone();
        let loader = Arc::new(SideLoader {
            calls: Mutex::new(vec![]),
            text: after,
        });
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        assert!(app.install_source_loader(&key, loader.clone()));
        app.toggle_source_gap();
        drain_one(&mut app);
        assert!(app.expanded_gaps.contains(&(key, 0)));
        assert_eq!(*loader.calls.lock().unwrap(), [ReviewSide::New]);
    }

    #[test]
    fn alpha_gap_reports_too_large_source_status() {
        struct TooLargeLoader;
        impl ReviewSourceLoader for TooLargeLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                _: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                Err(ReviewSourceLoadError::TooLarge)
            }
        }
        let review = pinned_alpha_source_review(800);
        let file = review.files[0].clone();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        assert!(app.install_source_loader(&file.key, Arc::new(TooLargeLoader)));
        app.toggle_source_gap_for_file(&file.key, 0).unwrap();
        drain_one(&mut app);
        assert_eq!(
            app.options.source_presentation.status(&file),
            Some(&workdeck_review::ReviewSourceStatus::Error {
                reason: Some(workdeck_review::ReviewSourceErrorReason::TooLarge),
            })
        );
    }

    #[test]
    fn alpha_gap_reopening_reuses_first_read() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct CountingLoader(AtomicUsize);
        impl ReviewSourceLoader for CountingLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                let count = self.0.fetch_add(1, Ordering::SeqCst) + 1;
                Ok((side == ReviewSide::New).then(|| format!("read-{count}\n")))
            }
        }
        let review = pinned_alpha_source_review(800);
        let file = review.files[0].clone();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let loader = Arc::new(CountingLoader(AtomicUsize::new(0)));
        assert!(app.install_source_loader(&file.key, loader.clone()));
        app.toggle_source_gap_for_file(&file.key, 0).unwrap();
        drain_one(&mut app);
        let first_calls = loader.0.load(Ordering::SeqCst);
        app.toggle_source_gap_for_file(&file.key, 0).unwrap();
        app.toggle_source_gap_for_file(&file.key, 0).unwrap();
        assert!(matches!(
            app.options.source_presentation.status(&file),
            Some(workdeck_review::ReviewSourceStatus::Loaded { text }) if text == "read-1\n"
        ));
        assert_eq!(loader.0.load(Ordering::SeqCst), first_calls);
    }

    #[test]
    fn alpha_gap_without_reader_does_not_expand_or_load() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let file = review.files[0].clone();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let selection = app.with_state(|state| state.selection());
        app.toggle_source_gap_for_file(&file.key, 0).unwrap();
        assert!(app.expanded_gaps.is_empty());
        assert!(app.options.source_presentation.status(&file).is_none());
        assert!(app.source_loaders.is_empty());
        assert!(app.pending_source_reveal.is_none());
        assert_eq!(app.poll_source_requests(), 0);
        assert_eq!(app.with_state(|state| state.selection()), selection);
    }

    #[test]
    fn alpha_gap_toggle_loads_exact_source_and_collapses() {
        struct AlphaLoader(mpsc::Sender<ReviewSide>);
        impl ReviewSourceLoader for AlphaLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                self.0.send(side).unwrap();
                Ok((side == ReviewSide::New).then(|| "alpha\nbeta\ngamma\n".into()))
            }
        }
        let review = pinned_alpha_source_review(800);
        let file = review.files[0].clone();
        let key = file.key.clone();
        let mut app = ReviewApp::new(
            review,
            ReviewOptions {
                highlight: false,
                ..Default::default()
            },
        );
        let (calls_tx, calls_rx) = mpsc::channel();
        assert!(app.install_source_loader(&key, Arc::new(AlphaLoader(calls_tx))));
        assert!(calls_rx.try_recv().is_err());
        app.toggle_source_gap_for_file(&key, 0).unwrap();
        drain_one(&mut app);
        assert!(app.expanded_gaps.contains(&(key.clone(), 0)));
        assert_eq!(
            calls_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ReviewSide::New
        );
        assert!(matches!(
            app.options.source_presentation.status(&file),
            Some(workdeck_review::ReviewSourceStatus::Loaded { text, .. })
                if text == "alpha\nbeta\ngamma\n"
        ));
        app.toggle_source_gap_for_file(&key, 0).unwrap();
        assert!(!app.expanded_gaps.contains(&(key, 0)));
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
    fn changed_alpha_reload_retires_source_and_loads_replacement_text() {
        struct TextLoader {
            text: &'static str,
            calls: mpsc::Sender<ReviewSide>,
        }
        impl ReviewSourceLoader for TextLoader {
            fn get_full_text(
                &self,
                _: &DiffFile,
                side: ReviewSide,
            ) -> std::result::Result<Option<String>, ReviewSourceLoadError> {
                self.calls.send(side).unwrap();
                Ok((side == ReviewSide::New).then(|| self.text.into()))
            }
        }
        let initial = pinned_alpha_source_review(800);
        let original_file = initial.files[0].clone();
        let mut app = ReviewApp::new(initial, ReviewOptions::default());
        let (first_tx, first_rx) = mpsc::channel();
        app.install_source_loader(
            &original_file.key,
            Arc::new(TextLoader {
                text: "first\n",
                calls: first_tx,
            }),
        );
        app.toggle_source_gap_for_file(&original_file.key, 0)
            .unwrap();
        drain_one(&mut app);
        assert!(matches!(
            app.options.source_presentation.status(&original_file),
            Some(workdeck_review::ReviewSourceStatus::Loaded { text }) if text == "first\n"
        ));
        assert!(app.expanded_gaps.contains(&(original_file.key.clone(), 0)));
        assert_eq!(
            first_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ReviewSide::New
        );
        let replacement = pinned_alpha_source_review(900);
        let replacement_file = replacement.files[0].clone();
        app.reload(replacement);
        let (second_tx, second_rx) = mpsc::channel();
        app.install_source_loader(
            &replacement_file.key,
            Arc::new(TextLoader {
                text: "second\n",
                calls: second_tx,
            }),
        );
        assert!(
            app.options
                .source_presentation
                .status(&replacement_file)
                .is_none()
        );
        assert!(app.expanded_gaps.is_empty());
        app.toggle_source_gap_for_file(&replacement_file.key, 0)
            .unwrap();
        drain_one(&mut app);
        assert!(matches!(
            app.options.source_presentation.status(&replacement_file),
            Some(workdeck_review::ReviewSourceStatus::Loaded { text }) if text == "second\n"
        ));
        assert_eq!(
            second_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ReviewSide::New
        );
    }

    #[test]
    fn unchanged_alpha_reload_with_fresh_reader_preserves_loaded_gap() {
        let initial = pinned_alpha_source_review(800);
        let file = initial.files[0].clone();
        let mut app = ReviewApp::new(initial, ReviewOptions::default());
        let (first_tx, first_rx) = mpsc::channel();
        assert!(app.install_source_loader(&file.key, Arc::new(Loader(Mutex::new(first_rx)))));
        app.toggle_source_gap_for_file(&file.key, 0).unwrap();
        first_tx.send("first\n".into()).unwrap();
        drain_one(&mut app);
        app.reload(pinned_alpha_source_review(800));
        let (_next_tx, next_rx) = mpsc::channel();
        assert!(app.install_source_loader(&file.key, Arc::new(Loader(Mutex::new(next_rx)))));
        assert!(matches!(
            app.options.source_presentation.status(&file),
            Some(workdeck_review::ReviewSourceStatus::Loaded { text }) if text == "first\n"
        ));
        assert!(app.expanded_gaps.contains(&(file.key, 0)));
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
        pinned_review_from_text("alpha", "alpha.ts", before, after)
    }

    fn pinned_review_from_text(id: &str, path: &str, before: &str, after: &str) -> Changeset {
        let mut file = workdeck_diff::diff_from_file_snapshots(
            workdeck_diff::FileSnapshot {
                cache_key: &format!("{id}:before"),
                contents: before,
                name: path,
            },
            workdeck_diff::FileSnapshot {
                cache_key: &format!("{id}:after"),
                contents: after,
                name: path,
            },
            workdeck_diff::FileComparisonOptions { context_radius: 3 },
        )
        .unwrap();
        file.runtime_id = id.into();
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

    fn pinned_two_hunk_alpha_review() -> Changeset {
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
        review
    }

    #[test]
    fn alpha_cursor_steps_one_row_and_clamps_at_stream_start() {
        let review = pinned_two_hunk_alpha_review();
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
    fn counted_alpha_cursor_movement_reveals_nearest_row_and_carries_hunk_selection() {
        for layout in [crate::LayoutMode::Stack, crate::LayoutMode::Split] {
            let mut app = ReviewApp::new(
                pinned_two_hunk_alpha_review(),
                ReviewOptions {
                    layout,
                    ..Default::default()
                },
            );
            app.review_height.set(8);
            let rows = app.current_review_geometry_rows();
            let cursors = crate::review_line_cursors(&rows);
            let initial = app.current_review_line_cursor().unwrap();
            let index = cursors
                .iter()
                .position(|cursor| *cursor == initial)
                .unwrap();
            app.step_diff_line(4);
            assert_eq!(app.current_review_line_cursor(), Some(cursors[index + 4]));
            let viewport = usize::from(8u16.saturating_sub(app.review_reserved_rows()).max(1));
            assert!(app.current_line_row >= app.scroll);
            assert!(app.current_line_row < app.scroll + viewport);
            let scroll = app.scroll;
            app.step_diff_line(0);
            assert_eq!(app.scroll, scroll);
            for _ in 0..40 {
                app.step_diff_line(1);
            }
            let last = app.current_review_line_cursor().unwrap();
            assert_eq!(last, *cursors.last().unwrap());
            assert_eq!(last.target.hunk_index, 1);
            assert_eq!(
                app.with_state(|state| state.selection().hunk_index),
                Some(1)
            );
            assert!(app.current_line_row >= app.scroll);
            assert!(app.current_line_row < app.scroll + viewport);
            app.step_diff_line(-100);
            assert_eq!(app.current_review_line_cursor(), cursors.first().copied());
            assert_eq!(
                app.with_state(|state| state.selection().hunk_index),
                Some(0)
            );
            assert_eq!(app.scroll, app.current_line_row);
        }
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
    fn hunk_navigation_carries_alpha_cursor_to_selected_hunk() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        assert_eq!(
            app.current_review_line_cursor().unwrap().target.hunk_index,
            0
        );
        app.move_selection(crate::ReviewSelectionScope::Hunk, 1);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
        assert_eq!(
            app.current_review_line_cursor().unwrap().target.hunk_index,
            1
        );
    }

    #[test]
    fn invalid_alpha_attention_marks_leave_review_unchanged() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        let cursor = app.current_review_line_cursor();
        let selection = app.with_state(|state| state.selection());
        for (path, line, start, end, expected) in [
            ("missing.ts", 1, 0, 4, "No diff file matches missing.ts."),
            (
                "alpha.ts",
                9001,
                0,
                4,
                "No new diff hunk in alpha.ts covers line 9001.",
            ),
            (
                "alpha.ts",
                1,
                4,
                4,
                "Highlight range [4, 4) is not a valid [start, end) character range.",
            ),
        ] {
            let error = app
                .session_add_agent_line_highlight(&workdeck_session::HighlightToolInput {
                    target_session: Default::default(),
                    file_path: path.into(),
                    side: ReviewSide::New,
                    line,
                    start,
                    end,
                    tone: None,
                    reveal: None,
                })
                .unwrap_err();
            assert_eq!(error, expected);
            assert!(app.agent_line_highlights.is_empty());
            assert_eq!(app.current_review_line_cursor(), cursor);
            assert_eq!(app.with_state(|state| state.selection()), selection);
        }
    }

    #[test]
    fn session_clear_alpha_human_notes_requires_explicit_opt_in() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        let comment = |summary: &str| workdeck_session::CommentToolInput {
            target_session: Default::default(),
            target: workdeck_session::CommentTargetInput {
                file_path: "alpha.ts".into(),
                hunk_index: Some(0),
                side: None,
                line: None,
                summary: summary.into(),
                rationale: None,
                markup: None,
                author: None,
            },
            reveal: None,
        };
        app.session_add_live_comment(&comment("Agent cleanup note"), "comment-1", false)
            .unwrap();
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Human cleanup note.".into();
        let _ = app.save_note_composer();
        let removed = app.session_remove_live_comment("comment-1").unwrap();
        assert_eq!(removed.comment_id, "comment-1");
        assert!(removed.removed);
        assert_eq!(removed.remaining_comment_count, 1);
        assert_eq!(serde_json::to_value(removed.source).unwrap(), "agent");
        assert!(app.session_live_comment_summaries().is_empty());
        let human = app.with_state(|state| {
            assert_eq!(state.comments().len(), 1);
            assert_eq!(state.comments()[0].summary, "Human cleanup note.");
            assert_eq!(state.comments()[0].file_path.as_deref(), Some("alpha.ts"));
            state.comments()[0].clone()
        });
        let summaries = app.session_review_note_summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].file_path, "alpha.ts");
        assert_eq!(serde_json::to_value(summaries[0].source).unwrap(), "user");
        app.session_add_live_comment(&comment("Default clear agent note"), "comment-2", false)
            .unwrap();
        let cleared = app.session_clear_live_comments(None, None).unwrap();
        assert_eq!(cleared.removed_count, 1);
        assert_eq!(cleared.remaining_comment_count, 1);
        assert_eq!(cleared.removed_live_comment_count, Some(1));
        assert_eq!(cleared.removed_user_note_count, Some(0));
        assert_eq!(cleared.remaining_user_note_count, Some(1));
        assert!(app.session_live_comment_summaries().is_empty());
        app.with_state(|state| assert_eq!(state.comments(), &[human]));
        app.session_add_live_comment(&comment("Inclusive clear agent note"), "comment-3", false)
            .unwrap();
        let cleared = app.session_clear_live_comments(None, Some(true)).unwrap();
        assert_eq!(cleared.removed_count, 2);
        assert_eq!(cleared.remaining_comment_count, 0);
        assert_eq!(cleared.removed_live_comment_count, Some(1));
        assert_eq!(cleared.removed_user_note_count, Some(1));
        assert!(app.session_live_comment_summaries().is_empty());
        assert!(app.with_state(|state| state.comments().is_empty()));
        assert!(app.session_review_note_summaries().is_empty());
    }

    #[test]
    fn reply_inherits_parent_range_anchor_and_resolution() {
        let mut app = ReviewApp::new(pinned_alpha_source_review(800), ReviewOptions::default());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Range parent".into();
        let mut parent = app.save_note_composer().unwrap();
        app.with_state(|state| state.remove_comment(&parent.id));
        parent.anchor.new_range = Some(workdeck_core::LineRange { start: 5, end: 7 });
        parent.resolution = workdeck_review::ReviewNoteResolution::Stale;
        app.with_state(|state| state.add_comment(parent.clone()))
            .unwrap();
        app.saved_note_hover = Some(parent.id.clone());
        app.open_active_note_reply();
        app.note_composer.as_mut().unwrap().body = "Reply".into();
        let reply = app.save_note_composer().unwrap();
        assert_eq!(reply.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(reply.anchor, parent.anchor);
        assert_eq!(reply.resolution, parent.resolution);
    }

    #[test]
    fn reply_save_rejects_parent_from_a_different_file() {
        let mut review = pinned_alpha_source_review(800);
        let beta = pinned_review_from_text("beta", "beta.ts", "before\n", "after\n");
        review.files.extend(beta.files);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Alpha parent".into();
        let parent = app.save_note_composer().unwrap();
        app.saved_note_hover = Some(parent.id.clone());
        app.open_active_note_reply();
        let draft = app.note_composer.as_mut().unwrap();
        draft.body = "Reply".into();
        draft.target.file_index = 1;
        let draft_id = draft.id.clone();
        let revision = app.with_state(|state| state.state_revision());
        assert!(app.save_note_composer().is_none());
        assert_eq!(app.note_composer.as_ref().unwrap().id, draft_id);
        assert_eq!(
            app.status,
            Some(format!(
                "Review note {} belongs to a different file.",
                parent.id
            ))
        );
        app.with_state(|state| {
            assert_eq!(state.comments(), &[parent]);
            assert_eq!(state.state_revision(), revision);
        });
    }

    #[test]
    fn reply_save_rejects_removed_parent_and_retains_draft() {
        let mut app = ReviewApp::new(pinned_alpha_source_review(800), ReviewOptions::default());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Parent".into();
        let parent = app.save_note_composer().unwrap();
        app.saved_note_hover = Some(parent.id.clone());
        app.open_active_note_reply();
        app.note_composer.as_mut().unwrap().body = "Reply".into();
        let draft_id = app.note_composer.as_ref().unwrap().id.clone();
        app.session_remove_live_comment(&parent.id).unwrap();
        let revision = app.with_state(|state| state.state_revision());
        assert!(app.save_note_composer().is_none());
        assert_eq!(app.note_composer.as_ref().unwrap().body, "Reply");
        assert_eq!(app.note_composer.as_ref().unwrap().id, draft_id);
        assert_eq!(
            app.status,
            Some(format!(
                "Review note {} is no longer available as a reply parent.",
                parent.id
            ))
        );
        assert!(app.with_state(|state| state.comments().is_empty()));
        assert_eq!(app.with_state(|state| state.state_revision()), revision);
    }

    #[test]
    fn blank_note_edit_retains_draft_and_original_note() {
        let mut app = ReviewApp::new(pinned_alpha_source_review(800), ReviewOptions::default());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Original".into();
        let original = app.save_note_composer_at(1_700_000_000_000).unwrap();
        app.saved_note_hover = Some(original.id.clone());
        app.open_active_note_edit();
        app.note_composer.as_mut().unwrap().body = " \n\t ".into();
        let revision = app.with_state(|state| state.state_revision());
        assert!(app.save_note_composer_at(1_700_000_001_000).is_none());
        assert_eq!(app.note_composer.as_ref().unwrap().body, " \n\t ");
        assert_eq!(
            app.with_state(|state| state.comments()[0].clone()),
            original
        );
        assert_eq!(app.with_state(|state| state.state_revision()), revision);
        assert_eq!(
            app.status.as_deref(),
            Some("An edited review note cannot be blank; cancel or delete it instead.")
        );
        app.note_composer.as_mut().unwrap().body = "Corrected".into();
        let corrected = app.save_note_composer_at(1_700_000_002_000).unwrap();
        assert_eq!(corrected.id, original.id);
        assert_eq!(corrected.summary, "Corrected");
        assert!(app.note_composer.is_none());
    }

    #[test]
    fn saved_note_timestamps_preserve_creation_across_edits() {
        let mut app = ReviewApp::new(pinned_alpha_source_review(800), ReviewOptions::default());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Original".into();
        let original = app.save_note_composer_at(1_700_000_000_000).unwrap();
        assert_eq!(
            original.created_at.as_deref(),
            Some("2023-11-14T22:13:20.000Z")
        );
        assert_eq!(original.updated_at, None);
        for (time, expected) in [
            (1_700_000_001_123, "2023-11-14T22:13:21.123Z"),
            (1_700_000_002_000, "2023-11-14T22:13:22.000Z"),
        ] {
            app.saved_note_hover = Some(original.id.clone());
            app.open_active_note_edit();
            app.note_composer.as_mut().unwrap().body = "Edited".into();
            let edited = app.save_note_composer_at(time).unwrap();
            assert_eq!(edited.id, original.id);
            assert_eq!(edited.created_at, original.created_at);
            assert_eq!(edited.updated_at.as_deref(), Some(expected));
            assert_eq!(app.with_state(|state| state.comments()[0].clone()), edited);
        }
    }

    #[test]
    fn alpha_keyboard_note_actions_restore_the_note_line_target() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.open_note_composer();
        let target = app.note_composer.as_ref().unwrap().target;
        app.note_composer.as_mut().unwrap().body = "Root note".into();
        let root = app.save_note_composer().unwrap();
        for action in [
            super::super::AppCommandAction::EditActiveNote,
            super::super::AppCommandAction::ReplyToActiveNote,
        ] {
            app.review_width.set(80);
            app.review_height.set(8);
            app.step_diff_line(2);
            assert_ne!(app.current_review_line_cursor().unwrap().target, target);
            app.saved_note_hover = Some(root.id.clone());
            app.scroll = 0;
            app.apply_builtin_command_action(action);
            assert_eq!(app.note_composer.as_ref().unwrap().target, target);
            assert_eq!(app.current_review_line_cursor().unwrap().target, target);
            let rows = app.current_review_geometry_rows();
            let (top, height) = rows.note_bounds[&app.note_composer.as_ref().unwrap().id];
            let viewport = usize::from(
                app.review_height
                    .get()
                    .saturating_sub(app.review_reserved_rows())
                    .max(1),
            );
            assert!(top + height > viewport, "fixture must require scrolling");
            assert_eq!(
                app.scroll,
                (top + height - viewport).min(rows.lines.len().saturating_sub(viewport))
            );
            app.handle_key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
    }

    #[test]
    fn alpha_mouse_targeted_note_drafts_preserve_cursor_and_scroll() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.review_width.set(80);
        app.review_height.set(8);
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Root note".into();
        let root = app.save_note_composer().unwrap();
        app.step_diff_line(2);
        let cursor = app.current_review_line_cursor().map(|cursor| cursor.target);
        assert!(cursor.is_some());
        let scroll = app.scroll;
        let selection = app.with_state(|state| state.selection());

        app.saved_note_hover = Some(root.id.clone());
        app.open_active_note_edit();
        assert!(app.note_composer.is_some());
        assert_eq!(
            app.current_review_line_cursor().map(|cursor| cursor.target),
            cursor
        );
        assert_eq!(app.scroll, scroll);
        assert_eq!(app.with_state(|state| state.selection()), selection);

        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(app.note_composer.is_none());
        app.saved_note_hover = Some(root.id);
        app.open_active_note_reply();
        assert!(app.note_composer.is_some());
        assert_eq!(
            app.current_review_line_cursor().map(|cursor| cursor.target),
            cursor
        );
        assert_eq!(app.scroll, scroll);
        assert_eq!(app.with_state(|state| state.selection()), selection);
    }

    #[test]
    fn alpha_saved_notes_edit_in_place_and_form_nested_reply_chains() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Root note".into();
        let root = app.save_note_composer().unwrap();

        app.saved_note_hover = Some(root.id.clone());
        app.open_active_note_edit();
        app.note_composer.as_mut().unwrap().body = "Edited root note".into();
        let edited = app.save_note_composer().unwrap();
        assert_eq!(edited.id, root.id);
        assert_eq!(edited.summary, "Edited root note");

        app.saved_note_hover = Some(root.id.clone());
        app.open_active_note_reply();
        app.note_composer.as_mut().unwrap().body = "Child reply".into();
        let child = app.save_note_composer().unwrap();

        app.saved_note_hover = Some(child.id.clone());
        app.open_active_note_reply();
        app.note_composer.as_mut().unwrap().body = "Grandchild reply".into();
        let grandchild = app.save_note_composer().unwrap();

        let notes = app.session_review_note_summaries();
        assert_eq!(
            notes
                .iter()
                .map(|note| (note.body.as_str(), note.parent_id.as_deref()))
                .collect::<Vec<_>>(),
            vec![
                ("Edited root note", None),
                ("Child reply", Some(root.id.as_str())),
                ("Grandchild reply", Some(child.id.as_str())),
            ]
        );
        assert_eq!(
            notes
                .iter()
                .map(|note| &note.note_id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            3
        );
        assert_eq!(grandchild.parent_id.as_deref(), Some(child.id.as_str()));
        let revision = app.with_state(|state| state.state_revision());
        let error = app.session_remove_live_comment(&root.id).unwrap_err();
        assert!(
            error.contains("cannot be removed while it has replies"),
            "{error}"
        );
        assert_eq!(app.session_review_note_summaries(), notes);
        assert_eq!(app.with_state(|state| state.state_revision()), revision);
        assert!(app.session_remove_live_comment(&child.id).is_err());
        app.session_remove_live_comment(&grandchild.id).unwrap();
        app.session_remove_live_comment(&child.id).unwrap();
        app.session_remove_live_comment(&root.id).unwrap();
        assert!(app.session_review_note_summaries().is_empty());
    }

    #[test]
    fn duplicate_alpha_draft_save_persists_once_and_next_draft_has_unique_id() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Save me once.".into();
        let saved = app.save_note_composer_at(1_700_000_000_000).unwrap();
        assert_eq!(saved.id, "user:1700000000000-1");
        assert!(app.save_note_composer_at(1_700_000_000_000).is_none());
        let first = app.with_state(|state| {
            assert_eq!(state.comments().len(), 1);
            assert_eq!(state.comments()[0].summary, "Save me once.");
            assert_eq!(state.comments()[0], saved);
            state.comments()[0].id.clone()
        });
        assert_eq!(first, "user:1700000000000-1");
        assert!(app.note_composer.is_none());
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Save me too.".into();
        let follow_up = app.save_note_composer_at(1_700_000_000_000).unwrap();
        assert_eq!(follow_up.id, "user:1700000000000-2");
        app.with_state(|state| {
            assert_eq!(state.comments().len(), 2);
            assert_eq!(state.comments()[1].summary, "Save me too.");
            assert_ne!(state.comments()[1].id, first);
            assert_eq!(state.comments()[1].id, "user:1700000000000-2");
        });
    }

    #[test]
    fn new_draft_does_not_collide_with_existing_user_note_id() {
        let mut original = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        original.open_note_composer();
        original.note_composer.as_mut().unwrap().body = "Existing note".into();
        let _ = original.save_note_composer_at(1_700_000_000_000);
        let saved = original.with_state(|state| state.comments()[0].clone());
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        app.with_state(|state| state.add_comment(saved.clone()))
            .unwrap();
        app.open_note_composer();
        assert_ne!(app.note_composer.as_ref().unwrap().id, saved.id);
        app.note_composer.as_mut().unwrap().body = "New note".into();
        let _ = app.save_note_composer_at(1_700_000_000_000);
        app.with_state(|state| {
            assert_eq!(state.comments().len(), 2);
            assert_eq!(state.comments()[0], saved);
            assert_eq!(state.comments()[1].summary, "New note");
            assert_eq!(state.comments()[1].id, "user:1700000000000-2");
        });
    }

    #[test]
    fn default_alpha_attention_mark_does_not_move_viewport_or_selection() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        app.review_height.set(8);
        app.step_diff_line(6);
        let cursor = app.current_review_line_cursor();
        let selection = app.with_state(|state| state.selection());
        let scroll = app.scroll;
        let result = app
            .session_add_agent_line_highlight(&workdeck_session::HighlightToolInput {
                target_session: Default::default(),
                file_path: "alpha.ts".into(),
                side: ReviewSide::New,
                line: 1,
                start: 13,
                end: 18,
                tone: None,
                reveal: None,
            })
            .unwrap();
        assert_eq!(result.hunk_index, 0);
        assert_eq!(result.file_mark_count, 1);
        assert_eq!(result.revealed, None);
        assert_eq!(
            serde_json::to_value(result.tone).unwrap(),
            serde_json::json!("match")
        );
        assert_eq!(app.scroll, scroll);
        assert_eq!(app.current_review_line_cursor(), cursor);
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert_eq!(
            serde_json::to_value(app.agent_line_highlights.get("alpha").unwrap()).unwrap(),
            serde_json::json!([
                {"side":"new", "line":1, "start":13, "end":18, "tone":"match"}
            ])
        );
    }

    #[test]
    fn unchanged_alpha_reload_rekeys_attention_marks_and_clear_counts() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review.clone(), ReviewOptions::default());
        app.session_add_agent_line_highlight(&workdeck_session::HighlightToolInput {
            target_session: Default::default(),
            file_path: "alpha.ts".into(),
            side: ReviewSide::New,
            line: 8,
            start: 0,
            end: 6,
            tone: None,
            reveal: None,
        })
        .unwrap();
        let original = app.agent_line_highlights.get("alpha").unwrap().to_vec();
        assert_eq!(original.len(), 1);
        assert_eq!(
            serde_json::to_value(&original).unwrap(),
            serde_json::json!([
                {"side":"new", "line":8, "start":0, "end":6, "tone":"match"}
            ])
        );
        review.files[0].runtime_id = "alpha-reloaded".into();
        review.refresh_review_identities();
        app.reload(review);
        assert!(app.agent_line_highlights.get("alpha").is_none());
        assert_eq!(
            app.agent_line_highlights.get("alpha-reloaded"),
            Some(original.as_slice())
        );
        let cleared = app.session_clear_agent_line_highlights(None).unwrap();
        assert_eq!(cleared.removed_count, 1);
        assert_eq!(cleared.remaining_count, 0);
        assert_eq!(cleared.file_path, None);
        assert!(app.agent_line_highlights.is_empty());
    }

    #[test]
    fn changed_alpha_reload_retires_attention_marks() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.session_add_agent_line_highlight(&workdeck_session::HighlightToolInput {
            target_session: Default::default(),
            file_path: "alpha.ts".into(),
            side: ReviewSide::New,
            line: 8,
            start: 0,
            end: 6,
            tone: None,
            reveal: None,
        })
        .unwrap();
        assert_eq!(app.agent_line_highlights.get("alpha").unwrap().len(), 1);
        let mut replacement = pinned_alpha_source_review(900);
        replacement.files[0].set_source_capability(None);
        replacement.refresh_review_identities();
        app.reload(replacement);
        assert!(app.agent_line_highlights.is_empty());
    }

    #[test]
    fn attention_mark_clear_counts_preserve_other_files() {
        let mut review = pinned_two_hunk_alpha_review();
        let before = (1..=30)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let after = before
            .replace("line1 = 1;", "line1 = 100;")
            .replace("line15 = 15;", "line15 = 1500;")
            .replace("line30 = 30;", "line30 = 3000;");
        let mut beta = pinned_review_from_text("beta", "beta.ts", &before, &after)
            .files
            .remove(0);
        beta.set_source_capability(None);
        assert_eq!(beta.hunks.len(), 3);
        review.files.push(beta);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        for (path, line) in [("alpha.ts", 1), ("alpha.ts", 12), ("beta.ts", 1)] {
            app.session_add_agent_line_highlight(&workdeck_session::HighlightToolInput {
                target_session: Default::default(),
                file_path: path.into(),
                side: ReviewSide::New,
                line,
                start: 0,
                end: 4,
                tone: None,
                reveal: None,
            })
            .unwrap();
        }
        let beta_marks = app.agent_line_highlights.get("beta").unwrap().to_vec();
        let cleared = app
            .session_clear_agent_line_highlights(Some("alpha.ts"))
            .unwrap();
        assert_eq!(cleared.removed_count, 2);
        assert_eq!(cleared.remaining_count, 1);
        assert_eq!(cleared.file_path.as_deref(), Some("alpha.ts"));
        assert!(app.agent_line_highlights.get("alpha").is_none());
        assert_eq!(
            app.agent_line_highlights.get("beta"),
            Some(beta_marks.as_slice())
        );
        let cleared = app.session_clear_agent_line_highlights(None).unwrap();
        assert_eq!(cleared.removed_count, 1);
        assert_eq!(cleared.remaining_count, 0);
        assert_eq!(cleared.file_path, None);
        assert!(app.agent_line_highlights.is_empty());
    }

    #[test]
    fn cross_file_beta_line_reveal_resolves_before_another_frame() {
        let mut review =
            pinned_alpha_review_from_text("export const alpha = 1;\n", "export const alpha = 2;\n");
        review.files[0].set_source_capability(None);
        let before = (1..=30)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let after = before
            .replace("line1 = 1;", "line1 = 100;")
            .replace("line15 = 15;", "line15 = 1500;")
            .replace("line30 = 30;", "line30 = 3000;");
        let mut beta = pinned_review_from_text("beta", "beta.ts", &before, &after)
            .files
            .remove(0);
        beta.set_source_capability(None);
        review.files.push(beta);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.review_height.set(12);
        assert_eq!(
            app.current_review_line_cursor().unwrap().target.file_index,
            0
        );
        app.reveal_extension_review_line("probe", "beta", ReviewSide::New, 30);
        let cursor = app.current_review_line_cursor().unwrap();
        assert_eq!(cursor.target.file_index, 1);
        assert_eq!(cursor.target.hunk_index, 2);
        assert_eq!(cursor.target.side, ReviewSide::New);
        assert_eq!(cursor.target.line, 30);
        let selected = app.with_state(|state| state.selection());
        assert_eq!(selected.file_index, 1);
        assert_eq!(selected.hunk_index, Some(2));
        let viewport = usize::from(12u16.saturating_sub(app.review_reserved_rows()).max(1));
        assert!(cursor.row >= app.scroll && cursor.row < app.scroll + viewport);
        assert!(app.status.is_none());
    }

    #[test]
    fn measured_line_lookup_excludes_gaps_hidden_files_and_disabled_cursor() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        assert!(app.has_measured_review_line(0, ReviewSide::New, 12));
        assert!(app.has_measured_review_line(0, ReviewSide::Old, 1));
        assert!(!app.has_measured_review_line(0, ReviewSide::New, 6));
        assert!(!app.has_measured_review_line(1, ReviewSide::New, 12));
        assert!(!app.has_measured_review_line(0, ReviewSide::New, 9001));
        app.filter = "beta".into();
        assert!(!app.has_measured_review_line(0, ReviewSide::New, 12));
        app.filter.clear();
        app.options.cursor_line = crate::CursorLineMode::Off;
        assert!(!app.has_measured_review_line(0, ReviewSide::New, 12));
    }

    #[test]
    fn alpha_session_navigation_without_cursor_rows_reports_hunk_fallback() {
        let mut app = ReviewApp::new(
            pinned_two_hunk_alpha_review(),
            ReviewOptions {
                cursor_line: crate::CursorLineMode::Off,
                ..Default::default()
            },
        );
        let result = app
            .session_navigate_to_location(&workdeck_session::NavigateToHunkToolInput {
                target_session: Default::default(),
                file_path: Some("alpha.ts".into()),
                hunk_index: None,
                side: Some(ReviewSide::New),
                line: Some(12),
                comment_direction: None,
            })
            .unwrap();
        assert_eq!(result.hunk_index, 1);
        assert_eq!(
            result.revealed,
            Some(workdeck_session::RevealedTarget::Hunk)
        );
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
    }

    #[test]
    fn extension_line_reveal_reads_current_rows_after_reload() {
        let before = (1..=30)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let after = before
            .replace("line1 = 1;", "line1 = 100;")
            .replace("line15 = 15;", "line15 = 1500;")
            .replace("line30 = 30;", "line30 = 3000;");
        let mut review = pinned_alpha_review_from_text(&before, &after);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        assert_eq!(review.files[0].hunks.len(), 3);
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        app.review_height.set(12);
        // A caller retained before reload must resolve against the current document.
        let reveal = ReviewApp::reveal_extension_review_line;
        app.reload(review);
        let reload_status = app.status.clone();
        reveal(&mut app, "probe", "alpha", ReviewSide::New, 30);
        let cursor = app.current_review_line_cursor().unwrap();
        assert_eq!(cursor.target.hunk_index, 2);
        assert_eq!(cursor.target.side, ReviewSide::New);
        assert_eq!(cursor.target.line, 30);
        let selection = app.with_state(|state| state.selection());
        assert_eq!(selection.hunk_index, Some(2));
        assert_eq!(selection.line, Some(30));
        assert_eq!(app.status, reload_status);
        let viewport = usize::from(12u16.saturating_sub(app.review_reserved_rows()).max(1));
        assert!(cursor.row >= app.scroll && cursor.row < app.scroll + viewport);
        reveal(&mut app, "probe", "alpha", ReviewSide::New, 999);
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert_eq!(app.current_review_line_cursor(), Some(cursor));
        assert!(app.status.is_some());
        assert_ne!(app.status, reload_status);
    }

    #[test]
    fn extension_line_reveal_cannot_select_a_filter_hidden_file() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        app.filter = "beta".into();
        let selection = app.with_state(|state| state.selection());
        let scroll = app.scroll;
        assert!(app.current_review_line_cursor().is_none());
        app.reveal_extension_review_line("probe", "alpha", ReviewSide::New, 12);
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert_eq!(app.scroll, scroll);
        assert!(app.current_review_line_cursor().is_none());
        assert!(app.status.is_some());
        app.filter.clear();
        app.reveal_extension_review_line("probe", "alpha", ReviewSide::New, 12);
        let cursor = app.current_review_line_cursor().unwrap().target;
        assert_eq!(cursor.hunk_index, 1);
        assert_eq!(cursor.side, ReviewSide::New);
        assert_eq!(cursor.line, 12);
    }

    #[test]
    fn cursor_off_line_reveal_uses_containing_hunk_placement() {
        let options = ReviewOptions {
            cursor_line: crate::CursorLineMode::Off,
            ..Default::default()
        };
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), options.clone());
        let mut expected = ReviewApp::new(pinned_two_hunk_alpha_review(), options);
        app.review_height.set(8);
        expected.review_height.set(8);
        expected.select_extension_review_hunk("probe", "alpha", 1);
        app.reveal_extension_review_line("probe", "alpha", ReviewSide::New, 12);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
        assert_eq!(app.scroll, expected.scroll);
        assert_eq!(
            app.with_state(|state| state.selection()),
            expected.with_state(|state| state.selection())
        );
    }

    #[test]
    fn filter_hides_alpha_cursor_and_clearing_restores_it() {
        let mut review =
            pinned_alpha_review_from_text("export const alpha = 1;\n", "export const alpha = 2;\n");
        let mut beta = workdeck_diff::diff_from_file_snapshots(
            workdeck_diff::FileSnapshot {
                cache_key: "beta:before",
                contents: "export const beta = 1;\n",
                name: "beta.ts",
            },
            workdeck_diff::FileSnapshot {
                cache_key: "beta:after",
                contents: "export const betaValue = 2;\n",
                name: "beta.ts",
            },
            workdeck_diff::FileComparisonOptions { context_radius: 3 },
        )
        .unwrap();
        beta.runtime_id = "beta".into();
        beta.language = Some("typescript".into());
        beta.patch.clear();
        for source in beta
            .sources
            .old
            .iter_mut()
            .chain(beta.sources.new.iter_mut())
        {
            source.origin = workdeck_core::SourceOrigin::DiffMetadata;
            source.attested = false;
        }
        beta.set_sources(beta.sources.clone());
        review.files[0].set_source_capability(None);
        review.files.push(beta);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let initial = app.current_review_line_cursor().unwrap();
        assert_eq!(initial.target.file_index, 0);
        let selected = app.with_state(|state| state.selection());
        app.focus = crate::Focus::Filter;
        for character in "beta".chars() {
            assert!(app.handle_filter_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            )));
        }
        assert!(app.current_review_line_cursor().is_none());
        assert_eq!(app.with_state(|state| state.selection()), selected);
        assert!(app.handle_filter_key(&crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        )));
        assert_eq!(app.current_review_line_cursor(), Some(initial));
    }

    #[test]
    fn alpha_user_note_save_projection_and_removal() {
        let mut review = pinned_review_from_text(
            "alpha",
            "alpha.ts",
            "export const alpha = 1;\n",
            "export const alpha = 2;\n",
        );
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.review_width.set(80);
        app.review_height.set(4);
        app.open_note_composer();
        app.note_composer.as_mut().unwrap().body = "Please add a regression test.".into();
        let saved = app.save_note_composer().unwrap();
        assert!(saved.id.starts_with("user:"));
        let id = app.with_state(|state| {
            assert_eq!(state.comments().len(), 1);
            assert_eq!(state.comments()[0], saved);
            saved.id.clone()
        });
        let notes = app.session_review_note_summaries();
        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.note_id, id);
        assert_eq!(serde_json::to_value(note.source).unwrap(), "user");
        assert_eq!(note.file_path, "alpha.ts");
        assert_eq!(note.hunk_index, Some(0));
        assert_eq!(note.new_range, Some([1, 1]));
        assert_eq!(note.body, "Please add a regression test.");
        assert!(note.editable);
        let removed = app.session_remove_live_comment(&id).unwrap();
        assert_eq!(removed.comment_id, id);
        assert!(removed.removed);
        assert_eq!(removed.remaining_comment_count, 0);
        assert_eq!(serde_json::to_value(removed.source).unwrap(), "user");
        assert!(app.with_state(|state| state.comments().is_empty()));
        assert!(app.session_review_note_summaries().is_empty());
    }

    #[test]
    fn alpha_sidecar_annotation_is_exposed_as_noneditable_ai_note() {
        let mut review = pinned_review_from_text(
            "alpha",
            "alpha.ts",
            "export const alpha = 1;\n",
            "export const alpha = 2;\n",
        );
        review.files[0].set_source_capability(None);
        review.files[0].agent = Some(
            serde_json::from_value(serde_json::json!({
                "path": "alpha.ts",
                "annotations": [{
                    "id": "ai:1", "source": "ai",
                    "summary": "Prefer a named constant.",
                    "rationale": "It documents the changed value.",
                    "new_range": {"start": 1, "end": 1},
                    "author": "assistant"
                }]
            }))
            .unwrap(),
        );
        review.refresh_review_identities();
        let app = ReviewApp::new(review, ReviewOptions::default());
        app.review_width.set(80);
        app.review_height.set(4);
        let notes = app.session_review_note_summaries();
        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.note_id, "ai:1");
        assert_eq!(serde_json::to_value(note.source).unwrap(), "ai");
        assert_eq!(note.file_path, "alpha.ts");
        assert_eq!(note.new_range, Some([1, 1]));
        assert_eq!(
            note.body,
            "Prefer a named constant.\n\nIt documents the changed value."
        );
        assert_eq!(note.author.as_deref(), Some("assistant"));
        assert!(!note.editable);
    }

    #[test]
    fn batch_first_reveal_repositions_an_already_selected_hunk() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        app.review_height.set(4);
        app.with_state(|state| state.select_hunk(0, 1)).unwrap();
        let target = workdeck_session::CommentTargetInput {
            file_path: "alpha.ts".into(),
            hunk_index: Some(1),
            side: None,
            line: None,
            summary: "Reveal selected hunk".into(),
            rationale: None,
            markup: None,
            author: None,
        };
        app.session_add_live_comment_batch(std::slice::from_ref(&target), "first", false)
            .unwrap();
        app.scroll_to_selection();
        let expected = app.scroll;
        assert!(expected > 0);
        app.scroll = 0;
        app.session_add_live_comment_batch(std::slice::from_ref(&target), "second", true)
            .unwrap();
        assert_eq!(app.scroll, expected);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
        let input = workdeck_session::CommentToolInput {
            target_session: Default::default(),
            target,
            reveal: Some(false),
        };
        app.scroll = 0;
        app.session_add_live_comment(&input, "single-no-reveal", false)
            .unwrap();
        assert_eq!(app.scroll, 0);
        let input = workdeck_session::CommentToolInput {
            reveal: Some(true),
            ..input
        };
        app.session_add_live_comment(&input, "single-reveal", true)
            .unwrap();
        assert_eq!(app.scroll, expected);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
    }

    #[test]
    fn alpha_comment_batch_preserves_order_and_reveals_first_hunk() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        let target = |hunk_index, summary: &str| workdeck_session::CommentTargetInput {
            file_path: "alpha.ts".into(),
            hunk_index: Some(hunk_index),
            side: None,
            line: None,
            summary: summary.into(),
            rationale: None,
            markup: None,
            author: None,
        };
        let result = app
            .session_add_live_comment_batch(
                &[target(1, "Later hunk note"), target(0, "Earlier hunk note")],
                "request-1",
                true,
            )
            .unwrap();
        assert_eq!(
            result
                .applied
                .iter()
                .map(|comment| comment.hunk_index)
                .collect::<Vec<_>>(),
            [1, 0]
        );
        assert_eq!(app.with_state(|state| state.comments().len()), 2);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
        assert_eq!(
            app.session_live_comment_summaries()
                .iter()
                .map(|comment| comment.summary.as_str())
                .collect::<Vec<_>>(),
            ["Later hunk note", "Earlier hunk note"]
        );
    }

    #[test]
    fn invalid_alpha_comment_batch_is_atomic() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        let target = |path: &str, summary: &str| workdeck_session::CommentTargetInput {
            file_path: path.into(),
            hunk_index: Some(0),
            side: None,
            line: None,
            summary: summary.into(),
            rationale: None,
            markup: None,
            author: None,
        };
        let selection = app.with_state(|state| state.selection());
        let error = app
            .session_add_live_comment_batch(
                &[
                    target("alpha.ts", "Valid note"),
                    target("missing.ts", "Invalid note"),
                ],
                "request-2",
                true,
            )
            .unwrap_err();
        assert_eq!(error, "No diff file matches missing.ts.");
        assert!(app.with_state(|state| state.comments().is_empty()));
        assert!(app.session_live_comment_summaries().is_empty());
        assert_eq!(app.with_state(|state| state.selection()), selection);
    }

    #[test]
    fn live_beta_comment_updates_annotated_navigation_without_reload() {
        let mut review = pinned_review_from_text(
            "alpha",
            "alpha.ts",
            "export const alpha = 1;\n",
            "export const alpha = 2;\n",
        );
        let mut beta = pinned_review_from_text(
            "beta",
            "beta.ts",
            "export const beta = 1;\n",
            "export const beta = 2;\n",
        );
        review.files.push(beta.files.remove(0));
        for file in &mut review.files {
            file.set_source_capability(None);
        }
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let generation = app.with_state(|state| state.generation());
        assert!(app.session_live_comment_summaries().is_empty());
        app.session_add_live_comment(
            &workdeck_session::CommentToolInput {
                target_session: Default::default(),
                target: workdeck_session::CommentTargetInput {
                    file_path: "beta.ts".into(),
                    hunk_index: None,
                    side: Some(ReviewSide::New),
                    line: Some(1),
                    summary: "Check beta rename".into(),
                    rationale: None,
                    markup: None,
                    author: None,
                },
                reveal: Some(false),
            },
            "comment-1",
            false,
        )
        .unwrap();
        let summaries = app.session_live_comment_summaries();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].summary, "Check beta rename");
        assert!(!app.options.agent_notes);
        app.filter = "alpha".into();
        let selection = app.with_state(|state| state.selection());
        let scroll = app.scroll;
        app.move_selection(ReviewSelectionScope::AnnotatedHunk, 1);
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert_eq!(app.scroll, scroll);
        assert_eq!(app.session_live_comment_summaries().len(), 1);
        app.filter.clear();
        app.move_selection(ReviewSelectionScope::AnnotatedHunk, 1);
        assert!(!app.options.agent_notes);
        app.with_state(|state| {
            assert_eq!(state.selected_file().unwrap().path, "beta.ts");
            assert_eq!(state.selection().hunk_index, Some(0));
            assert_eq!(state.generation(), generation);
        });
        app.session_remove_live_comment("comment-1").unwrap();
        assert!(app.session_live_comment_summaries().is_empty());
        app.select_extension_review_hunk("test", "alpha", 0);
        app.move_selection(ReviewSelectionScope::AnnotatedHunk, 1);
        app.with_state(|state| {
            assert_eq!(state.selected_file().unwrap().path, "alpha.ts");
            assert!(state.comments().is_empty());
            assert_eq!(state.generation(), generation);
        });
    }

    #[test]
    fn counted_alpha_file_navigation_commits_only_final_selection() {
        let mut review = pinned_alpha_source_review(800);
        review.files[0].set_source_capability(None);
        for id in ["beta", "gamma", "delta"] {
            let mut next = pinned_review_from_text(
                id,
                &format!("{id}.ts"),
                &format!("export const {id} = 1;\n"),
                &format!("export const {id} = 2;\n"),
            );
            next.files[0].set_source_capability(None);
            review.files.push(next.files.remove(0));
        }
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        app.review_width.set(80);
        app.review_height.set(4);
        let revision = app.with_state(|state| state.state_revision());
        app.move_selection(ReviewSelectionScope::File, 3);
        app.with_state(|state| {
            assert_eq!(state.selected_file().unwrap().path, "delta.ts");
            assert_eq!(state.selection().hunk_index, Some(0));
            assert_eq!(state.state_revision(), revision + 1);
        });
        let rows = app.current_review_geometry_rows();
        let viewport = usize::from(
            app.review_height
                .get()
                .saturating_sub(app.review_reserved_rows())
                .max(1),
        );
        assert_eq!(
            app.scroll,
            rows.file_body_tops[&3].min(rows.lines.len().saturating_sub(viewport))
        );
        let scroll = app.scroll;
        app.move_selection(ReviewSelectionScope::File, 3);
        assert_eq!(app.with_state(|state| state.state_revision()), revision + 1);
        assert_eq!(app.scroll, scroll);
    }

    #[test]
    fn selection_only_alpha_navigation_retains_document_allocation() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        let initial = app.with_state(|state| state.changeset_snapshot());
        let source_identity = initial.files[0].source_identity.clone();
        let generation = app.with_state(|state| state.generation());
        app.select_extension_review_hunk("test", "alpha", 1);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
        let current = app.with_state(|state| state.changeset_snapshot());
        assert!(Arc::ptr_eq(&initial, &current));
        assert_eq!(current.files[0].source_identity, source_identity);
        assert_eq!(app.with_state(|state| state.generation()), generation);
    }

    #[test]
    fn reload_hunk_clamp_preserves_existing_file_only_selection() {
        let mut initial = pinned_two_hunk_alpha_review();
        initial.files[0].hunks.clear();
        initial.refresh_review_identities();
        let mut app = ReviewApp::new(initial, ReviewOptions::default());
        app.with_state(|state| state.select_file(0)).unwrap();
        assert_eq!(app.with_state(|state| state.selection().hunk_index), None);
        let replacement = pinned_alpha_source_review(900);
        app.reload(replacement);
        app.with_state(|state| {
            assert_eq!(state.selected_file().unwrap().runtime_id, "alpha");
            assert_eq!(state.selection().hunk_index, None);
            assert_eq!(state.selection().side, None);
            assert_eq!(state.selection().line, None);
        });
    }

    #[test]
    fn reload_hunk_clamp_handles_file_without_hunks() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        app.select_extension_review_hunk("test", "alpha", 1);
        let mut replacement = pinned_two_hunk_alpha_review();
        replacement.files[0].hunks.clear();
        replacement.refresh_review_identities();
        app.reload(replacement);
        app.with_state(|state| {
            assert!(state.selected_file().unwrap().hunks.is_empty());
            assert_eq!(state.selection().hunk_index, None);
            assert_eq!(state.selection().side, None);
            assert_eq!(state.selection().line, None);
        });
        assert!(app.current_review_line_cursor().is_none());
    }

    #[test]
    fn reload_recovers_alpha_cursor_when_selected_hunk_is_retired() {
        let mut app = ReviewApp::new(pinned_two_hunk_alpha_review(), ReviewOptions::default());
        assert_eq!(
            app.with_state(|state| state.selected_file().unwrap().hunks.len()),
            2
        );
        app.select_extension_review_hunk("test", "alpha", 1);
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(1)
        );
        assert_eq!(
            app.current_review_line_cursor().unwrap().target.hunk_index,
            1
        );
        let before = (1..=12)
            .map(|line| format!("export const line{line} = {line};\n"))
            .collect::<String>();
        let after = before.replace("line1 = 1;", "line1 = 100;");
        let mut review = pinned_alpha_review_from_text(&before, &after);
        review.files[0].set_source_capability(None);
        review.refresh_review_identities();
        assert_eq!(review.files[0].hunks.len(), 1);
        app.reload(review);
        assert_eq!(
            app.with_state(|state| state.selected_file().unwrap().hunks.len()),
            1
        );
        assert_eq!(
            app.with_state(|state| state.selection().hunk_index),
            Some(0)
        );
        let cursor = app.current_review_line_cursor().unwrap().target;
        assert_eq!(cursor.hunk_index, 0);
        app.with_state(|state| {
            assert_eq!(
                state.changeset().files[cursor.file_index].runtime_id,
                "alpha"
            );
        });
    }

    #[test]
    fn pointer_note_start_moves_alpha_cursor_and_selection_to_note_line() {
        for cursor_line in [crate::CursorLineMode::Row, crate::CursorLineMode::Off] {
            for line in [12, 9] {
                let mut app = ReviewApp::new(
                    pinned_two_hunk_alpha_review(),
                    ReviewOptions {
                        cursor_line,
                        ..Default::default()
                    },
                );
                let target = crate::ReviewNoteTarget {
                    file_index: 0,
                    hunk_index: 1,
                    side: ReviewSide::New,
                    line,
                };
                app.note_hover_hit
                    .set(Some((ratatui::layout::Rect::new(1, 1, 1, 1), target)));
                assert!(app.handle_note_mouse(
                    &crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Up(
                            crossterm::event::MouseButton::Left
                        ),
                        column: 1,
                        row: 1,
                        modifiers: crossterm::event::KeyModifiers::NONE,
                    },
                    std::time::Instant::now(),
                ));
                assert_eq!(app.note_composer.as_ref().unwrap().target, target);
                assert_eq!(app.current_review_line_cursor().unwrap().target, target);
                let selection = app.with_state(|state| state.selection());
                assert_eq!(selection.hunk_index, Some(1));
                assert_eq!(selection.side, Some(ReviewSide::New));
                assert_eq!(selection.line, Some(line));
            }
        }
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
