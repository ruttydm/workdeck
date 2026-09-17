use super::*;
use crate::ReviewOptions;
use workdeck_pm::{CreateIssue, Repository, RequestId, registry::*};

#[test]
fn checkout_capacity_preserves_all_drafts_and_an_alias_does_not_consume_another_slot() {
    let directories = (0..=MAX_CONTEXTS)
        .map(|_| tempfile::tempdir().unwrap())
        .collect::<Vec<_>>();
    let repositories = directories
        .iter()
        .map(|directory| Repository::init(directory.path(), "WD").unwrap())
        .collect::<Vec<_>>();
    let root = directories[0].path().canonicalize().unwrap();
    let store = RegistryStore::open(&repositories[0]).unwrap();
    let mappings = directories
        .iter()
        .enumerate()
        .map(|(index, directory)| {
            let mapping = inspect_checkout(
                &format!("checkout-{index}"),
                directory.path(),
                SourceSelector::WorkingTree,
            )
            .unwrap();
            store
                .mutate(
                    &RegistryRequest {
                        expected: store.snapshot().unwrap().source,
                        mutation: RegistryMutation::Register {
                            checkout: mapping.clone(),
                        },
                    },
                    &RequestId::new(),
                )
                .unwrap();
            mapping
        })
        .collect::<Vec<_>>();
    let mut app = ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "retention",
            "retention",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        ),
        ReviewOptions {
            repo: Some(root.clone()),
            workbench: Some(WorkbenchOptions::new(root)),
            review_input: Some(CliInput::Vcs(VcsDiffCommandInput {
                range: None,
                range_endpoints: None,
                staged: false,
                pathspecs: Vec::new(),
                options: Default::default(),
            })),
            ..Default::default()
        },
    );
    for mapping in mappings.iter().take(MAX_CONTEXTS).skip(1) {
        let mut shell = WorkbenchShell::open(WorkbenchOptions::new(&mapping.checkout), true);
        shell.controller.begin_create(CreateIssue::new(
            &mapping.alias,
            "retain this unsaved source-bound intent",
        ));
        let review = app.capture_native_return(shell.controller.return_context());
        app.workbench_checkouts.retained.push(RetainedCheckout {
            shell: Mutex::new(shell),
            binding: mapping.clone(),
            navigation: Some(store.prepare_navigation(mapping).unwrap()),
            review,
            command_cwd: mapping.checkout.clone(),
            provider: None,
        });
    }
    let active_root = app.options.repo.clone();
    let failure = app.workbench_registered_checkout(
        store.prepare_navigation(&mappings[MAX_CONTEXTS]).unwrap(),
        &mut |_, _, _| panic!("ninth source reached loader"),
    );
    assert!(failure.unwrap_err().contains("retains 8 checkout contexts"));
    assert_eq!(app.options.repo, active_root);
    assert_eq!(app.workbench_checkouts.retained.len(), MAX_CONTEXTS - 1);
    for entry in &app.workbench_checkouts.retained {
        let shell = entry.shell.lock().unwrap();
        let (_, draft) = shell.controller.active_draft().unwrap();
        let super::super::DraftInput::Create(input) = &draft.input else {
            panic!("retained draft changed kind");
        };
        assert_eq!(input.title, entry.binding.alias);
        assert!(same_source(
            shell.checkout_binding.as_ref().unwrap(),
            &entry.binding
        ));
    }
    app.workbench_registered_checkout(
        store.prepare_navigation(&mappings[0]).unwrap(),
        &mut |_, _, _| panic!("same checkout alias unnecessarily reloaded"),
    )
    .unwrap();
    assert_eq!(app.workbench_checkouts.retained.len(), MAX_CONTEXTS - 1);
    assert_eq!(
        app.workbench_checkouts
            .navigation
            .as_ref()
            .unwrap()
            .checkout()
            .alias,
        "checkout-0"
    );
    app.shutdown_foreground_run().unwrap();
}
