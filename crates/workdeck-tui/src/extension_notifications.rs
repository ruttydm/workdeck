//! Terminal-safe, bounded presentation of native extension notifications.

use ratatui::style::Color;
use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};
use workdeck_extension_api::{ExtensionNotification, ExtensionNotifyType};

/// Match extension output to Workdeck's existing transient status duration.
pub const EXTENSION_TOAST_DURATION_MS: u64 = 4_000;

/// Maximum visible pending depth; a looping extension cannot create a backlog.
pub const EXTENSION_TOAST_QUEUE_LIMIT: usize = 8;

const TOAST_PREFIX: &str = "ext";
const TOAST_CHROME_COLUMNS: usize = TOAST_PREFIX.len() + 3;

/// Theme tokens needed by extension notification chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionToastTheme {
    pub badge_removed: Color,
    pub file_modified: Color,
    pub badge_neutral: Color,
}

/// Select the same semantic theme role as Hunk for each notification severity.
#[must_use]
pub const fn extension_toast_color(
    notification_type: ExtensionNotifyType,
    theme: ExtensionToastTheme,
) -> Color {
    match notification_type {
        ExtensionNotifyType::Error => theme.badge_removed,
        ExtensionNotifyType::Warning => theme.file_modified,
        ExtensionNotifyType::Info => theme.badge_neutral,
    }
}

/// Fit extension-authored text into one terminal row after stripping controls.
#[must_use]
pub fn extension_toast_message(message: &str, terminal_width: u16) -> String {
    let sanitized = sanitize_terminal_text(
        message,
        SanitizeOptions {
            preserve_newlines: true,
            preserve_tabs: true,
            preserve_ansi_style: false,
        },
    );
    let single_line = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let available = usize::from(terminal_width)
        .saturating_sub(TOAST_CHROME_COLUMNS)
        .max(8);
    let length = single_line.chars().count();
    if length <= available {
        return single_line;
    }
    let mut fitted = single_line
        .chars()
        .take(available.saturating_sub(1))
        .collect::<String>();
    fitted.push('…');
    fitted
}

#[must_use]
pub const fn extension_toast_prefix() -> &'static str {
    TOAST_PREFIX
}

/// Append newest-last and drop the oldest entries past the fixed queue cap.
#[must_use]
pub fn enqueue_extension_notification(
    queue: &[ExtensionNotification],
    notification: ExtensionNotification,
) -> Vec<ExtensionNotification> {
    let retained = queue
        .len()
        .saturating_add(1)
        .saturating_sub(EXTENSION_TOAST_QUEUE_LIMIT);
    let mut next = Vec::with_capacity(
        queue
            .len()
            .saturating_add(1)
            .min(EXTENSION_TOAST_QUEUE_LIMIT),
    );
    next.extend(queue.iter().skip(retained).cloned());
    next.push(notification);
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: u64) -> ExtensionNotification {
        ExtensionNotification {
            id,
            message: format!("message {id}"),
            notification_type: ExtensionNotifyType::Info,
        }
    }

    #[test]
    fn toast_message_collapses_whitespace_and_keeps_short_text_intact() {
        assert_eq!(
            extension_toast_message("  loaded\n  3   files \t", 80),
            "loaded 3 files"
        );
    }

    #[test]
    fn toast_message_truncates_at_the_width_left_after_chrome() {
        let fitted = extension_toast_message(&"x".repeat(40), 20);
        assert_eq!(fitted, format!("{}…", "x".repeat(13)));
        assert_eq!(fitted.chars().count(), 14);
    }

    #[test]
    fn toast_message_keeps_the_exact_boundary_untruncated() {
        let available = 30 - "ext".len() - 3;
        assert_eq!(
            extension_toast_message(&"y".repeat(available), 30),
            "y".repeat(available)
        );
        assert!(extension_toast_message(&"y".repeat(available + 1), 30).ends_with('…'));
    }

    #[test]
    fn toast_message_has_a_readable_floor_on_a_narrow_terminal() {
        assert_eq!(
            extension_toast_message(&"z".repeat(40), 4),
            format!("{}…", "z".repeat(7))
        );
    }

    #[test]
    fn toast_message_strips_terminal_control_sequences() {
        let hostile = "\x1b[2Jcleared\x1b]0;retitled\x07 \x1b[31mred\x1b[0m";
        let fitted = extension_toast_message(hostile, 200);
        assert_eq!(fitted, "cleared red");
        assert!(!fitted.contains('\x1b'));
    }

    #[test]
    fn toast_message_strips_c1_sequences_and_stray_controls() {
        assert_eq!(
            extension_toast_message("before\u{9b}31mafter\0", 200),
            "beforeafter"
        );
    }

    #[test]
    fn toast_queue_appends_newest_last() {
        let queue = enqueue_extension_notification(&[notification(1)], notification(2));
        assert_eq!(
            queue.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn toast_queue_drops_oldest_entries_once_capped() {
        let mut queue = Vec::new();
        for id in 1..=u64::try_from(EXTENSION_TOAST_QUEUE_LIMIT).unwrap() + 3 {
            queue = enqueue_extension_notification(&queue, notification(id));
        }
        assert_eq!(queue.len(), EXTENSION_TOAST_QUEUE_LIMIT);
        assert_eq!(queue.first().map(|entry| entry.id), Some(4));
        assert_eq!(
            queue.last().map(|entry| entry.id),
            Some(u64::try_from(EXTENSION_TOAST_QUEUE_LIMIT).unwrap() + 3)
        );
    }

    #[test]
    fn toast_queue_returns_a_new_allocation_without_mutating_current() {
        let current = vec![notification(1)];
        let current_ptr = current.as_ptr();
        let next = enqueue_extension_notification(&current, notification(2));
        assert_eq!(current.len(), 1);
        assert_ne!(next.as_ptr(), current_ptr);
    }
}
