//! Renderer-neutral user keybinding configuration.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserKeyBinding {
    Disabled,
    Chord(String),
    Chords(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserKeyBindingEntry {
    pub command_id: String,
    pub binding: UserKeyBinding,
}

impl UserKeyBindingEntry {
    #[must_use]
    pub fn new(command_id: impl Into<String>, binding: UserKeyBinding) -> Self {
        Self {
            command_id: command_id.into(),
            binding,
        }
    }
}
