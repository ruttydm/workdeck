//! Publication-bound adoption of prepared checkout navigation and panel providers.
use super::ReloadSessionOptions;
use crate::{DynamicReviewHostOptions, ReviewApp, ReviewOptions, workbench::*};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use workdeck_core::{CliInput, PatchCommandInput};
use workdeck_pm::{Repository, RequestId, SourceSelector, registry::*};

#[derive(Debug)]
struct BoundProvider(PathBuf);
impl RepositoryPanelProvider for BoundProvider {
    fn source(&self) -> RepositoryPanelSource {
        RepositoryPanelSource {
            root: self.0.clone(),
            identity: self.0.display().to_string(),
        }
    }
    fn load(&self, _: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
        Err(PanelError::new("fixture has no collection"))
    }
    fn preview(&self, _: &PanelTarget) -> Result<PanelPreview, PanelError> {
        Err(PanelError::new("fixture has no excerpt"))
    }
}
fn provider(root: &Path) -> Arc<dyn RepositoryPanelProvider> {
    Arc::new(BoundProvider(root.to_owned()))
}
fn changes(label: &str) -> workdeck_core::Changeset {
    workdeck_diff::changeset_from_patch(
        &format!("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+{label}\n"),
        label,
        label,
        "test",
        workdeck_core::ChangesetSource::WorkingTree { staged: false },
        None,
    )
}
fn input() -> CliInput {
    CliInput::Patch(PatchCommandInput {
        file: None,
        text: Some("fixture".into()),
        options: Default::default(),
    })
}
fn host(root: &Path, provider: Arc<dyn RepositoryPanelProvider>) -> DynamicReviewHostOptions {
    DynamicReviewHostOptions {
        command_cwd: root.to_owned(),
        repo_root: Some(root.to_owned()),
        repository_panels: Some(Some(provider)),
        ..Default::default()
    }
}

#[test]
fn checkout_panel_provider_and_cwd_change_only_after_publication_commits() {
    let old_root = Path::new("/original");
    let new_root = Path::new("/selected");
    let original = provider(old_root);
    let replacement = provider(new_root);
    let mut app = ReviewApp::new(
        changes("original"),
        ReviewOptions {
            repo: Some(old_root.into()),
            command_cwd: Some(old_root.into()),
            repository_panels: Some(original.clone()),
            ..Default::default()
        },
    );
    let publication = app.review_producer().get_publication_address();
    let failure = app.session_commit_reload_with_runtime(
        &input(),
        changes("selected"),
        &ReloadSessionOptions {
            reset_app: Some(false),
            ..Default::default()
        },
        Some(host(new_root, replacement.clone())),
        None,
        |_, _, _| Err("broker refused checkout".into()),
    );
    assert_eq!(failure.unwrap_err(), "broker refused checkout");
    assert_eq!(app.options.repo.as_deref(), Some(old_root));
    assert_eq!(app.options.command_cwd.as_deref(), Some(old_root));
    assert!(Arc::ptr_eq(
        app.options.repository_panels.as_ref().unwrap(),
        &original
    ));
    assert_eq!(app.review_producer().get_publication_address(), publication);
    app.session_commit_reload_with_runtime(
        &input(),
        changes("selected"),
        &ReloadSessionOptions {
            reset_app: Some(false),
            ..Default::default()
        },
        Some(host(new_root, replacement.clone())),
        None,
        |_, _, _| Ok("selected-session".into()),
    )
    .unwrap();
    assert_eq!(app.options.repo.as_deref(), Some(new_root));
    assert_eq!(app.options.command_cwd.as_deref(), Some(new_root));
    assert!(Arc::ptr_eq(
        app.options.repository_panels.as_ref().unwrap(),
        &replacement
    ));
    assert_eq!(original.source().root, old_root);
}

#[test]
fn wrong_root_provider_is_rejected_before_broker_or_review_changes() {
    let original = provider(Path::new("/original"));
    let mut app = ReviewApp::new(
        changes("original"),
        ReviewOptions {
            repository_panels: Some(original.clone()),
            ..Default::default()
        },
    );
    let publication = app.review_producer().get_publication_address();
    let failed = app.session_commit_reload_with_runtime(
        &input(),
        changes("selected"),
        &ReloadSessionOptions::default(),
        Some(host(Path::new("/selected"), original.clone())),
        None,
        |_, _, _| panic!("invalid provider reached broker"),
    );
    assert!(failed.unwrap_err().contains("provider does not match"));
    assert!(Arc::ptr_eq(
        app.options.repository_panels.as_ref().unwrap(),
        &original
    ));
    assert_eq!(app.review_producer().get_publication_address(), publication);
}

