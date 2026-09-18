use std::collections::VecDeque;
use std::fmt;
use std::path::PathBuf;
use workdeck_diff::sanitize_terminal_line;

const DEFAULT_CONFIRM_LABEL: &str = "ok";
const DEFAULT_CANCEL_LABEL: &str = "cancel";
const MAX_CONFIRM_BODY_LINES: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionInputDialog {
    pub request_id: u64,
    pub extension_index: usize,
    pub extension_id: String,
    pub action_id: String,
    pub show_attribution: bool,
    pub title: String,
    pub placeholder: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionSelectDialog {
    pub request_id: u64,
    pub extension_index: usize,
    pub extension_id: String,
    pub action_id: String,
    pub show_attribution: bool,
    pub title: String,
    pub options: Vec<String>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionConfirmDialog {
    pub request_id: u64,
    pub extension_index: usize,
    pub extension_id: String,
    pub action_id: String,
    pub show_attribution: bool,
    pub title: String,
    pub body_lines: Vec<String>,
    pub confirm_label: String,
    pub cancel_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionWorkspaceWriteDialog {
    pub request_id: String,
    pub extension_index: usize,
    pub extension_id: String,
    pub file_id: String,
    pub path: String,
    pub absolute_path: PathBuf,
    pub root: PathBuf,
    pub text: String,
    pub review_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtensionDialogRequest {
    Input(ExtensionInputDialog),
    Select(ExtensionSelectDialog),
    Confirm(ExtensionConfirmDialog),
    Workspace {
        queue_id: u64,
        dialog: ExtensionWorkspaceWriteDialog,
    },
}

impl ExtensionDialogRequest {
    pub const fn request_id(&self) -> u64 {
        match self {
            Self::Input(dialog) => dialog.request_id,
            Self::Select(dialog) => dialog.request_id,
            Self::Confirm(dialog) => dialog.request_id,
            Self::Workspace { queue_id, .. } => *queue_id,
        }
    }

    pub const fn extension_index(&self) -> usize {
        match self {
            Self::Input(dialog) => dialog.extension_index,
            Self::Select(dialog) => dialog.extension_index,
            Self::Confirm(dialog) => dialog.extension_index,
            Self::Workspace { dialog, .. } => dialog.extension_index,
        }
    }

    pub fn extension_id(&self) -> &str {
        match self {
            Self::Input(dialog) => &dialog.extension_id,
            Self::Select(dialog) => &dialog.extension_id,
            Self::Confirm(dialog) => &dialog.extension_id,
            Self::Workspace { dialog, .. } => &dialog.extension_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtensionDialogAnswer {
    Input(Option<String>),
    Select(Option<String>),
    Confirm(bool),
    Workspace(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtensionDialogSettlement {
    pub request: ExtensionDialogRequest,
    pub answer: ExtensionDialogAnswer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExtensionDialogError {
    EmptyTitle { method: &'static str },
    EmptyOptions,
}

impl fmt::Display for ExtensionDialogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTitle { method } => {
                write!(formatter, "dialogs.{method} requires a non-empty title.")
            }
            Self::EmptyOptions => {
                formatter.write_str("dialogs.select requires at least one option.")
            }
        }
    }
}

impl std::error::Error for ExtensionDialogError {}

#[derive(Debug, Default)]
pub(crate) struct ExtensionDialogQueue {
    pending: VecDeque<ExtensionDialogRequest>,
    next_id: u64,
    closed: bool,
}

impl ExtensionDialogQueue {
    pub fn enqueue_input(
        &mut self,
        extension_index: usize,
        extension_id: impl Into<String>,
        action_id: impl Into<String>,
        title: &str,
        placeholder: &str,
        initial: Option<&str>,
    ) -> Result<Option<ExtensionDialogSettlement>, ExtensionDialogError> {
        let request_id = self.take_id();
        let request = ExtensionDialogRequest::Input(ExtensionInputDialog {
            request_id,
            extension_index,
            extension_id: extension_id.into(),
            action_id: action_id.into(),
            show_attribution: true,
            title: normalize_title("input", title)?,
            placeholder: normalize_label(placeholder, ""),
            value: initial.map(sanitize_terminal_line).unwrap_or_default(),
        });
        Ok(self.enqueue(request))
    }

    pub fn enqueue_select(
        &mut self,
        extension_index: usize,
        extension_id: impl Into<String>,
        action_id: impl Into<String>,
        title: &str,
        options: Vec<String>,
    ) -> Result<Option<ExtensionDialogSettlement>, ExtensionDialogError> {
        if options.is_empty() {
            return Err(ExtensionDialogError::EmptyOptions);
        }
        let request_id = self.take_id();
        let request = ExtensionDialogRequest::Select(ExtensionSelectDialog {
            request_id,
            extension_index,
            extension_id: extension_id.into(),
            action_id: action_id.into(),
            show_attribution: true,
            title: normalize_title("select", title)?,
            options: options
                .into_iter()
                .map(|option| sanitize_terminal_line(&option))
                .collect(),
            selected: 0,
        });
        Ok(self.enqueue(request))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_confirm(
        &mut self,
        extension_index: usize,
        extension_id: impl Into<String>,
        action_id: impl Into<String>,
        title: &str,
        body: &str,
        confirm_label: &str,
        cancel_label: Option<&str>,
    ) -> Result<Option<ExtensionDialogSettlement>, ExtensionDialogError> {
        self.enqueue_confirm_with_attribution(
            extension_index,
            extension_id,
            action_id,
            title,
            body,
            confirm_label,
            cancel_label,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn enqueue_confirm_with_attribution(
        &mut self,
        extension_index: usize,
        extension_id: impl Into<String>,
        action_id: impl Into<String>,
        title: &str,
        body: &str,
        confirm_label: &str,
        cancel_label: Option<&str>,
        show_attribution: bool,
    ) -> Result<Option<ExtensionDialogSettlement>, ExtensionDialogError> {
        let request_id = self.take_id();
        let request = ExtensionDialogRequest::Confirm(ExtensionConfirmDialog {
            request_id,
            extension_index,
            extension_id: extension_id.into(),
            action_id: action_id.into(),
            show_attribution,
            title: normalize_title("confirm", title)?,
            body_lines: normalize_body_lines(body),
            confirm_label: normalize_label(confirm_label, DEFAULT_CONFIRM_LABEL),
            cancel_label: normalize_label(cancel_label.unwrap_or_default(), DEFAULT_CANCEL_LABEL),
        });
        Ok(self.enqueue(request))
    }

    pub fn enqueue_workspace(
        &mut self,
        dialog: ExtensionWorkspaceWriteDialog,
    ) -> Option<ExtensionDialogSettlement> {
        let queue_id = self.take_id();
        self.enqueue(ExtensionDialogRequest::Workspace { queue_id, dialog })
    }

    pub fn current(&self) -> Option<&ExtensionDialogRequest> {
        self.pending.front()
    }

    pub fn current_mut(&mut self) -> Option<&mut ExtensionDialogRequest> {
        self.pending.front_mut()
    }

    pub fn accept(
        &mut self,
        request_id: u64,
        value: Option<String>,
    ) -> Option<ExtensionDialogSettlement> {
        let request = self.take_current(request_id)?;
        let answer = match &request {
            ExtensionDialogRequest::Confirm(_) => ExtensionDialogAnswer::Confirm(true),
            ExtensionDialogRequest::Select(_) => ExtensionDialogAnswer::Select(value),
            ExtensionDialogRequest::Input(_) => ExtensionDialogAnswer::Input(value),
            ExtensionDialogRequest::Workspace { .. } => ExtensionDialogAnswer::Workspace(true),
        };
        Some(ExtensionDialogSettlement { request, answer })
    }

    pub fn cancel(&mut self, request_id: u64) -> Option<ExtensionDialogSettlement> {
        let request = self.take_current(request_id)?;
        Some(cancel_settlement(request))
    }

    pub fn move_selection(&mut self, delta: isize) {
        let Some(ExtensionDialogRequest::Select(dialog)) = self.current_mut() else {
            return;
        };
        let count = dialog.options.len();
        if count == 0 {
            return;
        }
        dialog.selected = (dialog.selected as isize + delta).rem_euclid(count as isize) as usize;
    }

    pub fn pick_option(&mut self, index: usize) {
        let Some(ExtensionDialogRequest::Select(dialog)) = self.current_mut() else {
            return;
        };
        if index < dialog.options.len() {
            dialog.selected = index;
        }
    }

    pub fn update_input(&mut self, value: String) {
        if let Some(ExtensionDialogRequest::Input(dialog)) = self.current_mut() {
            dialog.value = value;
        }
    }

    pub fn cancel_all(&mut self) -> Vec<ExtensionDialogSettlement> {
        self.pending.drain(..).map(cancel_settlement).collect()
    }

    pub fn shutdown(&mut self) -> Vec<ExtensionDialogSettlement> {
        self.closed = true;
        self.cancel_all()
    }

    #[cfg(test)]
    pub fn remove_extension(&mut self, extension_index: usize) -> Vec<ExtensionDialogSettlement> {
        let mut retained = VecDeque::new();
        let mut removed = Vec::new();
        while let Some(request) = self.pending.pop_front() {
            if request.extension_index() == extension_index {
                removed.push(cancel_settlement(request));
            } else {
                retained.push_back(request);
            }
        }
        self.pending = retained;
        removed
    }

    pub fn cancel_current_for_extension(
        &mut self,
        extension_index: usize,
    ) -> Option<ExtensionDialogSettlement> {
        let request_id = self
            .current()
            .filter(|request| request.extension_index() == extension_index)?
            .request_id();
        self.cancel(request_id)
    }

    pub fn remove_workspace_for_file(
        &mut self,
        extension_index: usize,
        file_id: &str,
    ) -> Vec<ExtensionDialogSettlement> {
        let mut retained = VecDeque::new();
        let mut removed = Vec::new();
        while let Some(request) = self.pending.pop_front() {
            let matches = matches!(
                &request,
                ExtensionDialogRequest::Workspace { dialog, .. }
                    if dialog.extension_index == extension_index && dialog.file_id == file_id
            );
            if matches {
                removed.push(cancel_settlement(request));
            } else {
                retained.push_back(request);
            }
        }
        self.pending = retained;
        removed
    }

    fn take_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        id
    }

    fn enqueue(&mut self, request: ExtensionDialogRequest) -> Option<ExtensionDialogSettlement> {
        if self.closed {
            return Some(cancel_settlement(request));
        }
        self.pending.push_back(request);
        None
    }

    fn take_current(&mut self, request_id: u64) -> Option<ExtensionDialogRequest> {
        (self.current()?.request_id() == request_id)
            .then(|| self.pending.pop_front())
            .flatten()
    }
}

fn cancel_settlement(request: ExtensionDialogRequest) -> ExtensionDialogSettlement {
    let answer = match request {
        ExtensionDialogRequest::Confirm(_) => ExtensionDialogAnswer::Confirm(false),
        ExtensionDialogRequest::Select(_) => ExtensionDialogAnswer::Select(None),
        ExtensionDialogRequest::Input(_) => ExtensionDialogAnswer::Input(None),
        ExtensionDialogRequest::Workspace { .. } => ExtensionDialogAnswer::Workspace(false),
    };
    ExtensionDialogSettlement { request, answer }
}

fn normalize_title(method: &'static str, title: &str) -> Result<String, ExtensionDialogError> {
    if title.trim().is_empty() {
        return Err(ExtensionDialogError::EmptyTitle { method });
    }
    Ok(sanitize_terminal_line(title.trim()))
}

fn normalize_label(label: &str, fallback: &str) -> String {
    if label.trim().is_empty() {
        fallback.into()
    } else {
        sanitize_terminal_line(label.trim())
    }
}

fn normalize_body_lines(body: &str) -> Vec<String> {
    if body.is_empty() {
        return Vec::new();
    }
    body.split('\n')
        .take(MAX_CONFIRM_BODY_LINES)
        .map(sanitize_terminal_line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_hunk_dialog_oracle_records_both_pinned_baselines_and_all_tests() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-dialog-controller.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["stablePresence"], "absent");
        assert_eq!(oracle["executedOracle"]["passed"], 15);
        assert_eq!(oracle["executedOracle"]["failed"], 0);
        assert_eq!(oracle["invariants"].as_array().unwrap().len(), 12);
    }

    fn confirm(queue: &mut ExtensionDialogQueue, extension: &str, title: &str) {
        assert!(
            queue
                .enqueue_confirm(0, extension, title, title, "", "", None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn queues_one_visible_dialog_fifo_across_extensions_and_resets_promoted_state() {
        let mut queue = ExtensionDialogQueue::default();
        confirm(&mut queue, "alpha", "First?");
        queue
            .enqueue_select(
                1,
                "beta",
                "second",
                "Second?",
                vec!["one".into(), "two".into()],
            )
            .unwrap();
        queue
            .enqueue_input(0, "alpha", "third", "Third?", "", Some("feature/base"))
            .unwrap();
        assert_eq!(queue.current().unwrap().extension_id(), "alpha");
        let first_id = queue.current().unwrap().request_id();
        assert_eq!(
            queue.accept(first_id, None).unwrap().answer,
            ExtensionDialogAnswer::Confirm(true)
        );
        let ExtensionDialogRequest::Select(selected) = queue.current_mut().unwrap() else {
            panic!("select should be promoted");
        };
        assert_eq!(selected.selected, 0);
        queue.move_selection(-1);
        let second_id = queue.current().unwrap().request_id();
        assert_eq!(
            queue.accept(second_id, Some("two".into())).unwrap().answer,
            ExtensionDialogAnswer::Select(Some("two".into()))
        );
        let ExtensionDialogRequest::Input(input) = queue.current().unwrap() else {
            panic!("input should be promoted");
        };
        assert_eq!(input.value, "feature/base");
    }

    #[test]
    fn each_kind_has_its_exact_accept_and_cancel_value() {
        let mut queue = ExtensionDialogQueue::default();
        confirm(&mut queue, "probe", "Sure?");
        let id = queue.current().unwrap().request_id();
        assert_eq!(
            queue.cancel(id).unwrap().answer,
            ExtensionDialogAnswer::Confirm(false)
        );
        queue
            .enqueue_select(0, "probe", "which", "Which?", vec!["a".into()])
            .unwrap();
        let id = queue.current().unwrap().request_id();
        assert_eq!(
            queue.cancel(id).unwrap().answer,
            ExtensionDialogAnswer::Select(None)
        );
        queue
            .enqueue_input(0, "probe", "name", "Name?", "", None)
            .unwrap();
        queue.update_input("typed".into());
        let id = queue.current().unwrap().request_id();
        assert_eq!(
            queue.accept(id, Some("typed".into())).unwrap().answer,
            ExtensionDialogAnswer::Input(Some("typed".into()))
        );
        queue
            .enqueue_select(0, "probe", "empty", "Which?", vec!["a".into()])
            .unwrap();
        let id = queue.current().unwrap().request_id();
        assert_eq!(
            queue.accept(id, None).unwrap().answer,
            ExtensionDialogAnswer::Select(None)
        );
    }

    #[test]
    fn stale_answer_ids_cannot_settle_the_promoted_request() {
        let mut queue = ExtensionDialogQueue::default();
        confirm(&mut queue, "probe", "First?");
        confirm(&mut queue, "probe", "Second?");
        let first = queue.current().unwrap().request_id();
        queue.accept(first, None).unwrap();
        assert!(queue.accept(first, None).is_none());
        let ExtensionDialogRequest::Confirm(dialog) = queue.current().unwrap() else {
            panic!("second confirm should be visible");
        };
        assert_eq!(dialog.title, "Second?");
    }

    #[test]
    fn normalizes_defaults_body_attribution_and_hostile_terminal_text() {
        let mut queue = ExtensionDialogQueue::default();
        queue
            .enqueue_confirm(
                0,
                "carrier",
                "delete",
                "  \u{1b}[31mDelete?\u{1b}[0m  ",
                "one\ntwo\nthree\nfour\nfive\nsix\nseven",
                "",
                None,
            )
            .unwrap();
        let ExtensionDialogRequest::Confirm(dialog) = queue.current().unwrap() else {
            panic!("confirm should be visible");
        };
        assert_eq!(dialog.title, "Delete?");
        assert_eq!(dialog.body_lines.len(), 6);
        assert_eq!(dialog.confirm_label, "ok");
        assert_eq!(dialog.cancel_label, "cancel");
        assert!(dialog.show_attribution);
    }

    #[test]
    fn host_can_omit_attribution_only_for_native_ui() {
        let mut queue = ExtensionDialogQueue::default();
        queue
            .enqueue_confirm_with_attribution(
                0,
                "bundled-guide",
                "welcome",
                "Welcome",
                "",
                "",
                None,
                false,
            )
            .unwrap();
        let ExtensionDialogRequest::Confirm(dialog) = queue.current().unwrap() else {
            panic!("confirm should be visible");
        };
        assert!(!dialog.show_attribution);
    }

    #[test]
    fn select_options_are_sanitized_and_empty_labels_remain_real_choices() {
        let mut queue = ExtensionDialogQueue::default();
        queue
            .enqueue_select(
                0,
                "hostile",
                "pick",
                "\u{1b}[31mPick\u{1b}[0m",
                vec!["\u{1b}]0;pwned\u{7}opt".into(), String::new()],
            )
            .unwrap();
        let ExtensionDialogRequest::Select(dialog) = queue.current().unwrap() else {
            panic!("select should be visible");
        };
        assert_eq!(dialog.title, "Pick");
        assert_eq!(dialog.options, ["opt", ""]);
    }

    #[test]
    fn input_initial_is_sanitized_without_trimming() {
        let mut queue = ExtensionDialogQueue::default();
        queue
            .enqueue_input(
                0,
                "hostile",
                "branch",
                "Branch",
                "",
                Some(" \u{1b}]0;pwned\u{7}feature/x "),
            )
            .unwrap();
        let ExtensionDialogRequest::Input(dialog) = queue.current().unwrap() else {
            panic!("input should be visible");
        };
        assert_eq!(dialog.value, " feature/x ");
    }

    #[test]
    fn cancel_all_drains_but_keeps_the_queue_open() {
        let mut queue = ExtensionDialogQueue::default();
        confirm(&mut queue, "probe", "First?");
        queue
            .enqueue_input(0, "probe", "second", "Second?", "", None)
            .unwrap();
        let settlements = queue.cancel_all();
        assert_eq!(settlements.len(), 2);
        assert!(queue.current().is_none());
        confirm(&mut queue, "replacement", "After reload?");
        let ExtensionDialogRequest::Confirm(dialog) = queue.current().unwrap() else {
            panic!("replacement confirm should be visible");
        };
        assert_eq!(dialog.title, "After reload?");
    }

    #[test]
    fn shutdown_drains_and_immediately_cancels_later_requests() {
        let mut queue = ExtensionDialogQueue::default();
        confirm(&mut queue, "probe", "First?");
        assert_eq!(queue.shutdown().len(), 1);
        let refused = queue
            .enqueue_input(0, "probe", "late", "Too late?", "", None)
            .unwrap()
            .unwrap();
        assert_eq!(refused.answer, ExtensionDialogAnswer::Input(None));
        assert!(queue.current().is_none());
    }

    #[test]
    fn rejects_blank_titles_and_empty_selects_without_queueing() {
        let mut queue = ExtensionDialogQueue::default();
        assert_eq!(
            queue
                .enqueue_input(0, "probe", "blank", "   ", "", None)
                .unwrap_err(),
            ExtensionDialogError::EmptyTitle { method: "input" }
        );
        assert_eq!(
            queue
                .enqueue_select(0, "probe", "empty", "Which?", Vec::new())
                .unwrap_err(),
            ExtensionDialogError::EmptyOptions
        );
        assert!(queue.current().is_none());
    }

    #[test]
    fn selection_movement_wraps_at_both_ends_and_pick_is_bounded() {
        let mut queue = ExtensionDialogQueue::default();
        queue
            .enqueue_select(
                0,
                "probe",
                "which",
                "Which?",
                vec!["a".into(), "b".into(), "c".into()],
            )
            .unwrap();
        queue.move_selection(-1);
        let ExtensionDialogRequest::Select(dialog) = queue.current().unwrap() else {
            panic!("select should be visible");
        };
        assert_eq!(dialog.selected, 2);
        queue.move_selection(1);
        queue.pick_option(1);
        queue.pick_option(99);
        let ExtensionDialogRequest::Select(dialog) = queue.current().unwrap() else {
            panic!("select should be visible");
        };
        assert_eq!(dialog.selected, 1);
    }

    #[test]
    fn removing_one_extension_preserves_other_extensions_fifo_order() {
        let mut queue = ExtensionDialogQueue::default();
        confirm(&mut queue, "alpha", "Alpha");
        queue
            .enqueue_input(1, "beta", "beta", "Beta", "", None)
            .unwrap();
        confirm(&mut queue, "alpha", "Alpha again");
        assert_eq!(queue.remove_extension(0).len(), 2);
        assert_eq!(queue.current().unwrap().extension_id(), "beta");
    }
}
