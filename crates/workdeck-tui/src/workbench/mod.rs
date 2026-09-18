//! Planning controller and bounded views for the persistent review shell.
//!
//! Planning I/O uses `workdeck-pm`; repository panel reads use a provider bound
//! by the composition root. This module does not initialize repositories, parse
//! authoritative documents, run processes, or own a terminal session. The host
//! retains its native review lifecycle and return context during navigation.
//! Explicit check execution delegates to the shared foreground runner and is
//! joined before that existing terminal lifecycle ends.

mod activity;
mod board_view;
pub(crate) mod checkouts;
mod checks_view;
mod checks_workspace;
mod claims_view;
mod claims_workspace;
mod context_view;
mod context_workspace;
mod controller;
mod feature_workspace;
mod graph_view;
mod host;
mod indexed_shell;
mod indexed_view;
mod indexed_workspace;
mod input;
mod my_work;
mod my_work_view;
mod owned_publication;
mod owned_run;
mod panel_controller;
mod panel_shell;
mod panels;
mod planning_workspace;
mod projection_reader;
mod projection_worker;
mod readonly;
mod review_authority;
mod shell;
mod shell_view;
mod source_actions;
mod source_actions_view;
mod sources_view;
mod sources_workspace;
mod view;
mod virtual_viewport;

pub use controller::*;
pub use owned_run::ForegroundRunSignal;
pub use panels::*;
pub(crate) use shell::WorkbenchShell;
pub use shell::{WorkbenchOptions, WorkbenchTab};
pub use view::*;

#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod panel_tests;

#[cfg(test)]
mod planning_navigation_tests;

#[cfg(test)]
mod checks_tests;
#[cfg(test)]
mod claims_tests;
#[cfg(test)]
mod context_review_tests;
#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod source_tests;

#[cfg(test)]
mod source_action_tests;

#[cfg(test)]
mod projection_worker_tests;

#[cfg(test)]
mod indexed_workspace_tests;

#[cfg(test)]
mod indexed_core_tests;

#[cfg(test)]
mod test_pump;

#[cfg(test)]
mod readonly_tests;
