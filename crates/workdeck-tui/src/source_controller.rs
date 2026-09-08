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
                self.install_source_loader(&file.key, loader);
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
        for file in &files.files {
            if self.expanded_gaps.iter().any(|(key, _)| key == &file.key) {
                self.start_source_load(&file.key, review_expansion_side(file.change_kind));
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
        let file = self.with_state(|state| {
            state
                .changeset()
                .files
                .iter()
                .find(|file| file.key == file_key)
                .cloned()
        });
        let Some(file) = file else {
            return false;
        };
        self.source_requests
            .retire(&BTreeSet::from([file.key.clone()]));
        self.source_loaders.insert(
            file.key.clone(),
            SourceLoaderBinding {
                identity: file.source_identity.clone(),
                loader,
            },
        );
        self.options.source_presentation.pending(&file);
        true
    }

    pub(super) fn start_source_load(&mut self, file_key: &str, side: ReviewSide) {
        let file = self.with_state(|state| {
            state
                .changeset()
                .files
                .iter()
                .find(|file| file.key == file_key)
                .cloned()
        });
        let Some(file) = file else {
            return;
        };
        let Some(binding) = self
            .source_loaders
            .get(file_key)
            .filter(|binding| binding.identity == file.source_identity)
        else {
            return;
        };
        if let Some(update) = self.source_requests.start(
            &file,
            side,
            Arc::clone(&binding.loader),
            self.options.source_presentation.status(&file),
        ) {
            self.options
                .source_presentation
                .set_status(&file, update.status);
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
        count
    }

    pub(super) fn reconcile_source_loaders(&mut self, changeset: &Changeset) {
        let retired: BTreeSet<_> = self
            .source_loaders
            .iter()
            .filter_map(|(key, binding)| {
                let retained = changeset.files.iter().any(|file| {
                    file.key == *key
                        && file.source_identity == binding.identity
                        && file.source_attested
                });
                (!retained).then(|| key.clone())
            })
            .collect();
        self.source_requests.retire(&retired);
        self.source_loaders.retain(|key, _| !retired.contains(key));
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
        assert_eq!(app.with_state(|state| state.changeset().clone()), original);
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
