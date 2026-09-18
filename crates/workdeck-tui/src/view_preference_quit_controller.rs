//! View-preference dirty state and the save-or-discard quit workflow.
//!
//! This is a clean-room Rust translation of Hunk's MIT-licensed
//! `src/ui/hooks/useViewPreferenceQuitController.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. The Ratatui app owns rendering
//! and event routing; this controller owns the mounted baseline and quit lock.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use workdeck_core::{
    PersistedViewPreferences, ViewPreferenceChange, ViewPreferencePersistenceError,
    ViewPreferenceWritePolicy, diff_persisted_view_preferences, save_global_view_preferences,
    save_view_preferences_prompt_preference,
};

pub const POST_PERSISTENCE_QUIT_DELAY: Duration = Duration::from_millis(120);
pub const DEFAULT_VIEW_PREFERENCES_CONFIG_LABEL: &str = "~/.config/workdeck/config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPreferenceDiffLine {
    pub removed: bool,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitRequestOutcome {
    Locked,
    PromptOpened,
    QuitNow,
}

#[derive(Debug)]
pub struct ViewPreferenceQuitController {
    saved_preferences: PersistedViewPreferences,
    config_path: Option<PathBuf>,
    write_policy: ViewPreferenceWritePolicy,
    pager_mode: bool,
    prompt_save_view_preferences: bool,
    transient_view_preferences: bool,
    home_directory: Option<PathBuf>,
    save_config_prompt_open: bool,
    pending_quit_deadline: Option<Instant>,
}

impl ViewPreferenceQuitController {
    #[must_use]
    pub fn new(
        current_preferences: PersistedViewPreferences,
        config_path: Option<PathBuf>,
        pager_mode: bool,
        prompt_save_view_preferences: bool,
        transient_view_preferences: bool,
        home_directory: Option<PathBuf>,
    ) -> Self {
        Self {
            saved_preferences: current_preferences,
            config_path,
            write_policy: ViewPreferenceWritePolicy::Writable,
            pager_mode,
            prompt_save_view_preferences,
            transient_view_preferences,
            home_directory,
            save_config_prompt_open: false,
            pending_quit_deadline: None,
        }
    }

    /// Install the host's resolved source policy, including on soft reload.
    pub fn set_write_policy(&mut self, policy: ViewPreferenceWritePolicy) {
        self.write_policy = policy;
    }

    /// Replace soft-bootstrap facts without replacing the baseline captured at mount.
    pub fn replace_inputs(
        &mut self,
        config_path: Option<PathBuf>,
        pager_mode: bool,
        prompt_save_view_preferences: bool,
        transient_view_preferences: bool,
        home_directory: Option<PathBuf>,
    ) {
        self.config_path = config_path;
        self.pager_mode = pager_mode;
        self.prompt_save_view_preferences = prompt_save_view_preferences;
        self.transient_view_preferences = transient_view_preferences;
        self.home_directory = home_directory;
    }

    #[must_use]
    pub fn changed_view_preferences(
        &self,
        current: &PersistedViewPreferences,
    ) -> Vec<ViewPreferenceChange> {
        diff_persisted_view_preferences(&self.saved_preferences, current)
    }

    #[must_use]
    pub fn view_preference_diff_lines(
        &self,
        current: &PersistedViewPreferences,
    ) -> Vec<ViewPreferenceDiffLine> {
        let changes = self.changed_view_preferences(current);
        let width = changes
            .iter()
            .map(|change| change.config_key.len())
            .max()
            .unwrap_or(0);
        changes
            .into_iter()
            .flat_map(|change| {
                [
                    ViewPreferenceDiffLine {
                        removed: true,
                        text: format!(
                            "- {:width$} = {}",
                            change.config_key,
                            change.previous_value,
                            width = width
                        ),
                    },
                    ViewPreferenceDiffLine {
                        removed: false,
                        text: format!(
                            "+ {:width$} = {}",
                            change.config_key,
                            change.next_value,
                            width = width
                        ),
                    },
                ]
            })
            .collect()
    }

