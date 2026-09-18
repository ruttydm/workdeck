//! Immutable ownership table for extension-provided top-level CLI commands.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use workdeck_diff::{SanitizeOptions, sanitize_terminal_text};
use workdeck_extension_api::CliCommandRegistration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredExtensionCliCommand {
    pub extension_index: usize,
    pub extension_id: String,
    pub source_path: PathBuf,
    pub origin: String,
    pub command: CliCommandRegistration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCliCommandCollision {
    pub name: String,
    pub winner_extension_id: String,
    pub rejected_extension_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedExtensionCliCommands {
    pub commands: BTreeMap<String, RegisteredExtensionCliCommand>,
    pub collisions: Vec<ExtensionCliCommandCollision>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCliLoadIssue {
    pub extension_id: String,
    pub path: PathBuf,
    pub origin: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionCliInterruptAction {
    Cancel,
    Exit,
}

/// Revocable ownership state behind the process-wide signal callback.
///
/// The `ctrlc` backend cannot unregister a callback, so retiring the lease restores the default
/// observable exit behavior while guaranteeing that delegated Workdeck work can no longer mutate
/// the extension command's cancellation flag.
#[derive(Debug)]
pub struct ExtensionCliSignalLease {
    active: AtomicBool,
    interrupts: AtomicU8,
    cancelled: AtomicBool,
}

impl ExtensionCliSignalLease {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active: AtomicBool::new(true),
            interrupts: AtomicU8::new(0),
            cancelled: AtomicBool::new(false),
        }
    }

    #[must_use]
    pub fn interrupt(&self) -> ExtensionCliInterruptAction {
        if !self.active.load(Ordering::Acquire) {
            return ExtensionCliInterruptAction::Exit;
        }
        if self.interrupts.fetch_add(1, Ordering::AcqRel) == 0 {
            self.cancelled.store(true, Ordering::Release);
            ExtensionCliInterruptAction::Cancel
        } else {
            ExtensionCliInterruptAction::Exit
        }
    }

    pub fn retire(&self) {
        self.active.store(false, Ordering::Release);
    }

    #[must_use]
    pub const fn cancellation_flag(&self) -> &AtomicBool {
        &self.cancelled
    }
}

impl Default for ExtensionCliSignalLease {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve exact top-level command ownership in registry order.
#[must_use]
pub fn resolve_extension_cli_commands(
    registered: &[RegisteredExtensionCliCommand],
) -> ResolvedExtensionCliCommands {
    let mut resolved = ResolvedExtensionCliCommands::default();
    for command in registered {
        if let Some(winner) = resolved.commands.get(&command.command.name) {
            resolved.collisions.push(ExtensionCliCommandCollision {
                name: command.command.name.clone(),
                winner_extension_id: winner.extension_id.clone(),
                rejected_extension_id: command.extension_id.clone(),
            });
        } else {
            resolved
                .commands
                .insert(command.command.name.clone(), command.clone());
        }
    }
    resolved
}

/// Find the extension handler that owns one exact, case-sensitive token.
#[must_use]
pub fn find_extension_cli_command<'a>(
    name: &str,
    resolved: &'a ResolvedExtensionCliCommands,
) -> Option<&'a RegisteredExtensionCliCommand> {
    resolved.commands.get(name)
}

pub(crate) fn single_line(value: &str) -> String {
    sanitize_terminal_text(
        value,
        SanitizeOptions {
            preserve_newlines: false,
            preserve_tabs: false,
            preserve_ansi_style: false,
        },
    )
    .trim()
    .to_owned()
}

/// Render each winning command as one terminal-safe Workdeck usage line.
#[must_use]
pub fn describe_extension_cli_commands(resolved: &ResolvedExtensionCliCommands) -> Vec<String> {
    resolved
        .commands
        .values()
        .map(|registered| &registered.command)
        .map(|command| {
            let usage = command
                .usage
                .as_deref()
                .map(single_line)
                .filter(|usage| !usage.is_empty())
                .map(|usage| format!(" {usage}"))
                .unwrap_or_default();
            format!(
                "workdeck {}{} — {}",
                command.name,
                usage,
                single_line(&command.summary)
            )
        })
        .collect()
}

/// Convert duplicate claims into source-attributed, sanitized load issues.
#[must_use]
pub fn create_extension_cli_collision_issues(
    registered: &[RegisteredExtensionCliCommand],
    collisions: &[ExtensionCliCommandCollision],
) -> Vec<ExtensionCliLoadIssue> {
    collisions
        .iter()
        .filter_map(|collision| {
            let rejected = registered
                .iter()
                .find(|entry| entry.extension_id == collision.rejected_extension_id)?;
            Some(ExtensionCliLoadIssue {
                extension_id: rejected.extension_id.clone(),
                path: rejected.source_path.clone(),
                origin: rejected.origin.clone(),
                message: format!(
                    "CLI command \"{}\" is already registered by {}; {} cannot replace it.",
                    single_line(&collision.name),
                    single_line(&collision.winner_extension_id),
                    single_line(&collision.rejected_extension_id)
                ),
            })
        })
        .collect()
}

/// Copy public command metadata before it is stored by the host.
#[must_use]
pub fn copy_extension_cli_command(command: &CliCommandRegistration) -> CliCommandRegistration {
    CliCommandRegistration {
        name: command.name.clone(),
        summary: command.summary.clone(),
        usage: command.usage.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(
        extension_index: usize,
        extension_id: &str,
        name: &str,
        summary: Option<&str>,
        usage: Option<&str>,
    ) -> RegisteredExtensionCliCommand {
        RegisteredExtensionCliCommand {
            extension_index,
            extension_id: extension_id.into(),
            source_path: PathBuf::from(format!("/{extension_id}.rs")),
            origin: "config".into(),
            command: CliCommandRegistration {
                name: name.into(),
                summary: summary.unwrap_or(&format!("{extension_id} command")).into(),
                usage: usage.map(str::to_owned),
            },
        }
    }

    #[test]
    fn keeps_the_first_command_claim_in_registry_order() {
        let registered = vec![
            command(0, "first", "pr", None, None),
            command(1, "second", "pr", None, None),
        ];
        let resolved = resolve_extension_cli_commands(&registered);

        assert_eq!(
            find_extension_cli_command("pr", &resolved).map(|entry| entry.extension_id.as_str()),
            Some("first")
        );
        assert!(find_extension_cli_command("PR", &resolved).is_none());
        assert_eq!(
            resolved.collisions,
            [ExtensionCliCommandCollision {
                name: "pr".into(),
                winner_extension_id: "first".into(),
                rejected_extension_id: "second".into(),
            }]
        );
        let issues = create_extension_cli_collision_issues(&registered, &resolved.collisions);
        assert!(
            issues[0]
                .message
                .contains("CLI command \"pr\" is already registered by first")
        );
        assert_eq!(issues[0].path, PathBuf::from("/second.rs"));
        assert_eq!(issues[0].origin, "config");
    }

    #[test]
    fn describes_each_winning_command_by_name_usage_and_summary() {
        let registered = vec![
            command(
                0,
                "review",
                "pr",
                Some("Review a pull request"),
                Some("<number>"),
            ),
            command(1, "loser", "pr", Some("Never listed"), None),
            command(2, "tools", "cli-tools", Some("Demonstrate workflows"), None),
        ];

        assert_eq!(
            describe_extension_cli_commands(&resolve_extension_cli_commands(&registered)),
            [
                "workdeck cli-tools — Demonstrate workflows",
                "workdeck pr <number> — Review a pull request",
            ]
        );
    }

    #[test]
    fn collapses_extension_supplied_metadata_into_one_safe_terminal_line() {
        let registered = vec![command(
            0,
            "hostile",
            "spoof",
            Some("Real\n\x1b[2KUnknown command: diff"),
            Some("<a>\tb"),
        )];

        assert_eq!(
            describe_extension_cli_commands(&resolve_extension_cli_commands(&registered)),
            ["workdeck spoof <a>b — RealUnknown command: diff"]
        );
    }

    #[test]
    fn copies_optional_public_metadata_without_aliasing_the_input() {
        let original = CliCommandRegistration {
            name: "demo".into(),
            summary: "Demo".into(),
            usage: Some("<path>".into()),
        };
        let copied = copy_extension_cli_command(&original);
        assert_eq!(copied, original);
        assert_ne!(copied.name.as_ptr(), original.name.as_ptr());
    }

    #[test]
    fn signal_lease_cancels_once_then_restores_default_exit_behavior() {
        let lease = ExtensionCliSignalLease::new();
        assert!(!lease.cancellation_flag().load(Ordering::Acquire));
        assert_eq!(lease.interrupt(), ExtensionCliInterruptAction::Cancel);
        assert!(lease.cancellation_flag().load(Ordering::Acquire));
        assert_eq!(lease.interrupt(), ExtensionCliInterruptAction::Exit);

        let retired = ExtensionCliSignalLease::new();
        retired.retire();
        assert_eq!(retired.interrupt(), ExtensionCliInterruptAction::Exit);
        assert!(!retired.cancellation_flag().load(Ordering::Acquire));
    }

    #[test]
    fn frozen_hunk_oracle_records_the_pinned_baseline_and_stable_absence() {
        let oracle = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../port/hunk/oracles/extension-cli-commands.json"
        ));
        assert!(oracle.contains("\"passed\": 3"));
        assert!(oracle.contains("945c21148a14e06b986c279d98659b4cce4babcd"));
        assert!(oracle.contains("\"status\": \"absent\""));
    }
}
