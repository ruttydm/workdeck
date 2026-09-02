//! Vim-normal navigation grammar translated from Hunk's bundled example.

use workdeck_extension_api::{ExtensionHostAction, ExtensionKeyEvent, KeyRoutingResult};

const MAX_COUNT: u16 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    G,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimCommandResult {
    Handled,
    Empty,
    Unknown,
}

#[derive(Debug, Default)]
pub struct VimNavigationState {
    count_text: String,
    prefix: Option<Prefix>,
}

impl VimNavigationState {
    pub fn reset(&mut self) {
        self.count_text.clear();
        self.prefix = None;
    }

    #[must_use]
    pub fn handle_key(
        &mut self,
        key: &ExtensionKeyEvent,
    ) -> (KeyRoutingResult, Vec<ExtensionHostAction>) {
        let text = if key.sequence.is_empty() {
            key.name.as_str()
        } else {
            key.sequence.as_str()
        };
        let shifted_g = text == "G" || (key.name == "g" && key.shift);

        if key.ctrl {
            let command = if !key.meta && !key.option && !key.shift {
                match if key.name.is_empty() {
                    key.sequence.as_str()
                } else {
                    key.name.as_str()
                } {
                    "d" => Some("workdeck.review.half-page-down"),
                    "u" => Some("workdeck.review.half-page-up"),
                    _ => None,
                }
            } else {
                None
            };
            return match command {
                Some(command) => self.execute_relative(command),
                None => {
                    self.reset();
                    (KeyRoutingResult::Pass, Vec::new())
                }
            };
        }
        if key.meta || key.option {
            self.reset();
            return (KeyRoutingResult::Pass, Vec::new());
        }

        if text.len() == 1 && text.as_bytes()[0].is_ascii_digit() {
            if text == "0" && self.count_text.is_empty() {
                self.reset();
                return (KeyRoutingResult::Pass, Vec::new());
            }
            if self.prefix.is_some() {
                self.reset();
                return (KeyRoutingResult::Pass, Vec::new());
            }
            let next = format!("{}{text}", self.count_text)
                .parse::<u32>()
                .unwrap_or(u32::from(MAX_COUNT))
                .min(u32::from(MAX_COUNT));
            self.count_text = next.to_string();
            return (KeyRoutingResult::Handled, Vec::new());
        }

        if self.prefix == Some(Prefix::G) {
            let matches = text == "g" && !shifted_g;
            self.reset();
            return if matches {
                handled_action("workdeck.review.jump-to-top", None)
            } else {
                (KeyRoutingResult::Pass, Vec::new())
            };
        }
        if self.prefix == Some(Prefix::Z) {
            let command = match text {
                "t" => Some("workdeck.review.align-current-line-top"),
                "z" => Some("workdeck.review.align-current-line-center"),
                "b" => Some("workdeck.review.align-current-line-bottom"),
                _ => None,
            };
            self.reset();
            return command.map_or_else(
                || (KeyRoutingResult::Pass, Vec::new()),
                |command| handled_action(command, None),
            );
        }

        if text == ":" {
            self.reset();
            return (KeyRoutingResult::Pass, Vec::new());
        }
        if let Some(command) = match text {
            "j" => Some("workdeck.review.step-down"),
            "k" => Some("workdeck.review.step-up"),
            "[" => Some("workdeck.review.previous-hunk"),
            "]" => Some("workdeck.review.next-hunk"),
            _ => None,
        } {
            return self.execute_relative(command);
        }
        if shifted_g {
            self.reset();
            return handled_action("workdeck.review.jump-to-bottom", None);
        }
        if text == "g" || text == "z" {
            self.prefix = Some(if text == "g" { Prefix::G } else { Prefix::Z });
            return (KeyRoutingResult::Handled, Vec::new());
        }

        self.reset();
        (KeyRoutingResult::Pass, Vec::new())
    }

    fn execute_relative(&mut self, command: &str) -> (KeyRoutingResult, Vec<ExtensionHostAction>) {
        let count = self
            .count_text
            .parse::<u16>()
            .ok()
            .filter(|count| *count > 0)
            .unwrap_or(1);
        self.reset();
        handled_action(command, Some(count))
    }
}

