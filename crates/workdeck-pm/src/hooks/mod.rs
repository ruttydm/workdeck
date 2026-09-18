//! Explicit, reviewed local Git-hook publication. No index or ref writes.
mod files;
mod store;
mod types;
use crate::{ErrorCode, PmError, RequestId, Result};
use std::path::Path;
pub use types::*;

pub fn hook_preview(worktree: &Path, mode: HookMode) -> Result<HookPlan> {
    store::preview(worktree, mode)
}
pub fn hook_status(worktree: &Path) -> Result<HookStatus> {
    store::status(worktree)
}
pub fn apply_hook(worktree: &Path, input: &HookApply, request: &RequestId) -> Result<HookReceipt> {
    apply_hook_with_faults(worktree, input, request, |_| Ok(()))
}
#[doc(hidden)]
pub fn apply_hook_with_faults(
    worktree: &Path,
    input: &HookApply,
    request: &RequestId,
    fault: impl FnMut(HookFaultPoint) -> Result<()>,
) -> Result<HookReceipt> {
    store::apply(worktree, input, request, fault)
}
pub fn recover_hook(worktree: &Path, request: &RequestId) -> Result<HookReceipt> {
    store::recover(worktree, request)
}
