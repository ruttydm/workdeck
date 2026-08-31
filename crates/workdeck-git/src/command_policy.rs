//! One review-only policy for every shell-backed Git command in Workdeck.

use anyhow::{Result, bail};

const MUTATING_COMMANDS: &[&str] = &[
    "add",
    "am",
    "apply",
    "bisect",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "fetch",
    "gc",
    "init",
    "merge",
    "mv",
    "notes",
    "pull",
    "push",
    "rebase",
    "reflog",
    "remote-update",
    "replace",
    "reset",
    "restore",
    "revert",
    "rm",
    "submodule",
    "switch",
    "update-index",
];

const READ_ONLY_COMMANDS: &[&str] = &[
    "blame",
    "branch",
    "cat-file",
    "describe",
    "diff",
    "diff-tree",
    "for-each-ref",
    "grep",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "merge-base",
    "name-rev",
    "remote",
    "rev-list",
    "rev-parse",
    "shortlog",
    "show",
    "show-ref",
    "status",
    "stash",
    "symbolic-ref",
    "tag",
    "worktree",
];

/// Rejects every Git invocation that is not explicitly known to be read-only.
/// Process-local `-c key=value` options are accepted; repository config writes
/// and mutating porcelain/plumbing commands are never accepted.
pub fn validate_read_only_git_args(arguments: &[&str]) -> Result<()> {
    let command_index = command_index(arguments)?;
    let command = arguments[command_index];
    if MUTATING_COMMANDS.contains(&command) || !READ_ONLY_COMMANDS.contains(&command) {
        bail!("Workdeck read-only policy rejected git {command}");
    }
    let tail = &arguments[command_index + 1..];
    match command {
        "worktree" if tail.first().copied() != Some("list") => {
            bail!("Workdeck only permits git worktree list")
        }
        "remote" if !remote_is_read_only(tail) => {
            bail!("Workdeck rejected a mutating git remote operation")
        }
        "branch" if !listing_operation(tail) => {
            bail!("Workdeck only permits listing branches")
        }
        "tag" if !listing_operation(tail) => {
            bail!("Workdeck only permits listing tags")
        }
        "stash" if !stash_is_read_only(tail) => {
            bail!("Workdeck only permits listing or showing stashes")
        }
        "symbolic-ref" if tail.iter().filter(|value| !value.starts_with('-')).count() > 1 => {
            bail!("Workdeck only permits reading symbolic refs")
        }
        _ => {}
    }
    Ok(())
}

fn command_index(arguments: &[&str]) -> Result<usize> {
    let mut index = 0;
    while let Some(argument) = arguments.get(index) {
        match *argument {
            "-c" | "--config-env" => index += 2,
            "--no-pager" | "--paginate" | "--literal-pathspecs" | "--no-optional-locks" => {
                index += 1
            }
            value if value.starts_with('-') => {
                bail!("Workdeck read-only policy rejected Git global option {value}")
            }
            _ => return Ok(index),
        }
    }
    bail!("Workdeck read-only policy requires a Git command")
}

fn remote_is_read_only(arguments: &[&str]) -> bool {
    arguments.is_empty()
        || matches!(
            arguments.first().copied(),
            Some("-v" | "--verbose" | "get-url" | "show")
        )
}

fn listing_operation(arguments: &[&str]) -> bool {
    arguments.is_empty()
        || arguments.iter().all(|argument| {
            argument.starts_with('-')
                || argument.starts_with("refs/")
                || argument == &"HEAD"
                || argument.contains('*')
        })
}

fn stash_is_read_only(arguments: &[&str]) -> bool {
    matches!(arguments.first().copied(), Some("list" | "show"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_review_commands_and_process_local_configuration() {
        for arguments in [
            &["status"][..],
            &["diff", "--cached", "--", "src/lib.rs"],
            &["-c", "color.ui=never", "log", "--oneline"],
            &["worktree", "list", "--porcelain"],
            &["remote", "-v"],
            &["branch", "--show-current"],
            &["tag", "--list", "v*"],
            &["stash", "list", "--format=%gd%x09%s"],
            &[
                "stash",
                "show",
                "--stat",
                "--patch",
                "--color=never",
                "stash@{0}",
            ],
        ] {
            validate_read_only_git_args(arguments).unwrap();
        }
    }

    #[test]
    fn rejects_mutation_including_ambiguous_branch_remote_and_worktree_forms() {
        for arguments in [
            &["add", "."][..],
            &["commit", "-m", "no"],
            &["fetch", "origin"],
            &["push", "origin", "main"],
            &["branch", "new-branch"],
            &["branch", "-D", "main"],
            &["tag", "v1"],
            &["remote", "set-url", "origin", "example.invalid"],
            &["worktree", "prune"],
            &["stash"],
            &["stash", "push", "-m", "mutation"],
            &["stash", "pop"],
            &["stash", "drop", "stash@{0}"],
            &["stash", "clear"],
            &["config", "user.name", "mutation"],
        ] {
            assert!(
                validate_read_only_git_args(arguments).is_err(),
                "unexpectedly allowed {arguments:?}"
            );
        }
    }
}