#[must_use]
pub fn execute_vim_command(input: &str) -> (VimCommandResult, Vec<ExtensionHostAction>) {
    let command = input.trim();
    let command = command.strip_prefix(':').unwrap_or(command).trim();
    match command {
        "" => (VimCommandResult::Empty, Vec::new()),
        "top" => (
            VimCommandResult::Handled,
            handled_action("workdeck.review.jump-to-top", None).1,
        ),
        "bottom" => (
            VimCommandResult::Handled,
            handled_action("workdeck.review.jump-to-bottom", None).1,
        ),
        _ => (VimCommandResult::Unknown, Vec::new()),
    }
}

fn handled_action(id: &str, count: Option<u16>) -> (KeyRoutingResult, Vec<ExtensionHostAction>) {
    (
        KeyRoutingResult::Handled,
        vec![ExtensionHostAction::ExecuteReviewCommand {
            id: id.into(),
            count,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sequence: &str) -> ExtensionKeyEvent {
        ExtensionKeyEvent {
            name: sequence.to_ascii_lowercase(),
            sequence: sequence.into(),
            ..ExtensionKeyEvent::default()
        }
    }

    fn action_id(actions: &[ExtensionHostAction]) -> (&str, Option<u16>) {
        match actions.first().unwrap() {
            ExtensionHostAction::ExecuteReviewCommand { id, count } => (id, *count),
            action => panic!("unexpected action: {action:?}"),
        }
    }

    #[test]
    fn counts_saturate_and_apply_to_relative_commands() {
        let mut state = VimNavigationState::default();
        for digit in ["9", "9", "9", "9", "9", "j"] {
            let (result, actions) = state.handle_key(&key(digit));
            assert_eq!(result, KeyRoutingResult::Handled);
            if digit == "j" {
                assert_eq!(
                    action_id(&actions),
                    ("workdeck.review.step-down", Some(10_000))
                );
            }
        }
    }

    #[test]
    fn bare_zero_and_invalid_prefix_continuations_pass_and_reset() {
        let mut state = VimNavigationState::default();
        assert_eq!(state.handle_key(&key("0")).0, KeyRoutingResult::Pass);
        assert_eq!(state.handle_key(&key("g")).0, KeyRoutingResult::Handled);
        assert_eq!(state.handle_key(&key("x")).0, KeyRoutingResult::Pass);
        assert_eq!(action_id(&state.handle_key(&key("j")).1).1, Some(1));
    }

    #[test]
    fn supports_jumps_alignment_controls_and_command_line_pass_through() {
        let mut state = VimNavigationState::default();
        let _ = state.handle_key(&key("g"));
        assert_eq!(
            action_id(&state.handle_key(&key("g")).1).0,
            "workdeck.review.jump-to-top"
        );
        assert_eq!(
            action_id(&state.handle_key(&key("G")).1).0,
            "workdeck.review.jump-to-bottom"
        );
        let _ = state.handle_key(&key("z"));
        assert_eq!(
            action_id(&state.handle_key(&key("b")).1).0,
            "workdeck.review.align-current-line-bottom"
        );
        let mut control = key("d");
        control.ctrl = true;
        assert_eq!(
            action_id(&state.handle_key(&control).1).0,
            "workdeck.review.half-page-down"
        );
        assert_eq!(
            state.handle_key(&key(":")),
            (KeyRoutingResult::Pass, Vec::new())
        );
    }

    #[test]
    fn ex_commands_trim_optional_colons_and_report_empty_or_unknown() {
        assert_eq!(execute_vim_command(" : top ").0, VimCommandResult::Handled);
        assert_eq!(execute_vim_command("bottom").0, VimCommandResult::Handled);
        assert_eq!(execute_vim_command(" : ").0, VimCommandResult::Empty);
        assert_eq!(execute_vim_command("middle").0, VimCommandResult::Unknown);
    }
}
