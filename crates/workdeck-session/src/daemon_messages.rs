//! Shared build-skew wording across window notices, agent errors, and daemon commands.

/// How a mismatch message names each side of the skew.
pub const WORKDECK_BUILD_RELATION_OLDER: &str = "an older Workdeck build";
pub const WORKDECK_BUILD_RELATION_NEWER: &str = "a newer Workdeck build";

pub const WORKDECK_DAEMON_RESTART_COMMAND: &str = "`workdeck daemon restart`";
pub const WORKDECK_WINDOW_RELAUNCH_CLAUSE: &str = "must be relaunched, losing their notes.";

/// The notice a window keeps while it waits out an incompatible daemon.
pub const WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE: &str =
    "Session daemon is a different Workdeck build. Run `workdeck daemon restart`.";

/// Tells a newer window how to replace the daemon without relaunching itself.
pub const WORKDECK_DAEMON_CLIENT_NEWER_MESSAGE: &str =
    "Session daemon is an older Workdeck build. Run `workdeck daemon restart`.";

/// Tells an older window to relaunch because replacing a newer daemon cannot upgrade it.
pub const WORKDECK_DAEMON_CLIENT_OLDER_MESSAGE: &str =
    "Session daemon is newer; relaunch this window (notes are lost).";

/// Covers a refused registration after the daemon accepted the window's hello.
pub const WORKDECK_DAEMON_REGISTRATION_REJECTED_MESSAGE: &str =
    "Session daemon rejected this window. Run `workdeck daemon restart`.";

/// Names the attached windows without guessing when the daemon cannot report a count.
#[must_use]
pub fn describe_attached_windows(count: Option<usize>) -> String {
    match count {
        None => "an unknown number of attached windows".into(),
        Some(1) => "1 attached window".into(),
        Some(count) => format!("{count} attached windows"),
    }
}

/// States the restart cost shared by agent remedies and confirmation prompts.
#[must_use]
pub fn daemon_restart_disconnects(count: Option<usize>) -> String {
    format!(
        "Restarting disconnects {}",
        describe_attached_windows(count)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sticky_notice_fits_an_eighty_column_status_line() {
        for notice in [
            WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE,
            WORKDECK_DAEMON_CLIENT_NEWER_MESSAGE,
            WORKDECK_DAEMON_CLIENT_OLDER_MESSAGE,
            WORKDECK_DAEMON_REGISTRATION_REJECTED_MESSAGE,
        ] {
            assert!(notice.chars().count() <= 78, "{notice}");
        }
    }

    #[test]
    fn counts_windows_without_guessing() {
        assert_eq!(
            describe_attached_windows(None),
            "an unknown number of attached windows"
        );
        assert_eq!(describe_attached_windows(Some(1)), "1 attached window");
        assert_eq!(describe_attached_windows(Some(2)), "2 attached windows");
        assert_eq!(
            daemon_restart_disconnects(Some(2)),
            "Restarting disconnects 2 attached windows"
        );
        assert_eq!(
            daemon_restart_disconnects(None),
            "Restarting disconnects an unknown number of attached windows"
        );
    }
}
