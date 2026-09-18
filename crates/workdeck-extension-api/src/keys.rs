//! Provider-neutral key-chord grammar shared by Workdeck and native extensions.

use crate::ExtensionKeyEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Host-resolved command bindings exposed to declarative native panes.
///
/// This is the subprocess-safe counterpart of Hunk's pane `keybindings`
/// helper. Invalid and shadowed user declarations have already been removed by
/// the host, so extensions observe the exact keys used by live dispatch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionResolvedKeybindings {
    #[serde(default)]
    pub keys: BTreeMap<String, Vec<String>>,
}

impl ExtensionResolvedKeybindings {
    #[must_use]
    pub fn get_keys(&self, command_id: &str) -> &[String] {
        self.keys
            .get(command_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    #[must_use]
    pub fn matches(&self, key: &ExtensionKeyEvent, command_id: &str) -> bool {
        self.get_keys(command_id)
            .iter()
            .any(|chord| matches_key(chord, key))
    }
}

/// Modifier-normalized description of one parsed chord.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedKeyChord {
    /// A named key (`escape`, `f10`, `pageup`) or one character.
    pub base: String,
    pub ctrl: bool,
    pub meta: bool,
    pub option: bool,
    pub shift: bool,
}

/// Exact diagnostic produced when a declared key chord is unusable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChordParseError(String);

impl KeyChordParseError {
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyChordParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for KeyChordParseError {}

fn is_named_key(base: &str) -> bool {
    matches!(
        base,
        "escape"
            | "tab"
            | "space"
            | "return"
            | "enter"
            | "backspace"
            | "delete"
            | "up"
            | "down"
            | "left"
            | "right"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
            | "insert"
            | "f1"
            | "f2"
            | "f3"
            | "f4"
            | "f5"
            | "f6"
            | "f7"
            | "f8"
            | "f9"
            | "f10"
            | "f11"
            | "f12"
    )
}

fn is_letter_base(base: &str) -> bool {
    base.len() == 1 && base.as_bytes()[0].is_ascii_lowercase()
}

/// Parse one binding chord, rejecting spelling mistakes and layout-dependent
/// `shift+symbol` forms at registration time.
pub fn parse_key_chord(chord: &str) -> Result<ParsedKeyChord, KeyChordParseError> {
    let tokens = chord
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        return if chord.trim() == "+" {
            Ok(ParsedKeyChord {
                base: "+".into(),
                ctrl: false,
                meta: false,
                option: false,
                shift: false,
            })
        } else {
            Err(KeyChordParseError(format!("Empty key chord \"{chord}\"")))
        };
    }

    let mut parsed = ParsedKeyChord {
        base: String::new(),
        ctrl: false,
        meta: false,
        option: false,
        shift: false,
    };

    for (index, token) in tokens.iter().enumerate() {
        let modifier = match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some("ctrl"),
            "meta" | "cmd" | "command" => Some("meta"),
            "alt" | "option" => Some("option"),
            "shift" => Some("shift"),
            _ => None,
        };
        if let Some(modifier) = modifier.filter(|_| index < tokens.len() - 1) {
            match modifier {
                "ctrl" => parsed.ctrl = true,
                "meta" => parsed.meta = true,
                "option" => parsed.option = true,
                "shift" => parsed.shift = true,
                _ => unreachable!("modifier table is exhaustive"),
            }
            continue;
        }

        if index != tokens.len() - 1 {
            return Err(KeyChordParseError(format!(
                "Unknown modifier \"{token}\" in key chord \"{chord}\""
            )));
        }

        // JavaScript's `length === 1` is one UTF-16 code unit. Preserve that
        // boundary so astral characters are rejected exactly like Hunk.
        if token.encode_utf16().count() == 1 {
            let lower = token.to_lowercase();
            let upper = token.to_uppercase();
            if *token != lower && *token != upper {
                return Err(KeyChordParseError(format!(
                    "Unusable key \"{token}\" in key chord \"{chord}\""
                )));
            }

            if token.len() == 1 && token.as_bytes()[0].is_ascii_uppercase() {
                parsed.shift = true;
                parsed.base = token.to_ascii_lowercase();
            } else {
                parsed.base = (*token).to_owned();
            }
            continue;
        }

        let named = token.to_ascii_lowercase();
        if !is_named_key(&named) {
            return Err(KeyChordParseError(format!(
                "Unknown key \"{token}\" in key chord \"{chord}\""
            )));
        }
        parsed.base = named;
    }

    if parsed.shift && !is_named_key(&parsed.base) && !is_letter_base(&parsed.base) {
        return Err(KeyChordParseError(format!(
            "Key chord \"{chord}\" uses shift with \"{}\"; bind the shifted character itself instead (e.g. \"!\" rather than \"shift+1\")",
            parsed.base
        )));
    }

    Ok(parsed)
}

