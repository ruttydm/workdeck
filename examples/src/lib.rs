//! Workspace-compiled source pairs used by Workdeck's review examples.

#[path = "../1-hello-diff/after.rs"]
pub mod hello_diff_after;
#[path = "../1-hello-diff/before.rs"]
pub mod hello_diff_before;

#[path = "../2-mini-app-refactor/after/src/main.rs"]
pub mod mini_app_after;
#[path = "../2-mini-app-refactor/before/src/main.rs"]
pub mod mini_app_before;

#[cfg(test)]
#[path = "../2-mini-app-refactor/after/test/main_demo.rs"]
mod mini_app_after_demo;
#[cfg(test)]
#[path = "../2-mini-app-refactor/before/test/main_demo.rs"]
mod mini_app_before_demo;

#[cfg(test)]
mod patch_tests {
    use workdeck_core::ChangesetSource;
    use workdeck_diff::parse_patch;

    #[test]
    fn mini_app_patch_is_a_parseable_four_file_rust_review() {
        let patch = include_str!("../2-mini-app-refactor/change.patch");
        let changeset = parse_patch(
            patch,
            "mini-app-refactor",
            "Mini app refactor",
            ChangesetSource::Patch {
                label: "mini-app-refactor".into(),
            },
        )
        .expect("translated example patch parses");
        assert_eq!(
            changeset
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            [
                "src/format.rs",
                "src/group_tasks.rs",
                "src/main.rs",
                "test/main_demo.rs",
            ]
        );
        assert!(patch.contains("pub fn group_tasks"));
        assert!(patch.contains("[blocked]"));
        assert!(!patch.contains("bun:test"));
        assert!(!patch.contains(".ts"));
    }
}
