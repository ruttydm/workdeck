//! Built-in CLI command names reserved from native extensions.

pub const BUILT_IN_CLI_COMMAND_NAMES: &[&str] = &[
    "diff",
    "show",
    "patch",
    "pager",
    "difftool",
    "stash",
    "session",
    "markup",
    "skill",
    "extension",
    "ext",
    "update",
    "daemon",
    "mcp",
    "help",
    "version",
    "migrate",
    "status",
    "files",
    "changes",
    "search",
    "config",
    "events",
    "import",
    "doctor",
    "export",
    "issue",
    "agent",
    "project",
    "cycle",
    "label",
];

pub fn is_built_in_cli_command_name(name: &str) -> bool {
    BUILT_IN_CLI_COMMAND_NAMES.contains(&name)
}

pub fn is_valid_extension_cli_command_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub fn is_reserved_extension_cli_command_name(name: &str) -> bool {
    is_built_in_cli_command_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_builtins_extension_grammar_and_product_reservations() {
        for name in ["diff", "session", "ext", "help", "issue"] {
            assert!(is_built_in_cli_command_name(name));
            assert!(is_reserved_extension_cli_command_name(name));
        }
        for valid in ["lint", "review-export", "x1"] {
            assert!(is_valid_extension_cli_command_name(valid));
        }
        for invalid in ["", "1lint", "Lint", "lint_me", "lint/me"] {
            assert!(!is_valid_extension_cli_command_name(invalid));
        }
    }
}
