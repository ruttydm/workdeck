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

#[path = "../3-agent-review-demo/after/src/index.rs"]
pub mod agent_review_after;
#[path = "../3-agent-review-demo/before/src/index.rs"]
pub mod agent_review_before;

#[path = "../4-ui-polish/after.rs"]
pub mod ui_polish_after;
#[path = "../4-ui-polish/before.rs"]
pub mod ui_polish_before;

#[path = "../5-pager-tour/after.rs"]
pub mod pager_tour_after;
#[path = "../5-pager-tour/before.rs"]
pub mod pager_tour_before;

#[path = "../6-readme-screenshot/after/src/index.rs"]
pub mod readme_screenshot_after;
#[path = "../6-readme-screenshot/before/src/index.rs"]
pub mod readme_screenshot_before;

#[path = "../7-ratatui-component/after.rs"]
pub mod ratatui_component_after;
#[path = "../7-ratatui-component/before.rs"]
pub mod ratatui_component_before;
#[path = "../7-ratatui-component/from_files.rs"]
pub mod ratatui_component_from_files;
#[path = "../7-ratatui-component/from_patch.rs"]
pub mod ratatui_component_from_patch;
#[path = "../7-ratatui-component/support.rs"]
pub mod ratatui_component_support;

#[path = "../8-ratatui-primitives/primitives_demo.rs"]
pub mod ratatui_primitives_demo;

#[path = "../9-agent-markup-notes/after/retry.rs"]
pub mod agent_markup_after;
#[path = "../9-agent-markup-notes/before/retry.rs"]
pub mod agent_markup_before;

#[path = "../extensions/cli-tools/extension.rs"]
pub mod cli_tools_extension;

#[path = "../extensions/pane-layout/extension.rs"]
pub mod pane_layout_extension;

#[path = "../extensions/vim-navigation/extension.rs"]
pub mod vim_navigation_extension;

#[path = "../extensions/review-snapshot-export/extension.rs"]
pub mod review_snapshot_export_extension;

#[path = "../extensions/review-note-navigator/extension.rs"]
pub mod review_note_navigator_extension;

#[path = "../extensions/rendered-markdown/extension.rs"]
pub mod rendered_markdown_extension;

#[path = "../extensions/jsx-file-view/extension.rs"]
pub mod jsx_file_view_extension;

#[path = "../extensions/inline-edit/extension.rs"]
pub mod inline_edit_extension;

#[path = "../extensions/review-triage/extension.rs"]
pub mod review_triage_extension;

#[path = "../extensions/github-pr/extension.rs"]
pub mod github_pr_extension;

#[path = "../extensions/file-view-gallery/extension.rs"]
pub mod file_view_gallery_extension;

#[path = "../extensions/native-vcs/extension.rs"]
pub mod native_vcs_extension;

#[path = "../extensions/file-view-gallery/fixtures/change-atlas/after.rs"]
pub mod file_view_gallery_change_atlas_after;
#[path = "../extensions/file-view-gallery/fixtures/change-atlas/before.rs"]
pub mod file_view_gallery_change_atlas_before;

#[cfg(test)]
#[path = "../3-agent-review-demo/after/test/search_demo.rs"]
mod agent_review_after_demo;
#[cfg(test)]
#[path = "../3-agent-review-demo/before/test/search_demo.rs"]
mod agent_review_before_demo;

