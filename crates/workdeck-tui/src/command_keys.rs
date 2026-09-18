//! Command-registry helpers built on the public extension key grammar.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use workdeck_extension_api::{
    ExtensionKeyEvent, ParsedKeyChord, matches_key_chord, parse_key_chord,
};

static PARSED_CHORD_CACHE: OnceLock<Mutex<HashMap<String, Option<ParsedKeyChord>>>> =
    OnceLock::new();

/// The two binding declaration shapes accepted by the command registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredKeyBinding<'a> {
    Absent,
    One(&'a str),
    Many(&'a [&'a str]),
}

/// Normalize one declared binding into the list of chords it names.
#[must_use]
pub fn to_key_chord_list(binding: DeclaredKeyBinding<'_>) -> Vec<String> {
    match binding {
        DeclaredKeyBinding::Absent => Vec::new(),
        DeclaredKeyBinding::One(chord) => vec![chord.to_owned()],
        DeclaredKeyBinding::Many(chords) => {
            chords.iter().map(|chord| (*chord).to_owned()).collect()
        }
    }
}

/// Parse one chord, memoizing both valid and invalid declarations.
#[must_use]
pub fn parse_key_chord_or_none(chord: &str) -> Option<ParsedKeyChord> {
    let cache = PARSED_CHORD_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(parsed) = cache.get(chord) {
        return parsed.clone();
    }
    let parsed = parse_key_chord(chord).ok();
    cache.insert(chord.to_owned(), parsed.clone());
    parsed
}

/// Preparsed matcher accepting any valid chord and skipping invalid entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnyKeyChordMatcher {
    parsed: Vec<ParsedKeyChord>,
}

impl AnyKeyChordMatcher {
    #[must_use]
    pub fn matches(&self, key: &ExtensionKeyEvent) -> bool {
        self.parsed
            .iter()
            .any(|chord| matches_key_chord(chord, key))
    }
}

/// Build one matcher that accepts any declared, parseable chord.
#[must_use]
pub fn matches_any_key_chord(chords: &[impl AsRef<str>]) -> AnyKeyChordMatcher {
    AnyKeyChordMatcher {
        parsed: chords
            .iter()
            .filter_map(|chord| parse_key_chord_or_none(chord.as_ref()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widens_both_declared_binding_forms() {
        assert!(to_key_chord_list(DeclaredKeyBinding::Absent).is_empty());
        assert_eq!(to_key_chord_list(DeclaredKeyBinding::One("y")), ["y"]);
        assert_eq!(
            to_key_chord_list(DeclaredKeyBinding::Many(&["y", "ctrl+g"])),
            ["y", "ctrl+g"]
        );
    }

    #[test]
    fn cached_parser_retains_success_and_failure_results() {
        assert_eq!(parse_key_chord_or_none("ctrl+r").unwrap().base, "r");
        assert_eq!(parse_key_chord_or_none("ctrl+r").unwrap().base, "r");
        assert!(parse_key_chord_or_none("ctlr+r").is_none());
        assert!(parse_key_chord_or_none("ctlr+r").is_none());
    }

    #[test]
    fn any_matcher_skips_invalid_chords_and_matches_valid_ones() {
        let matcher = matches_any_key_chord(&["ctlr+x", "y", "ctrl+g"]);
        let y = ExtensionKeyEvent {
            name: "y".into(),
            sequence: "y".into(),
            ..ExtensionKeyEvent::default()
        };
        assert!(matcher.matches(&y));
        assert!(!matches_any_key_chord(&["ctlr+x"]).matches(&y));
    }
}
