//! Default issue collection composition. The index owns browsing; the native
//! controller owns one selected record, mutation intent and retained drafts.
use super::{WorkbenchShell, indexed_workspace::IndexedWorkspace};
use workdeck_pm::{SnapshotKind, SourceSelector, projection::*};

impl WorkbenchShell {
    pub(super) fn poll_index(&mut self, refresh: bool) {
        if !self.available {
            return;
        }
        if self.index.is_none() {
            match IndexedWorkspace::new(
                self.options.root.clone(),
                SourceSelector::WorkingTree,
                ProjectionQuery::Issues {
                    query: self.controller.filter().query(),
                    group_by: None,
                },
                ProjectionLimits::default(),
            ) {
                Ok(mut index) => {
                    index.refresh(false);
                    self.index = Some(index);
                }
                Err(error) => {
                    self.notice = Some(format!("Cannot start planning reader: {error}"));
                    return;
                }
            }
        }
        let index = self.index.as_mut().expect("opened index");
        index.poll();
        let query = ProjectionQuery::Issues {
            query: self.controller.filter().query(),
            group_by: index.board.then_some(index.board_group),
        };
        if index.query != query {
            index.set_query(query);
        }
        if let Some(receipt) = self.controller.last_receipt()
            && self.index_receipt.as_ref() != Some(&receipt.operation_id)
        {
            self.index_receipt = Some(receipt.operation_id.clone());
            self.index_attempted = None;
            self.index_pending_selection =
                self.controller.selected_id().map(|id| ProjectionRecordKey {
                    repository: self
                        .controller
                        .repository()
                        .expect("bound controller")
                        .identity()
                        .clone(),
                    kind: SnapshotKind::Issue,
                    id: id.to_string(),
                });
            index.refresh(false);
        } else if refresh && !index.refreshing {
            self.index_attempted = None;
            index.refresh(false);
        }
        if !index.refreshing
            && !index.querying
            && index.handle.is_some()
            && let Some(key) = self.index_pending_selection.take()
        {
            index.locate(key);
        }
        self.sync_index_selection();
        self.replay_index_input();
    }

    pub(super) fn sync_index_selection(&mut self) {
        let Some(index) = &self.index else {
            return;
        };
        if !index.is_idle() || index.refreshing || index.querying || index.loading_page {
            return;
        }
        let token = index.selected_row().map(|row| row.token.clone());
        let Some(token) = token else {
            if index
                .handle
                .as_ref()
                .is_some_and(|handle| handle.total == 0)
            {
                self.controller.clear_index_selection();
                self.index_attempted = None;
            }
            return;
        };
        if self.index_attempted.as_ref() == Some(&token) {
            return;
        }
        self.index_attempted = Some(token.clone());
        if let Err(error) = self.controller.select_projection(&token) {
            self.index_selection_failed(error);
        }
    }

    fn index_selection_failed(&mut self, error: workdeck_pm::PmError) {
        self.notice = Some(error.message.clone());
        if let Some(index) = &mut self.index {
            index.error = Some(error);
            if let Some(status) = &mut index.status {
                status.state = ProjectionState::Stale;
            }
        }
    }