#[cfg(test)]
#[path = "../6-readme-screenshot/after/test/review_summary_card_demo.rs"]
mod readme_screenshot_after_demo;
#[cfg(test)]
#[path = "../6-readme-screenshot/before/test/review_summary_card_demo.rs"]
mod readme_screenshot_before_demo;

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

    #[test]
    fn agent_review_patch_and_sidecar_address_the_translated_rust_files() {
        let patch = include_str!("../3-agent-review-demo/change.patch");
        let mut changeset = parse_patch(
            patch,
            "agent-review-demo",
            "Agent review demo",
            ChangesetSource::Patch {
                label: "agent-review-demo".into(),
            },
        )
        .expect("translated agent-review patch parses");
        assert_eq!(
            changeset
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            [
                "src/commands.rs",
                "src/index.rs",
                "src/normalize.rs",
                "src/search.rs",
                "test/search_demo.rs",
            ]
        );
        let context = workdeck_core::AgentContext::from_json(include_str!(
            "../3-agent-review-demo/agent-context.json"
        ))
        .expect("translated agent context validates");
        context.apply_to(&mut changeset);

        assert_eq!(changeset.files.len(), 5);
        assert_eq!(
            changeset
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            [
                "src/normalize.rs",
                "src/search.rs",
                "src/index.rs",
                "test/search_demo.rs",
                "src/commands.rs",
            ]
        );
        assert_eq!(
            changeset
                .files
                .iter()
                .filter(|file| file.agent.is_some())
                .count(),
            4
        );
        assert_eq!(
            changeset
                .files
                .iter()
                .filter_map(|file| file.agent.as_ref())
                .flat_map(|context| &context.annotations)
                .count(),
            5
        );
        assert!(!patch.contains("bun:test"));
        assert!(!patch.contains(".ts"));
    }

    #[test]
    fn readme_screenshot_patch_and_sidecar_form_a_three_file_rust_review() {
        let patch = include_str!("../6-readme-screenshot/change.patch");
        let mut changeset = parse_patch(
            patch,
            "readme-screenshot",
            "README screenshot",
            ChangesetSource::Patch {
                label: "readme-screenshot".into(),
            },
        )
        .expect("translated screenshot patch parses");
        assert_eq!(
            changeset
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            [
                "src/components/review_summary_card.rs",
                "src/lib/review_copy.rs",
                "test/review_summary_card_demo.rs",
            ]
        );
        let context = workdeck_core::AgentContext::from_json(include_str!(
            "../6-readme-screenshot/agent-context.json"
        ))
        .expect("translated screenshot agent context validates");
        context.apply_to(&mut changeset);

        assert_eq!(changeset.files.len(), 3);
        assert!(changeset.files.iter().all(|file| file.agent.is_some()));
        assert_eq!(
            changeset
                .files
                .iter()
                .filter_map(|file| file.agent.as_ref())
                .flat_map(|context| &context.annotations)
                .count(),
            3
        );
        assert!(!patch.contains("bun:test"));
        assert!(!patch.contains(".tsx"));
        assert!(!patch.contains(".ts"));
    }

    #[test]
    fn ratatui_component_patch_is_a_single_file_rust_review() {
        let patch = include_str!("../7-ratatui-component/change.patch");
        let changeset = parse_patch(
            patch,
            "ratatui-component",
            "Ratatui component",
            ChangesetSource::Patch {
                label: "ratatui-component".into(),
            },
        )
        .expect("translated component patch parses");

        assert_eq!(changeset.files.len(), 1);
        assert_eq!(changeset.files[0].path, "src/review_summary.rs");
        assert!(patch.contains("pub tags: Vec<String>"));
        assert!(patch.contains("format_review_summary"));
        assert!(!patch.contains(".ts"));
    }

    #[test]
    fn agent_markup_patch_and_both_stml_notes_are_executable() {
        let patch = include_str!("../9-agent-markup-notes/change.patch");
        let mut changeset = parse_patch(
            patch,
            "agent-markup-notes",
            "Agent markup notes",
            ChangesetSource::Patch {
                label: "agent-markup-notes".into(),
            },
        )
        .expect("translated retry patch parses");
        let context = workdeck_core::AgentContext::from_json(include_str!(
            "../9-agent-markup-notes/agent-context.json"
        ))
        .expect("translated STML sidecar validates");
        context.apply_to(&mut changeset);

        assert_eq!(changeset.files.len(), 1);
        assert_eq!(changeset.files[0].path, "src/retry.rs");
        let annotations = &changeset.files[0]
            .agent
            .as_ref()
            .expect("retry file has agent context")
            .annotations;
        assert_eq!(annotations.len(), 2);
        let rendered = annotations
            .iter()
            .map(|annotation| {
                workdeck_markup::render(
                    annotation.markup.as_deref().expect("annotation has STML"),
                    56,
                )
            })
            .collect::<Vec<_>>();
        assert!(rendered.iter().all(|markup| markup.notes.is_empty()));
        assert!(rendered[0].lines.join("\n").contains("Retry flow"));
        assert!(rendered[0].lines.join("\n").contains("3 attempts"));
        assert!(rendered[1].lines.join("\n").contains("fn backoff"));
        assert!(!patch.contains(".ts"));
    }
}
