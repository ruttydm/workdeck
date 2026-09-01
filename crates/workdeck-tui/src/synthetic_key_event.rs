//! Complete host-side key event synthesized from a parsed binding chord.

use workdeck_extension_api::{ExtensionKeyEvent, ParsedKeyChord};

/// Native equivalent of the OpenTUI event Hunk used for matcher probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntheticKeyEvent {
    pub key: ExtensionKeyEvent,
    pub event_type: &'static str,
    pub source: &'static str,
    pub default_prevented: bool,
    pub propagation_stopped: bool,
}

impl SyntheticKeyEvent {
    #[must_use]
    pub fn as_extension_event(&self) -> &ExtensionKeyEvent {
        &self.key
    }

    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }
}

/// Synthesize the complete key event described by one normalized chord.
#[must_use]
pub fn synthesize_key_event(chord: &ParsedKeyChord) -> SyntheticKeyEvent {
    // Hunk measured JavaScript string length, i.e. UTF-16 code units.
    let named = chord.base.encode_utf16().count() > 1;
    let letter = chord.base.len() == 1 && chord.base.as_bytes()[0].is_ascii_lowercase();
    let sequence = if named {
        String::new()
    } else if letter && chord.shift {
        chord.base.to_ascii_uppercase()
    } else {
        chord.base.clone()
    };
    let name = if named || letter {
        chord.base.clone()
    } else {
        sequence.clone()
    };

    SyntheticKeyEvent {
        key: ExtensionKeyEvent {
            name,
            sequence,
            ctrl: chord.ctrl,
            meta: chord.meta,
            option: chord.option,
            shift: chord.shift,
        },
        event_type: "press",
        source: "raw",
        default_prevented: false,
        propagation_stopped: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_extension_api::{matches_key_chord, parse_key_chord};

    #[test]
    fn every_chord_form_round_trips_through_the_matcher() {
        for chord in [
            "y",
            "G",
            "ctrl+shift+m",
            "f10",
            "{",
            "alt+left",
            ".",
            "space",
        ] {
            let parsed = parse_key_chord(chord).unwrap();
            let event = synthesize_key_event(&parsed);
            assert!(
                matches_key_chord(&parsed, event.as_extension_event()),
                "{chord}"
            );
        }
    }

    #[test]
    fn event_has_lifecycle_fields_and_propagation_controls() {
        let parsed = parse_key_chord("ctrl+r").unwrap();
        let mut event = synthesize_key_event(&parsed);
        assert_eq!(event.event_type, "press");
        assert_eq!(event.source, "raw");
        event.prevent_default();
        event.stop_propagation();
        assert!(event.default_prevented);
        assert!(event.propagation_stopped);
    }
}
