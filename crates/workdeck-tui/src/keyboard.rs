//! Modal-owned key predicates spanning terminal input encodings.

const CTRL_S: &str = "\u{13}";
const CTRL_S_CSI_U: &str = "\u{1b}[115;5u";

/// Borrowed terminal event including the raw channel intentionally excluded
/// from the method-free extension event contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalKeyEvent<'a> {
    pub name: Option<&'a str>,
    pub sequence: Option<&'a str>,
    pub raw: Option<&'a str>,
    pub ctrl: bool,
}

/// Normalize Escape aliases emitted by raw, named, and embedded input paths.
#[must_use]
pub fn is_escape_key(key: TerminalKeyEvent<'_>) -> bool {
    matches!(key.name, Some("escape" | "esc" | "Escape"))
        || key.sequence == Some("\u{1b}")
        || key.raw == Some("\u{1b}")
}

/// Match Ctrl-S across raw C0, Kitty/CSI-u, and tmux control-mode encodings.
///
/// This deliberately remains wider than the public chord grammar: a raw C0
/// byte saves a draft even when the event carries other modifier metadata.
#[must_use]
pub fn is_save_draft_note_key(key: TerminalKeyEvent<'_>) -> bool {
    let named_s = key.name.is_some_and(|name| name.eq_ignore_ascii_case("s"));
    (key.ctrl && (named_s || key.sequence == Some("s") || key.sequence == Some(CTRL_S)))
        || key.sequence == Some(CTRL_S)
        || key.raw == Some(CTRL_S)
        || key.sequence == Some(CTRL_S_CSI_U)
        || key.raw == Some(CTRL_S_CSI_U)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_accepts_only_hunks_named_and_raw_aliases() {
        for key in [
            TerminalKeyEvent {
                name: Some("escape"),
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                name: Some("esc"),
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                name: Some("Escape"),
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                sequence: Some("\u{1b}"),
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                raw: Some("\u{1b}"),
                ..TerminalKeyEvent::default()
            },
        ] {
            assert!(is_escape_key(key));
        }
        assert!(!is_escape_key(TerminalKeyEvent {
            name: Some("ESCAPE"),
            ..TerminalKeyEvent::default()
        }));
        assert!(!is_escape_key(TerminalKeyEvent {
            name: Some("q"),
            ..TerminalKeyEvent::default()
        }));
    }

    #[test]
    fn save_draft_accepts_decoded_c0_raw_c0_and_csi_u() {
        for key in [
            TerminalKeyEvent {
                name: Some("s"),
                ctrl: true,
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                name: Some("S"),
                ctrl: true,
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                sequence: Some("s"),
                ctrl: true,
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                sequence: Some(CTRL_S),
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                raw: Some(CTRL_S),
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                sequence: Some(CTRL_S_CSI_U),
                ..TerminalKeyEvent::default()
            },
            TerminalKeyEvent {
                raw: Some(CTRL_S_CSI_U),
                ..TerminalKeyEvent::default()
            },
        ] {
            assert!(is_save_draft_note_key(key));
        }
        assert!(!is_save_draft_note_key(TerminalKeyEvent {
            name: Some("s"),
            ..TerminalKeyEvent::default()
        }));
        assert!(!is_save_draft_note_key(TerminalKeyEvent {
            name: Some("x"),
            ctrl: true,
            ..TerminalKeyEvent::default()
        }));
    }
}
