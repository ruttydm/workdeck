//! Explicit local hook publication; previews never install or recover a hook.
use super::pm_cli;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::json;
use std::path::Path;
use workdeck_pm::{HookApply, HookMode, RequestId, Result};

#[derive(Debug, Default, Args)]
pub(super) struct HookOptions {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(
        long,
        global = true,
        help = "Never prompt; mutations always require a reviewed plan and request ID"
    )]
    pub no_input: bool,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum HookName {
    PreCommit,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(super) enum Mode {
    Install,
    Update,
    Remove,
}
impl From<Mode> for HookMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Install => Self::Install,
            Mode::Update => Self::Update,
            Mode::Remove => Self::Remove,
        }
    }
}
#[derive(Debug, Args)]
pub(super) struct ApplyOptions {
    #[arg(value_enum, default_value = "pre-commit")]
    pub hook: HookName,
    #[arg(
        long,
        help = "Fingerprint from hooks preview; changed target bytes, modes, and configuration are rejected"
    )]
    pub expected_plan: String,
    #[arg(long, help = "Exact request identity retained for retry and recovery")]
    pub request_id: String,
}
#[derive(Debug, Subcommand)]
pub(super) enum HookCommand {
    Preview {
        #[arg(value_enum, default_value = "pre-commit")]
        hook: HookName,
        #[arg(long, value_enum, default_value = "install")]
        mode: Mode,
    },
    Install(ApplyOptions),
    Update(ApplyOptions),
    Remove(ApplyOptions),
    Status {
        #[arg(value_enum, default_value = "pre-commit")]
        hook: HookName,
    },
    Recover {
        #[arg(long)]
        request_id: String,
    },
}
pub(super) fn run(cwd: &Path, options: &HookOptions, command: &HookCommand) -> Result<()> {
    let (kind, result, repository, target) = match command {
        HookCommand::Preview { mode, .. } => {
            let plan = workdeck_pm::hook_preview(cwd, (*mode).into())?;
            ("hooks.preview", json!(plan), plan.repository, plan.target)
        }
        HookCommand::Status { .. } => {
            let status = workdeck_pm::hook_status(cwd)?;
            (
                "hooks.status",
                json!(status),
                status.repository,
                status.target,
            )
        }
        HookCommand::Recover { request_id } => {
            let receipt = workdeck_pm::recover_hook(cwd, &request_id.parse::<RequestId>()?)?;
            (
                "hooks.recover",
                json!(receipt),
                receipt.repository,
                receipt.plan.target,
            )
        }
        HookCommand::Install(input) | HookCommand::Update(input) | HookCommand::Remove(input) => {
            let mode = match command {
                HookCommand::Install(_) => HookMode::Install,
                HookCommand::Update(_) => HookMode::Update,
                HookCommand::Remove(_) => HookMode::Remove,
                _ => unreachable!(),
            };
            let request = input.request_id.parse::<RequestId>()?;
            let input = HookApply {
                mode,
                expected_plan: input.expected_plan.parse()?,
            };
            let receipt = workdeck_pm::apply_hook(cwd, &input, &request)?;
            (
                "hooks.apply",
                json!(receipt),
                receipt.repository,
                receipt.plan.target,
            )
        }
    };
    let source =
        json!({"repository":repository,"root":cwd,"target":target,"scope":"local_git_hook"});
    pm_cli::emit(options.json, kind, &source, &result, None)
}