    #[must_use]
    pub fn view_preferences_config_label(&self) -> String {
        let path = self
            .config_path
            .as_deref()
            .map_or_else(|| DEFAULT_VIEW_PREFERENCES_CONFIG_LABEL.into(), path_text);
        let Some(home) = self.home_directory.as_deref().map(path_text) else {
            return path;
        };
        path.strip_prefix(&home)
            .map_or(path.clone(), |suffix| format!("~{suffix}"))
    }

    #[must_use]
    pub const fn save_config_prompt_open(&self) -> bool {
        self.save_config_prompt_open
    }

    #[must_use]
    pub const fn quit_pending(&self) -> bool {
        self.pending_quit_deadline.is_some()
    }

    pub fn request_quit(&mut self, current: &PersistedViewPreferences) -> QuitRequestOutcome {
        if self.quit_pending() {
            return QuitRequestOutcome::Locked;
        }
        if !self.pager_mode
            && !self.transient_view_preferences
            && self.prompt_save_view_preferences
            && !self.changed_view_preferences(current).is_empty()
        {
            self.save_config_prompt_open = true;
            QuitRequestOutcome::PromptOpened
        } else {
            QuitRequestOutcome::QuitNow
        }
    }

    pub fn save_view_preferences_and_schedule_quit(
        &mut self,
        current: &PersistedViewPreferences,
        now: Instant,
    ) -> Result<Option<PathBuf>, ViewPreferencePersistenceError> {
        if self.quit_pending() {
            return Ok(None);
        }
        self.write_policy.ensure_writable()?;
        let path = save_global_view_preferences(current, self.config_path.as_deref())?;
        self.saved_preferences.clone_from(current);
        self.schedule_quit(now);
        Ok(Some(path))
    }

    #[must_use]
    pub fn discard_view_preferences_and_quit(&mut self) -> bool {
        if self.quit_pending() {
            return false;
        }
        self.save_config_prompt_open = false;
        true
    }

    pub fn never_ask_and_schedule_quit(
        &mut self,
        now: Instant,
    ) -> Result<Option<PathBuf>, ViewPreferencePersistenceError> {
        if self.quit_pending() {
            return Ok(None);
        }
        self.write_policy.ensure_writable()?;
        let path = save_view_preferences_prompt_preference(false, self.config_path.as_deref())?;
        self.schedule_quit(now);
        Ok(Some(path))
    }

    pub fn close_save_config_prompt(&mut self) {
        if !self.quit_pending() {
            self.save_config_prompt_open = false;
        }
    }

    /// Advance the one-shot delayed quit. A dropped controller cancels it by ownership.
    #[must_use]
    pub fn poll_quit(&mut self, now: Instant) -> bool {
        let due = self
            .pending_quit_deadline
            .is_some_and(|deadline| now >= deadline);
        if due {
            self.pending_quit_deadline = None;
        }
        due
    }

