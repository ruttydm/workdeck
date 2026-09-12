//! Test-only event-loop pumping for the asynchronous collection reader. It does
//! not refresh authority or change the captured source of open forms/contexts.
use super::WorkbenchShell;
use std::time::{Duration, Instant};

pub(super) fn settle_shell(shell: &mut WorkbenchShell) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        shell.poll_index(false);
        if let Some(source) = &mut shell.readonly {
            source.poll();
        }
        if let Some(view) = &mut shell.my_work {
            view.poll();
        }
        if let Some(activity) = &mut shell.activity {
            activity.poll();
        }
        shell.features.poll_index();
        shell.planning.poll_index();
        if shell
            .readonly
            .as_ref()
            .is_none_or(|source| source.is_idle())
            && shell.index.as_ref().is_none_or(|index| index.is_idle())
            && shell
                .activity
                .as_ref()
                .is_none_or(|activity| activity.is_idle())
            && shell.my_work.as_ref().is_none_or(|view| view.is_idle())
            && shell.features.reader_idle()
            && shell.planning.reader_idle()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "indexed reader did not settle: {:?}",
            shell.notice
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
pub(super) fn settle_app(app: &mut crate::ReviewApp) {
    if let Some(shell) = &app.workbench {
        settle_shell(&mut shell.lock().unwrap_or_else(|error| error.into_inner()));
    }
}
