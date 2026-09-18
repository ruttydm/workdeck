//! Native replacement for Hunk's `test/helpers/config-home.ts`.
//!
//! A `TempDir` owns the same lifecycle that the JavaScript helper implemented
//! with a process-wide cleanup stack, while exposing a small command adapter so
//! every spawned Workdeck process reads only the test configuration.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{Builder, TempDir};

pub struct IsolatedConfigHome {
    directory: TempDir,
}

impl IsolatedConfigHome {
    pub fn new(prefix: &str) -> Self {
        let directory = Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("create isolated config home");
        Self { directory }
    }

    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    pub fn apply(&self, command: &mut Command) {
        command.env("XDG_CONFIG_HOME", self.path());
    }

    pub fn into_path(self) -> PathBuf {
        self.directory.keep()
    }
}

#[test]
fn isolated_config_home_is_scoped_and_removed_after_drop() {
    let first_path;
    let second_path;
    {
        let first = IsolatedConfigHome::new("workdeck-test-config-");
        let second = IsolatedConfigHome::new("workdeck-test-config-");
        first_path = first.path().to_owned();
        second_path = second.path().to_owned();

        let mut command = Command::new("workdeck");
        first.apply(&mut command);
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == "XDG_CONFIG_HOME")
                .map(|(_, value)| value),
            Some(Some(first.path().as_os_str()))
        );
        assert!(first.path().is_dir());
        assert!(second.path().is_dir());
    }
    assert!(!first_path.exists());
    assert!(!second_path.exists());
}
