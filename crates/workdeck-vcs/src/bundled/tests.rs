use super::*;
use crate::{
    VcsLoadContext, VcsReviewInput, VcsReviewOperationKind, VcsWatchCoverage, VcsWatchTarget,
    VcsWatchTargetSource, get_vcs_adapter, load_vcs_review,
};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;
use workdeck_core::{CliInput, CommonOptions, VcsDiffCommandInput};

#[test]
fn loads_every_shipped_backend_through_the_public_catalog_shape() {
    let load = load_bundled_vcs_extensions();
    assert!(load.issues.is_empty());
    assert_eq!(
        load.extensions
            .iter()
            .map(|extension| extension.id)
            .collect::<Vec<_>>(),
        ["jj", "sl", "git"]
    );
    assert!(
        load.extensions
            .iter()
            .all(|extension| extension.origin == "bundled")
    );
    assert_eq!(
        get_bundled_vcs_adapters()
            .iter()
            .map(|adapter| adapter.id.as_str())
            .collect::<Vec<_>>(),
        ["jj", "sl", "git"]
    );
}

#[test]
fn normalizes_every_bundled_adapter_through_the_same_operation_type() {
    for adapter in get_bundled_vcs_adapters() {
        assert!(
            adapter
                .operations
                .contains_key(&VcsReviewOperationKind::WorkingTreeDiff)
        );
        assert!(
            adapter
                .operations
                .contains_key(&VcsReviewOperationKind::RevisionShow)
        );
        for operation in adapter.operations.values() {
            assert!(operation.watch_signature.is_some());
            assert!(operation.watch_plan.is_some());
        }
    }
}

#[test]
fn keeps_stash_review_to_git() {
    let catalog = bundled_vcs_catalog();
    assert!(
        get_vcs_adapter("git", catalog)
            .unwrap()
            .operations
            .contains_key(&VcsReviewOperationKind::StashShow)
    );
    for id in ["jj", "sl"] {
        assert!(
            !get_vcs_adapter(id, catalog)
                .unwrap()
                .operations
                .contains_key(&VcsReviewOperationKind::StashShow)
        );
    }
}

#[test]
fn loads_once_so_every_resolution_path_observes_one_adapter_identity() {
    let first = load_bundled_vcs_extensions();
    let second = load_bundled_vcs_extensions();
    assert!(std::ptr::eq(first, second));
    assert!(std::ptr::eq(
        &get_bundled_vcs_adapters()[0],
        &first.catalog.adapters[0]
    ));
}

#[test]
fn jujutsu_and_sapling_outrank_the_git_baseline() {
    let priority = |id: &str| {
        get_vcs_adapter(id, bundled_vcs_catalog())
            .unwrap()
            .detection_priority
            .unwrap_or_default()
    };
    assert_eq!(priority("git"), 0);
    assert!(priority("sl") > priority("git"));
    assert!(priority("jj") > priority("sl"));
}

#[test]
fn git_operation_loads_through_the_composed_catalog() {
    let directory = tempdir().unwrap();
    git(directory.path(), &["init"]);
    git(
        directory.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(directory.path(), &["config", "user.name", "Workdeck Tests"]);
    fs::write(directory.path().join("tracked.txt"), "before\n").unwrap();
    git(directory.path(), &["add", "tracked.txt"]);
    git(directory.path(), &["commit", "-m", "initial"]);
    fs::write(directory.path().join("tracked.txt"), "after\n").unwrap();

    let catalog = bundled_vcs_catalog();
    let adapter = get_vcs_adapter("git", catalog).unwrap();
    let input = VcsReviewInput::Diff(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions {
            vcs: Some("git".into()),
            exclude_untracked: Some(false),
            ..CommonOptions::default()
        },
    });
    let operation = crate::operation_from_input(input);
    let loaded = load_vcs_review(
        adapter,
        &operation,
        &VcsLoadContext {
            cwd: directory.path().to_owned(),
        },
        catalog,
    )
    .unwrap();
    assert_eq!(loaded.repo_root, directory.path().canonicalize().unwrap());
    assert!(loaded.patch_text.contains("tracked.txt"));
    assert!(loaded.source_reader.is_some());
}

#[test]
fn composed_git_catalog_drives_native_watch_plans_and_signatures() {
    let directory = tempdir().unwrap();
    git(directory.path(), &["init"]);
    git(
        directory.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(directory.path(), &["config", "user.name", "Workdeck Tests"]);
    fs::write(directory.path().join("tracked.txt"), "before\n").unwrap();
    git(directory.path(), &["add", "tracked.txt"]);
    git(directory.path(), &["commit", "-m", "initial"]);

    let input = CliInput::Vcs(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions {
            vcs: Some("git".into()),
            exclude_untracked: Some(false),
            ..CommonOptions::default()
        },
    });
    let catalog = bundled_vcs_catalog();
    let plan = crate::resolve_watch_plan(
        &input,
        crate::WatchPlanContext::current(directory.path(), Some(catalog)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(plan.coverage, VcsWatchCoverage::Hybrid);
    assert!(plan.targets.iter().any(|target| match target {
        VcsWatchTarget::DirectoryTree { sources, .. }
        | VcsWatchTarget::DirectoryEntries { sources, .. } => {
            sources.contains(&VcsWatchTargetSource::Worktree)
        }
    }));
    assert!(plan.targets.iter().any(|target| match target {
        VcsWatchTarget::DirectoryTree { sources, .. }
        | VcsWatchTarget::DirectoryEntries { sources, .. } => {
            sources.contains(&VcsWatchTargetSource::VcsMetadata)
        }
    }));

    let context = crate::WatchSignatureContext {
        cwd: directory.path(),
        vcs_catalog: Some(catalog),
    };
    let before = crate::compute_watch_signature(&input, context).unwrap();
    fs::write(directory.path().join("tracked.txt"), "after\n").unwrap();
    let after = crate::compute_watch_signature(&input, context).unwrap();
    assert_ne!(before, after);
}

fn git(cwd: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}