#[test]
fn removed_registration_after_loading_cannot_publish_a_checkout_reload() {
    let origin = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner = Repository::init(origin.path(), "WD").unwrap();
    Repository::init(target.path(), "WD").unwrap();
    let registry = RegistryStore::open(&owner).unwrap();
    let mapping =
        inspect_checkout("secondary", target.path(), SourceSelector::WorkingTree).unwrap();
    registry
        .mutate(
            &RegistryRequest {
                expected: registry.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: mapping.clone(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let navigation = registry.prepare_navigation(&mapping).unwrap();
    let original = provider(origin.path());
    let mut app = ReviewApp::new(
        changes("original"),
        ReviewOptions {
            repository_panels: Some(original.clone()),
            ..Default::default()
        },
    );
    let publication = app.review_producer().get_publication_address();
    let mut prepared = host(&mapping.checkout, provider(&mapping.checkout));
    prepared.registry_navigation = Some(navigation);
    registry
        .mutate(
            &RegistryRequest {
                expected: registry.snapshot().unwrap().source,
                mutation: RegistryMutation::Remove {
                    alias: mapping.alias,
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let failed = app.session_commit_reload_with_runtime(
        &input(),
        changes("selected"),
        &ReloadSessionOptions::default(),
        Some(prepared),
        None,
        |_, _, _| panic!("removed registration reached broker"),
    );
    assert!(failed.is_err());
    assert_eq!(app.review_producer().get_publication_address(), publication);
    assert!(Arc::ptr_eq(
        app.options.repository_panels.as_ref().unwrap(),
        &original
    ));
}

#[test]
fn registered_checkout_cannot_publish_with_another_repository_command_cwd() {
    let origin = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner = Repository::init(origin.path(), "WD").unwrap();
    Repository::init(target.path(), "WD").unwrap();
    let registry = RegistryStore::open(&owner).unwrap();
    let mapping =
        inspect_checkout("secondary", target.path(), SourceSelector::WorkingTree).unwrap();
    registry
        .mutate(
            &RegistryRequest {
                expected: registry.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: mapping.clone(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let original = provider(origin.path());
    let mut app = ReviewApp::new(
        changes("original"),
        ReviewOptions {
            command_cwd: Some(origin.path().to_owned()),
            repository_panels: Some(original.clone()),
            ..Default::default()
        },
    );
    let publication = app.review_producer().get_publication_address();
    let mut prepared = host(&mapping.checkout, provider(&mapping.checkout));
    prepared.registry_navigation = Some(registry.prepare_navigation(&mapping).unwrap());
    prepared.command_cwd = origin.path().to_owned();
    let mut reached_broker = false;
    let result = app.session_commit_reload_with_runtime(
        &input(),
        changes("selected"),
        &ReloadSessionOptions::default(),
        Some(prepared.clone()),
        None,
        |_, _, _| {
            reached_broker = true;
            Err("unexpected broker call".into())
        },
    );
    assert!(
        !reached_broker,
        "wrong-source command cwd reached the publication broker"
    );
    assert!(result.unwrap_err().contains("command cwd"));
    assert_eq!(app.options.command_cwd.as_deref(), Some(origin.path()));
    assert!(Arc::ptr_eq(
        app.options.repository_panels.as_ref().unwrap(),
        &original
    ));
    assert_eq!(app.review_producer().get_publication_address(), publication);
    #[cfg(unix)]
    {
        let escape = mapping.checkout.join("linked-cwd");
        std::os::unix::fs::symlink(origin.path(), &escape).unwrap();
        prepared.command_cwd = escape;
        let failure = app.session_commit_reload_with_runtime(
            &input(),
            changes("selected"),
            &ReloadSessionOptions::default(),
            Some(prepared.clone()),
            None,
            |_, _, _| panic!("symlink escape reached broker"),
        );
        assert!(failure.unwrap_err().contains("command cwd"));
    }
    // A legitimate nested cwd remains supported by the existing review loader.
    let nested = mapping.checkout.join("src");
    std::fs::create_dir(&nested).unwrap();
    prepared.command_cwd = nested.clone();
    app.session_commit_reload_with_runtime(
        &input(),
        changes("selected"),
        &ReloadSessionOptions::default(),
        Some(prepared),
        None,
        |_, _, _| Ok("nested-checkout-session".into()),
    )
    .unwrap();
    assert_eq!(app.options.command_cwd.as_deref(), Some(nested.as_path()));
    assert_eq!(app.options.repo.as_ref(), Some(&mapping.checkout));
}