fn matches_control_character(parsed: &ParsedKeyChord, key: &ExtensionKeyEvent) -> bool {
    if !parsed.ctrl || parsed.meta || parsed.option || parsed.shift || !is_letter_base(&parsed.base)
    {
        return false;
    }
    if !key.name.is_empty() || key.meta || key.option || key.shift {
        return false;
    }

    let control = char::from(parsed.base.as_bytes()[0] - 0x60).to_string();
    key.sequence == control
}

fn matches_named_key(base: &str, key: &ExtensionKeyEvent) -> bool {
    let name = key.name.to_ascii_lowercase();
    name == base
        || (base == "return" && name == "enter")
        || (base == "enter" && name == "return")
        || (base == "space" && (name == " " || key.sequence == " "))
}

/// Match one normalized chord against the structural terminal event contract.
#[must_use]
pub fn matches_key_chord(parsed: &ParsedKeyChord, key: &ExtensionKeyEvent) -> bool {
    if matches_control_character(parsed, key) {
        return true;
    }
    if key.ctrl != parsed.ctrl || key.meta != parsed.meta || key.option != parsed.option {
        return false;
    }

    if is_named_key(&parsed.base) {
        return matches_named_key(&parsed.base, key) && key.shift == parsed.shift;
    }

    if is_letter_base(&parsed.base) {
        if parsed.shift {
            return key.sequence == parsed.base.to_ascii_uppercase()
                || (key.name == parsed.base && key.shift);
        }
        return (key.name == parsed.base || key.sequence == parsed.base) && !key.shift;
    }

    key.sequence == parsed.base || key.name == parsed.base
}