    fn schedule_quit(&mut self, now: Instant) {
        self.save_config_prompt_open = false;
        self.pending_quit_deadline = Some(now + POST_PERSISTENCE_QUIT_DELAY);
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{InputCursorLine, InputLayoutMode};

    fn preferences() -> PersistedViewPreferences {
        PersistedViewPreferences {
            mode: InputLayoutMode::Auto,
            theme: Some("github-dark-default".into()),
            show_line_numbers: true,
            wrap_lines: false,
            show_hunk_headers: true,
            show_menu_bar: true,
            show_agent_notes: false,
            copy_decorations: false,
            cursor_line: InputCursorLine::Row,
        }
    }

    fn controller(
        current: PersistedViewPreferences,
        config_path: Option<PathBuf>,
    ) -> ViewPreferenceQuitController {
        ViewPreferenceQuitController::new(
            current,
            config_path,
            false,
            true,
            false,
            Some(PathBuf::from("/test/home")),
        )
    }

    #[test]
    fn derives_dirty_preferences_and_aligned_toml_rows_in_persistence_order() {
        let initial = preferences();
        let controller = controller(initial.clone(), None);
        assert!(controller.changed_view_preferences(&initial).is_empty());
        let current = PersistedViewPreferences {
            theme: Some("github-dark-dimmed".into()),
            show_line_numbers: false,
            wrap_lines: true,
            ..initial
        };
        assert_eq!(
            controller
                .changed_view_preferences(&current)
                .iter()
                .map(|change| change.config_key)
                .collect::<Vec<_>>(),
            ["theme", "line_numbers", "wrap_lines"]
        );
        assert_eq!(
            controller.view_preference_diff_lines(&current),
            [
                ViewPreferenceDiffLine {
                    removed: true,
                    text: "- theme        = \"github-dark-default\"".into()
                },
                ViewPreferenceDiffLine {
                    removed: false,
                    text: "+ theme        = \"github-dark-dimmed\"".into()
                },
                ViewPreferenceDiffLine {
                    removed: true,
                    text: "- line_numbers = true".into()
                },
                ViewPreferenceDiffLine {
                    removed: false,
                    text: "+ line_numbers = false".into()
                },
                ViewPreferenceDiffLine {
                    removed: true,
                    text: "- wrap_lines   = false".into()
                },
                ViewPreferenceDiffLine {
                    removed: false,
                    text: "+ wrap_lines   = true".into()
                },
            ]
        );
    }

    #[test]
    fn shortens_config_paths_only_when_an_explicit_home_contains_them() {
        let mut controller = controller(
            preferences(),
            Some(PathBuf::from("/users/probe/.config/workdeck/config.toml")),
        );
        controller.home_directory = Some(PathBuf::from("/users/probe"));
        assert_eq!(
            controller.view_preferences_config_label(),
            "~/.config/workdeck/config.toml"
        );
        controller.config_path = Some(PathBuf::from("/etc/workdeck/config.toml"));
        assert_eq!(
            controller.view_preferences_config_label(),
            "/etc/workdeck/config.toml"
        );
        controller.config_path = Some(PathBuf::from("/users/probe/.config/workdeck/config.toml"));
        controller.home_directory = None;
        assert_eq!(
            controller.view_preferences_config_label(),
            "/users/probe/.config/workdeck/config.toml"
        );
        controller.config_path = None;
        assert_eq!(
            controller.view_preferences_config_label(),
            DEFAULT_VIEW_PREFERENCES_CONFIG_LABEL
        );
    }

    #[test]
    fn opens_the_prompt_only_for_changed_persistent_preferences() {
        let initial = preferences();
        let mut controller = controller(initial.clone(), None);
        let changed = PersistedViewPreferences {
            wrap_lines: true,
            ..initial
        };
        assert_eq!(
            controller.request_quit(&changed),
            QuitRequestOutcome::PromptOpened
        );
        assert!(controller.save_config_prompt_open());
    }

    #[test]
    fn bypasses_prompting_when_unchanged_paging_transient_or_disabled_by_policy() {
        let initial = preferences();
        let changed = PersistedViewPreferences {
            wrap_lines: true,
            ..initial.clone()
        };
        let mut unchanged = controller(initial.clone(), None);
        assert_eq!(
            unchanged.request_quit(&initial),
            QuitRequestOutcome::QuitNow
        );
        for (pager, transient, prompt) in [
            (true, false, true),
            (false, true, true),
            (false, false, false),
        ] {
            let mut controller = controller(initial.clone(), None);
            controller.pager_mode = pager;
            controller.transient_view_preferences = transient;
            controller.prompt_save_view_preferences = prompt;
            assert_eq!(
                controller.request_quit(&changed),
                QuitRequestOutcome::QuitNow
            );
        }
    }

    #[test]
    fn saves_preferences_closes_prompt_advances_baseline_and_delays_quit() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        let initial = preferences();
        let current = PersistedViewPreferences {
            theme: Some("github-dark-dimmed".into()),
            ..initial.clone()
        };
        let mut controller = controller(initial, Some(path.clone()));
        assert_eq!(
            controller.request_quit(&current),
            QuitRequestOutcome::PromptOpened
        );
        let now = Instant::now();
        assert_eq!(
            controller
                .save_view_preferences_and_schedule_quit(&current, now)
                .unwrap(),
            Some(path.clone())
        );
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains("theme = \"github-dark-dimmed\"")
        );
        assert!(controller.changed_view_preferences(&current).is_empty());
        assert!(!controller.save_config_prompt_open());
        assert!(!controller.poll_quit(now + Duration::from_millis(119)));
        assert!(controller.poll_quit(now + POST_PERSISTENCE_QUIT_DELAY));
    }

    #[test]
    fn locks_persistence_and_quit_actions_after_scheduling_one_successful_quit() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        let initial = preferences();
        let current = PersistedViewPreferences {
            wrap_lines: true,
            ..initial.clone()
        };
        let mut controller = controller(initial, Some(path.clone()));
        let now = Instant::now();
        assert!(
            controller
                .save_view_preferences_and_schedule_quit(&current, now)
                .unwrap()
                .is_some()
        );
        assert!(
            controller
                .save_view_preferences_and_schedule_quit(&current, now)
                .unwrap()
                .is_none()
        );
        assert!(
            controller
                .never_ask_and_schedule_quit(now)
                .unwrap()
                .is_none()
        );
        assert!(!controller.discard_view_preferences_and_quit());
        assert_eq!(
            controller.request_quit(&current),
            QuitRequestOutcome::Locked
        );
        assert!(
            !std::fs::read_to_string(path)
                .unwrap()
                .contains(workdeck_core::VIEW_PREFERENCES_PROMPT_CONFIG_KEY)
        );
    }

    #[test]
    fn dropping_a_controller_cancels_its_pending_delayed_quit() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        let current = PersistedViewPreferences {
            wrap_lines: true,
            ..preferences()
        };
        let mut controller = controller(preferences(), Some(path));
        controller
            .save_view_preferences_and_schedule_quit(&current, Instant::now())
            .unwrap();
        assert!(controller.quit_pending());
        drop(controller);
    }

    #[test]
    fn reports_save_failures_without_advancing_baseline_or_quitting() {
        let directory = tempfile::TempDir::new().unwrap();
        let initial = preferences();
        let current = PersistedViewPreferences {
            wrap_lines: true,
            ..initial.clone()
        };
        let mut controller = controller(initial, Some(directory.path().to_owned()));
        assert_eq!(
            controller.request_quit(&current),
            QuitRequestOutcome::PromptOpened
        );
        assert!(
            controller
                .save_view_preferences_and_schedule_quit(&current, Instant::now())
                .is_err()
        );
        assert_eq!(controller.changed_view_preferences(&current).len(), 1);
        assert!(controller.save_config_prompt_open());
        assert!(!controller.quit_pending());
    }

    #[test]
    fn legacy_preference_saves_and_never_ask_are_read_only_without_global_fallback() {
        let directory = tempfile::TempDir::new().unwrap();
        // The policy, not a magic directory name, controls write admission.
        let path = directory.path().join("old-preferences.toml");
        let global = directory.path().join("global/config.toml");
        std::fs::create_dir_all(global.parent().unwrap()).unwrap();
        std::fs::write(&global, "# global settings\nmode = 'stack'\n").unwrap();
        let original = "# legacy settings\nmode = 'split'\n";
        std::fs::write(&path, original).unwrap();
        let current = PersistedViewPreferences {
            wrap_lines: true,
            ..preferences()
        };
        let mut controller = controller(preferences(), Some(path.clone()));
        controller.home_directory = Some(directory.path().to_owned());
        controller.set_write_policy(ViewPreferenceWritePolicy::LegacyReadOnly);
        assert_eq!(
            controller.request_quit(&current),
            QuitRequestOutcome::PromptOpened
        );
        let save = controller
            .save_view_preferences_and_schedule_quit(&current, Instant::now())
            .unwrap_err();
        assert!(matches!(
            save,
            ViewPreferencePersistenceError::LegacyReadOnly
        ));
        assert!(save.to_string().contains("workdeck config init"));
        let never = controller
            .never_ask_and_schedule_quit(Instant::now())
            .unwrap_err();
        assert!(matches!(
            never,
            ViewPreferencePersistenceError::LegacyReadOnly
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(global).unwrap(),
            "# global settings\nmode = 'stack'\n"
        );
        assert!(controller.save_config_prompt_open());
        assert!(!controller.quit_pending());
        assert_eq!(controller.changed_view_preferences(&current).len(), 1);
        assert!(controller.discard_view_preferences_and_quit());
    }

    #[test]
    fn explicit_native_and_global_targets_remain_writable() {
        let directory = tempfile::TempDir::new().unwrap();
        for relative in [".workdeck/config.toml", "global/config.toml"] {
            let path = directory.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "# preserved\n[future]\nvalue = 42\n").unwrap();
            let current = PersistedViewPreferences {
                wrap_lines: true,
                ..preferences()
            };
            let mut controller = controller(preferences(), Some(path.clone()));
            controller.set_write_policy(ViewPreferenceWritePolicy::Writable);
            assert_eq!(
                controller
                    .save_view_preferences_and_schedule_quit(&current, Instant::now())
                    .unwrap(),
                Some(path.clone())
            );
            let raw = std::fs::read_to_string(path).unwrap();
            assert!(raw.contains("wrap_lines = true"));
            assert!(raw.contains("# preserved"));
            assert!(raw.contains("[future]\nvalue = 42"));
        }
    }

    #[test]
    fn discards_without_writing_and_quits_immediately() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        let mut controller = controller(preferences(), Some(path.clone()));
        controller.save_config_prompt_open = true;
        assert!(controller.discard_view_preferences_and_quit());
        assert!(!path.exists());
        assert!(!controller.save_config_prompt_open());
    }

    #[test]
    fn never_ask_writes_only_prompt_policy_closes_prompt_and_delays_quit() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "# keep me\n").unwrap();
        let current = PersistedViewPreferences {
            theme: Some("github-dark-dimmed".into()),
            ..preferences()
        };
        let mut controller = controller(preferences(), Some(path.clone()));
        controller.request_quit(&current);
        let now = Instant::now();
        assert_eq!(
            controller.never_ask_and_schedule_quit(now).unwrap(),
            Some(path.clone())
        );
        let source = std::fs::read_to_string(path).unwrap();
        assert!(source.contains("# keep me"));
        assert!(source.contains("prompt_save_view_preferences = false"));
        assert!(!source.contains("theme ="));
        assert_eq!(controller.changed_view_preferences(&current).len(), 1);
        assert!(!controller.save_config_prompt_open());
        assert!(!controller.poll_quit(now + Duration::from_millis(119)));
        assert!(controller.poll_quit(now + POST_PERSISTENCE_QUIT_DELAY));
    }

    #[test]
    fn reports_never_ask_failures_without_quitting() {
        let directory = tempfile::TempDir::new().unwrap();
        let current = PersistedViewPreferences {
            wrap_lines: true,
            ..preferences()
        };
        let mut controller = controller(preferences(), Some(directory.path().to_owned()));
        controller.request_quit(&current);
        assert!(
            controller
                .never_ask_and_schedule_quit(Instant::now())
                .is_err()
        );
        assert_eq!(controller.changed_view_preferences(&current).len(), 1);
        assert!(controller.save_config_prompt_open());
        assert!(!controller.quit_pending());
    }

    #[test]
    fn cancels_the_prompt_without_quitting_or_clearing_dirty_state() {
        let current = PersistedViewPreferences {
            wrap_lines: true,
            ..preferences()
        };
        let mut controller = controller(preferences(), None);
        controller.request_quit(&current);
        controller.close_save_config_prompt();
        assert!(!controller.save_config_prompt_open());
        assert_eq!(controller.changed_view_preferences(&current).len(), 1);
        assert!(!controller.quit_pending());
    }

    #[test]
    fn preserves_the_mounted_baseline_when_soft_reload_inputs_change() {
        let initial = preferences();
        let current = PersistedViewPreferences {
            wrap_lines: true,
            ..initial.clone()
        };
        let mut controller = controller(
            initial.clone(),
            Some(PathBuf::from("/review/one/config.toml")),
        );
        assert_eq!(controller.changed_view_preferences(&current).len(), 1);
        controller.replace_inputs(
            Some(PathBuf::from("/review/two/config.toml")),
            false,
            true,
            false,
            None,
        );
        assert_eq!(
            controller.view_preferences_config_label(),
            "/review/two/config.toml"
        );
        assert_eq!(controller.changed_view_preferences(&current).len(), 1);
        assert!(controller.changed_view_preferences(&initial).is_empty());
    }
}
