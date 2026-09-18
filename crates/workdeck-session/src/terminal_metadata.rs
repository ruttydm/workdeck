//! Terminal and multiplexer location discovery for live review sessions.

use crate::{SessionTerminalLocation, SessionTerminalMetadata};
use std::collections::BTreeMap;

fn trimmed(value: Option<&String>) -> Option<&str> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
}

fn push_location(locations: &mut Vec<SessionTerminalLocation>, location: SessionTerminalLocation) {
    if !locations.contains(&location) {
        locations.push(location);
    }
}

fn infer_location_source(program: Option<&str>) -> &'static str {
    match program
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("iterm.app" | "iterm2") => "iterm2",
        Some("ghostty") => "ghostty",
        Some("apple_terminal" | "apple terminal") => "terminal.app",
        _ => "terminal",
    }
}

fn parse_hierarchical_ids(session_id: &str) -> (Option<String>, Option<String>, Option<String>) {
    let prefix = session_id.split(':').next().unwrap_or_default().trim();
    let lower = prefix.to_ascii_lowercase();
    let Some(after_window) = lower.strip_prefix('w') else {
        return (None, None, None);
    };
    let window_digits = after_window.bytes().take_while(u8::is_ascii_digit).count();
    if window_digits == 0 {
        return (None, None, None);
    }
    let (window, after_window) = after_window.split_at(window_digits);
    let Some(after_tab) = after_window.strip_prefix('t') else {
        return (None, None, None);
    };
    let tab_digits = after_tab.bytes().take_while(u8::is_ascii_digit).count();
    if tab_digits == 0 {
        return (None, None, None);
    }
    let (tab, after_tab) = after_tab.split_at(tab_digits);
    let pane = if after_tab.is_empty() {
        None
    } else {
        let Some(after_pane) = after_tab.strip_prefix('p') else {
            return (None, None, None);
        };
        if after_pane.is_empty() || !after_pane.bytes().all(|byte| byte.is_ascii_digit()) {
            return (None, None, None);
        }
        Some(after_pane.to_owned())
    };
    (Some(window.into()), Some(tab.into()), pane)
}

/// Capture terminal- and multiplexer-facing location metadata for one live app session.
#[must_use]
pub fn resolve_session_terminal_metadata(
    env: &BTreeMap<String, String>,
    tty: Option<&str>,
) -> Option<SessionTerminalMetadata> {
    let term_program = trimmed(env.get("TERM_PROGRAM"));
    let lc_terminal = trimmed(env.get("LC_TERMINAL"));
    let program = if term_program.is_some_and(|value| value.eq_ignore_ascii_case("tmux")) {
        lc_terminal.or(term_program)
    } else {
        term_program.or(lc_terminal)
    };
    let mut locations = Vec::new();

    if let Some(tty) = tty.map(str::trim).filter(|value| !value.is_empty()) {
        push_location(
            &mut locations,
            SessionTerminalLocation {
                source: "tty".into(),
                tty: Some(tty.into()),
                ..SessionTerminalLocation::default()
            },
        );
    }
    if let Some(pane_id) = trimmed(env.get("TMUX_PANE")) {
        push_location(
            &mut locations,
            SessionTerminalLocation {
                source: "tmux".into(),
                pane_id: Some(pane_id.into()),
                ..SessionTerminalLocation::default()
            },
        );
    }
    let iterm_session_id = trimmed(env.get("ITERM_SESSION_ID"));
    if let Some(session_id) = iterm_session_id {
        let (window_id, tab_id, pane_id) = parse_hierarchical_ids(session_id);
        push_location(
            &mut locations,
            SessionTerminalLocation {
                source: "iterm2".into(),
                window_id,
                tab_id,
                pane_id,
                session_id: Some(session_id.into()),
                ..SessionTerminalLocation::default()
            },
        );
    }
    if let Some(session_id) = trimmed(env.get("TERM_SESSION_ID"))
        && Some(session_id) != iterm_session_id
    {
        let (window_id, tab_id, pane_id) = parse_hierarchical_ids(session_id);
        push_location(
            &mut locations,
            SessionTerminalLocation {
                source: infer_location_source(program).into(),
                window_id,
                tab_id,
                pane_id,
                session_id: Some(session_id.into()),
                ..SessionTerminalLocation::default()
            },
        );
    }
    if program.is_none() && locations.is_empty() {
        return None;
    }
    Some(SessionTerminalMetadata {
        program: program.map(str::to_owned),
        locations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }

    #[test]
    fn captures_tty_tmux_and_iterm2_identifiers_in_one_structure() {
        let terminal = resolve_session_terminal_metadata(
            &env(&[
                ("TERM_PROGRAM", "tmux"),
                ("LC_TERMINAL", "iTerm2"),
                ("ITERM_SESSION_ID", "w1t2p3:ABCDEF"),
                ("TMUX_PANE", "%7"),
            ]),
            Some("/dev/ttys003"),
        )
        .unwrap();
        assert_eq!(terminal.program.as_deref(), Some("iTerm2"));
        assert_eq!(
            terminal.locations,
            vec![
                SessionTerminalLocation {
                    source: "tty".into(),
                    tty: Some("/dev/ttys003".into()),
                    ..SessionTerminalLocation::default()
                },
                SessionTerminalLocation {
                    source: "tmux".into(),
                    pane_id: Some("%7".into()),
                    ..SessionTerminalLocation::default()
                },
                SessionTerminalLocation {
                    source: "iterm2".into(),
                    window_id: Some("1".into()),
                    tab_id: Some("2".into()),
                    pane_id: Some("3".into()),
                    session_id: Some("w1t2p3:ABCDEF".into()),
                    ..SessionTerminalLocation::default()
                },
            ]
        );
    }

    #[test]
    fn keeps_program_metadata_without_window_or_pane_ids() {
        assert_eq!(
            resolve_session_terminal_metadata(
                &env(&[("TERM_PROGRAM", "ghostty")]),
                Some("/dev/pts/4")
            ),
            Some(SessionTerminalMetadata {
                program: Some("ghostty".into()),
                locations: vec![SessionTerminalLocation {
                    source: "tty".into(),
                    tty: Some("/dev/pts/4".into()),
                    ..SessionTerminalLocation::default()
                }],
            })
        );
    }

    #[test]
    fn returns_none_without_terminal_metadata() {
        assert_eq!(
            resolve_session_terminal_metadata(&BTreeMap::new(), None),
            None
        );
    }

    #[test]
    fn deduplicates_overlapping_session_ids_and_rejects_partial_hierarchies() {
        let terminal = resolve_session_terminal_metadata(
            &env(&[
                ("TERM_PROGRAM", "iTerm.app"),
                ("ITERM_SESSION_ID", "w1t2:ABC"),
                ("TERM_SESSION_ID", "w1t2:ABC"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(terminal.locations.len(), 1);
        assert_eq!(terminal.locations[0].window_id.as_deref(), Some("1"));
        assert_eq!(terminal.locations[0].tab_id.as_deref(), Some("2"));
        assert_eq!(parse_hierarchical_ids("w1t:bad"), (None, None, None));
    }
}
