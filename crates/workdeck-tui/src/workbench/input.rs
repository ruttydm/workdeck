use super::{DraftKey, MutationTarget};
use crate::{insert_filter_character, remove_filter_character_at, remove_filter_character_before};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const MAX_FIELD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub(super) enum FormKind {
    Planning,
    Draft(DraftKey),
    Filter,
    Status(MutationTarget),
    Assign(MutationTarget),
    Priority(MutationTarget),
    Labels(MutationTarget),
    LinkFile {
        target: MutationTarget,
        link: workdeck_pm::SourceLink,
    },
    Links(Vec<super::IssueNavigation<()>>),
}

#[derive(Debug, Clone)]
pub(super) struct TextField {
    pub label: &'static str,
    pub value: String,
    pub cursor: usize,
    pub multiline: bool,
}

impl TextField {
    pub fn new(label: &'static str, value: String, multiline: bool) -> Self {
        Self {
            label,
            cursor: value.chars().count(),
            value,
            multiline,
        }
    }

    pub fn insert(&mut self, text: &str) {
        for character in text.chars() {
            if self.value.len().saturating_add(character.len_utf8()) > MAX_FIELD_BYTES {
                break;
            }
            if character.is_control() && !(self.multiline && character == '\n') {
                continue;
            }
            insert_filter_character(&mut self.value, &mut self.cursor, character);
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct WorkbenchForm {
    pub kind: FormKind,
    pub title: String,
    pub fields: Vec<TextField>,
    pub selected: usize,
    pub help: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FormAction {
    Edited,
    Submit,
    Close,
}

impl WorkbenchForm {
    pub fn new(kind: FormKind, title: impl Into<String>, fields: Vec<TextField>) -> Self {
        Self {
            kind,
            title: title.into(),
            fields,
            selected: 0,
            help: Vec::new(),
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> FormAction {
        if key.code == KeyCode::Esc {
            return FormAction::Close;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
            return FormAction::Submit;
        }
        if self.fields.is_empty() {
            return if key.code == KeyCode::Enter {
                FormAction::Submit
            } else {
                FormAction::Edited
            };
        }
        let field = &mut self.fields[self.selected];
        match key.code {
            KeyCode::BackTab => {
                self.selected = self
                    .selected
                    .checked_sub(1)
                    .unwrap_or(self.fields.len() - 1)
            }
            KeyCode::Tab => self.selected = (self.selected + 1) % self.fields.len(),
            KeyCode::Enter if !field.multiline => {
                if self.selected + 1 == self.fields.len() {
                    return FormAction::Submit;
                }
                self.selected += 1;
            }
            KeyCode::Enter => field.insert("\n"),
            KeyCode::Backspace => {
                remove_filter_character_before(&mut field.value, &mut field.cursor)
            }
            KeyCode::Delete => remove_filter_character_at(&mut field.value, &mut field.cursor),
            KeyCode::Left => field.cursor = field.cursor.saturating_sub(1),
            KeyCode::Right => {
                field.cursor = field
                    .cursor
                    .saturating_add(1)
                    .min(field.value.chars().count())
            }
            KeyCode::Home => field.cursor = 0,
            KeyCode::End => field.cursor = field.value.chars().count(),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                field.value.clear();
                field.cursor = 0;
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                field.insert(&character.to_string())
            }
            _ => {}
        }
        FormAction::Edited
    }
}