/// Parse and match in one call. Invalid chords deliberately match nothing.
#[must_use]
pub fn matches_key(chord: &str, key: &ExtensionKeyEvent) -> bool {
    parse_key_chord(chord).is_ok_and(|parsed| matches_key_chord(&parsed, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, sequence: &str) -> ExtensionKeyEvent {
        ExtensionKeyEvent {
            name: name.into(),
            sequence: sequence.into(),
            ..ExtensionKeyEvent::default()
        }
    }

    fn parsed(chord: &str) -> ParsedKeyChord {
        parse_key_chord(chord).unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn parses_plain_modifiers_named_and_uppercase_keys() {
        assert_eq!(
            parsed("y"),
            ParsedKeyChord {
                base: "y".into(),
                ctrl: false,
                meta: false,
                option: false,
                shift: false,
            }
        );
        assert_eq!(
            parsed("ctrl+shift+m"),
            ParsedKeyChord {
                base: "m".into(),
                ctrl: true,
                meta: false,
                option: false,
                shift: true,
            }
        );
        assert_eq!(parsed("F2").base, "f2");
        assert!(parsed("alt+left").option);
        assert_eq!(parsed("G"), parsed("shift+g"));
        assert_eq!(parsed("+").base, "+");
    }

    #[test]
    fn refuses_unknown_dangling_and_layout_dependent_chords() {
        for chord in [
            "f13",
            "ctlr+s",
            "ctrl",
            "ctrl+shift",
            "ctrl+",
            "",
            "shift+1",
            "shift+[",
            "ctrl+shift+.",
        ] {
            assert!(parse_key_chord(chord).is_err(), "{chord}");
        }
        assert_eq!(
            parse_key_chord("shift+1").unwrap_err().to_string(),
            "Key chord \"shift+1\" uses shift with \"1\"; bind the shifted character itself instead (e.g. \"!\" rather than \"shift+1\")"
        );
        assert!(parsed("shift+tab").shift);
        assert!(parsed("shift+g").shift);
    }

    #[test]
    fn resolved_pane_keybindings_round_trip_and_match_the_host_chords() {
        let keybindings = ExtensionResolvedKeybindings {
            keys: BTreeMap::from([
                ("workdeck.review.nextFile".into(), vec!["ctrl+n".into()]),
                ("probe.blocked".into(), Vec::new()),
            ]),
        };
        let encoded = serde_json::to_value(&keybindings).unwrap();
        let decoded: ExtensionResolvedKeybindings = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.get_keys("workdeck.review.nextFile"), ["ctrl+n"]);
        assert!(decoded.get_keys("probe.blocked").is_empty());
        assert!(decoded.matches(
            &ExtensionKeyEvent {
                name: "n".into(),
                ctrl: true,
                ..ExtensionKeyEvent::default()
            },
            "workdeck.review.nextFile"
        ));
    }

    #[test]
    fn letters_require_exact_shift_except_uppercase_sequence_fallback() {
        let mut lower = event("g", "g");
        assert!(matches_key_chord(&parsed("g"), &lower));
        lower.sequence = "G".into();
        lower.shift = true;
        assert!(!matches_key_chord(&parsed("g"), &lower));

        assert!(matches_key_chord(&parsed("G"), &lower));
        lower.shift = false;
        assert!(matches_key_chord(&parsed("G"), &lower));
        lower.sequence = "g".into();
        assert!(!matches_key_chord(&parsed("G"), &lower));
    }

    #[test]
    fn modifiers_match_exactly() {
        let mut key = event("r", "");
        key.ctrl = true;
        assert!(matches_key_chord(&parsed("ctrl+r"), &key));
        key.ctrl = false;
        assert!(!matches_key_chord(&parsed("ctrl+r"), &key));
        key.ctrl = true;
        assert!(!matches_key_chord(&parsed("r"), &key));
    }

    #[test]
    fn symbols_match_sequence_regardless_of_shift() {
        let mut key = event("[", "{");
        key.shift = true;
        assert!(matches_key_chord(&parsed("{"), &key));
        assert!(matches_key_chord(&parsed("{"), &event("", "{")));
    }

    #[test]
    fn named_aliases_match_and_shift_remains_exact() {
        assert!(matches_key_chord(&parsed("f2"), &event("f2", "")));
        assert!(matches_key_chord(&parsed("enter"), &event("return", "")));
        assert!(matches_key_chord(&parsed("pageup"), &event("pageup", "")));
        assert!(matches_key_chord(&parsed("space"), &event("space", "")));
        assert!(matches_key_chord(&parsed("space"), &event("", " ")));

        let mut shifted_tab = event("tab", "");
        shifted_tab.shift = true;
        assert!(matches_key_chord(&parsed("shift+tab"), &shifted_tab));
        shifted_tab.shift = false;
        assert!(!matches_key_chord(&parsed("shift+tab"), &shifted_tab));
        shifted_tab.shift = true;
        assert!(!matches_key_chord(&parsed("tab"), &shifted_tab));
    }

    #[test]
    fn bare_control_characters_have_the_same_narrow_compatibility_net() {
        assert!(matches_key_chord(&parsed("ctrl+s"), &event("", "\u{13}")));
        let mut ctrl_s = event("", "\u{13}");
        ctrl_s.ctrl = true;
        assert!(matches_key_chord(&parsed("ctrl+s"), &ctrl_s));
        assert!(matches_key_chord(&parsed("ctrl+a"), &event("", "\u{1}")));
        assert!(matches_key_chord(&parsed("ctrl+z"), &event("", "\u{1a}")));
        assert!(!matches_key_chord(&parsed("ctrl+s"), &event("", "\u{1}")));
        assert!(!matches_key_chord(&parsed("s"), &event("", "\u{13}")));
        assert!(matches_key_chord(&parsed("s"), &event("s", "s")));
    }

    #[test]
    fn named_or_extra_modifier_events_never_use_control_character_fallback() {
        assert!(!matches_key_chord(
            &parsed("ctrl+i"),
            &event("tab", "\u{9}")
        ));
        assert!(!matches_key_chord(
            &parsed("ctrl+m"),
            &event("return", "\r")
        ));
        assert!(matches_key_chord(&parsed("tab"), &event("tab", "\u{9}")));
        assert!(matches_key_chord(&parsed("enter"), &event("return", "\r")));
        assert!(!matches_key_chord(&parsed("ctrl+s"), &event("s", "\u{13}")));
        let mut named_ctrl_s = event("s", "\u{13}");
        named_ctrl_s.ctrl = true;
        assert!(matches_key_chord(&parsed("ctrl+s"), &named_ctrl_s));

        for chord in ["ctrl+shift+s", "ctrl+meta+s", "ctrl+alt+s"] {
            assert!(!matches_key_chord(&parsed(chord), &event("", "\u{13}")));
        }
        for modifier in ["shift", "meta", "option"] {
            let mut key = event("", "\u{13}");
            match modifier {
                "shift" => key.shift = true,
                "meta" => key.meta = true,
                "option" => key.option = true,
                _ => unreachable!(),
            }
            assert!(!matches_key_chord(&parsed("ctrl+s"), &key));
        }
    }

    #[test]
    fn convenience_matcher_parses_or_rejects() {
        let mut ctrl_n = event("n", "");
        ctrl_n.ctrl = true;
        assert!(matches_key("ctrl+n", &ctrl_n));
        ctrl_n.ctrl = false;
        assert!(!matches_key("ctrl+n", &ctrl_n));
        let mut upper_g = event("g", "G");
        upper_g.shift = true;
        assert!(matches_key("G", &upper_g));
        assert!(!matches_key("ctlr+s", &ctrl_n));
        assert!(!matches_key("", &ctrl_n));
        assert!(!matches_key("shift+1", &ctrl_n));
    }

    #[test]
    fn absent_wire_fields_deserialize_to_the_structural_empty_event() {
        assert_eq!(
            serde_json::from_str::<ExtensionKeyEvent>("{}").unwrap(),
            ExtensionKeyEvent::default()
        );
    }
}
