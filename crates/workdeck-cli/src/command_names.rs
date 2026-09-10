//! Built-in CLI command names reserved from native extensions.

pub use workdeck_extension_api::BUILT_IN_CLI_COMMAND_NAMES;

pub fn is_built_in_cli_command_name(name: &str) -> bool {
    BUILT_IN_CLI_COMMAND_NAMES.contains(&name)
}

pub fn is_valid_extension_cli_command_name(name: &str) -> bool {
    workdeck_extension_api::is_valid_extension_cli_command_name(name)
}

pub fn is_reserved_extension_cli_command_name(name: &str) -> bool {
    workdeck_extension_api::is_reserved_extension_cli_command_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_builtins_extension_grammar_and_product_reservations() {
        for name in ["diff", "session", "ext", "help", "issue", "install"] {
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