    pub(super) fn prepare_index_selection(&mut self) -> bool {
        let token = self
            .index
            .as_ref()
            .and_then(|index| index.selected_row())
            .map(|row| row.token.clone());
        let Some(token) = token else {
            self.notice =
                Some("Select an available indexed issue before opening this action".into());
            return false;
        };
        match self.controller.select_projection(&token) {
            Ok(()) => true,
            Err(error) => {
                self.index_selection_failed(error);
                false
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingInput {
    issue: Option<workdeck_pm::IssueId>,
    entries: Vec<QueuedInput>,
    bytes: usize,
    canceled: bool,
}
#[derive(Debug)]
enum QueuedInput {
    Key(crossterm::event::KeyEvent),
    Paste(String),
}

impl WorkbenchShell {
    fn queue_index_input(&mut self, entry: QueuedInput, bytes: usize) {
        let Some(pending) = &mut self.index_input else {
            return;
        };
        if pending.canceled {
            return;
        }
        if pending.entries.len() >= 64 || pending.bytes.saturating_add(bytes) > 64 * 1024 {
            pending.entries.clear();
            pending.bytes = 0;
            pending.canceled = true;
            self.notice = Some(
                "Pending planning input exceeded its bound; press Esc before retrying the action"
                    .into(),
            );
            return;
        }
        pending.bytes += bytes;
        pending.entries.push(entry);
    }
    pub(super) fn queue_index_paste(&mut self, text: &str) -> bool {
        if self.index_input.is_none() {
            return false;
        }
        if text.len() > 64 * 1024 {
            let pending = self.index_input.as_mut().expect("pending paste");
            pending.entries.clear();
            pending.bytes = 0;
            pending.canceled = true;
            self.notice = Some("Pending paste exceeds 64 KiB; press Esc before retrying".into());
        } else {
            self.queue_index_input(QueuedInput::Paste(text.into()), text.len());
        }
        true
    }
    pub(super) fn defer_index_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        self.poll_index(false);
        if self.index_input.is_some() {
            self.queue_index_input(QueuedInput::Key(key), 32);
            return true;
        }
        if self.tab != super::WorkbenchTab::Issues
            || self.context_visible
            || self.graph.is_some()
            || self.form.is_some()
            || !key.modifiers.is_empty()
            || !matches!(
                key.code,
                KeyCode::Char(
                    'i' | 'e' | 'c' | 's' | 'a' | 'p' | 'l' | 'f' | 'g' | 'b' | 'd' | 'o' | 'y'
                )
            )
        {
            return false;
        }
        let Some(index) = &self.index else {
            return false;
        };
        if index.is_idle()
            || (index.selected_row().is_some()
                && self.controller.selected_issue().is_some()
                && self.index_pending_selection.is_none())
        {
            return false;
        }
        let issue = self
            .controller
            .selected_id()
            .cloned()
            .or_else(|| {
                index
                    .selected_row()
                    .and_then(|row| row.token.key.id.parse().ok())
            })
            .or_else(|| {
                self.index_rendered
                    .as_ref()
                    .and_then(|rendered| rendered.selected.as_ref())
                    .and_then(|token| token.key.id.parse().ok())
            });
        // Reaching this point means the index is still busy, so a mutation key
        // with no issue identity yet is a first load racing the user: deferring
        // it does not choose anything, because the replay below still requires
        // the regular selection sync before a form can open. An idle index
        // never reaches this point and keeps refusing to guess.
        self.index_input = Some(PendingInput {
            issue,
            entries: Vec::new(),
            bytes: 0,
            canceled: false,
        });
        self.queue_index_input(QueuedInput::Key(key), 32);
        self.notice = Some("Waiting for the selected planning source before handling input".into());
        true
    }
    fn replay_index_input(&mut self) {
        if self.input_paused
            || self.replaying_index_input
            || self
                .index_input
                .as_ref()
                .is_none_or(|pending| pending.canceled)
        {
            return;
        }
        let Some(index) = &self.index else {
            return;
        };
        if !index.is_idle() || index.refreshing || index.querying || index.loading_page {
            return;
        }
        if index.error.is_some() || index.worker_error.is_some() {
            let pending = self.index_input.as_mut().expect("pending input");
            pending.entries.clear();
            pending.bytes = 0;
            pending.canceled = true;
            self.notice = Some("Pending planning input canceled because the source could not be read; press Esc and inspect the source error".into());
            return;
        }
        let mut pending = self.index_input.take().expect("pending input");
        if pending.issue.as_ref().is_some_and(|id| {
            index
                .selected_row()
                .is_none_or(|row| row.token.key.id != id.as_str())
        }) {
            pending.entries.clear();
            pending.bytes = 0;
            pending.canceled = true;
            self.index_input = Some(pending);
            self.notice = Some("Pending planning input canceled because its selected issue changed; press Esc before retrying".into());
            return;
        }
        self.notice = None;
        self.replaying_index_input = true;
        for entry in pending.entries {
            match entry {
                QueuedInput::Key(key) => {
                    self.key(key);
                }
                QueuedInput::Paste(text) => {
                    self.paste(&text);
                }
            }
        }
        self.replaying_index_input = false;
    }
}

#[cfg(test)]
mod input_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    #[test]
    fn queued_field_text_keeps_q_and_paste_in_order_until_explicit_escape() {
        let directory = tempfile::tempdir().unwrap();
        workdeck_pm::Repository::init(directory.path(), "WD").unwrap();
        let mut shell =
            WorkbenchShell::open(super::super::WorkbenchOptions::new(directory.path()), true);
        // A canceled queue also retains ownership of subsequent field input:
        // no character may escape it and become an unrelated shell command.
        shell.index_input = Some(PendingInput {
            issue: None,
            entries: Vec::new(),
            bytes: 0,
            canceled: true,
        });
        shell.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(shell.index_input.as_ref().unwrap().canceled);
        assert!(shell.queue_index_paste("queued label"));
        assert!(shell.index_input.as_ref().unwrap().entries.is_empty());
        shell.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(shell.index_input.is_none());
    }

    #[test]
    fn input_overflow_requires_explicit_cancellation_before_new_actions() {
        let directory = tempfile::tempdir().unwrap();
        workdeck_pm::Repository::init(directory.path(), "WD").unwrap();
        let mut shell =
            WorkbenchShell::open(super::super::WorkbenchOptions::new(directory.path()), true);
        shell.index_input = Some(PendingInput {
            issue: None,
            entries: Vec::new(),
            bytes: 0,
            canceled: false,
        });
        for _ in 0..65 {
            shell.queue_index_input(
                QueuedInput::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
                32,
            );
        }
        assert!(shell.index_input.as_ref().unwrap().canceled);
        assert!(shell.index_input.as_ref().unwrap().entries.is_empty());
        shell.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert!(
            shell.form.is_none(),
            "overflow must not reinterpret buffered field text as new actions"
        );
        shell.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(shell.index_input.is_none());
        shell.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert!(shell.form.is_some());
    }
}
