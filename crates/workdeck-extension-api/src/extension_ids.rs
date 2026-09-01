//! Parseable extension namespaces retained across the native host boundary.

/// Vendor namespace reserved for Workdeck's built-in commands, panes, and UI.
pub const WORKDECK_VENDOR_EXTENSION_ID: &str = "workdeck";
/// Stable key of the bundled files pane after Workdeck branding normalization.
pub const WORKDECK_FILES_PANE_KEY: &str = "workdeck:files";
/// Human-readable legacy extension-stem constraint.
pub const EXTENSION_ID_RULE: &str =
    "ids must start with a letter or digit and use only letters, digits, \"-\", and \"_\"";

/// Validate a file-stem extension ID before it becomes a namespace.
///
/// Native `workdeck-extension.toml` IDs may additionally use dots for backward
/// compatibility with Workdeck's existing SDK. Legacy Hunk discovery calls
/// this exact stem validator so command (`.`) and pane (`:`) qualification
/// remain unambiguous.
#[must_use]
pub fn is_valid_extension_stem(id: &str) -> bool {
    let mut bytes = id.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_stems_keep_command_and_pane_qualifiers_parseable() {
        for valid in ["a", "Hunk", "git-lite", "review_notes", "9patch"] {
            assert!(is_valid_extension_stem(valid), "{valid}");
        }
        for invalid in [
            "",
            "-leading",
            "_leading",
            "has.dot",
            "has:colon",
            "has space",
            "naive-🚀",
        ] {
            assert!(!is_valid_extension_stem(invalid), "{invalid}");
        }
    }

    #[test]
    fn built_in_namespace_and_files_key_use_workdeck_branding() {
        assert_eq!(WORKDECK_VENDOR_EXTENSION_ID, "workdeck");
        assert_eq!(WORKDECK_FILES_PANE_KEY, "workdeck:files");
        assert!(WORKDECK_FILES_PANE_KEY.starts_with(WORKDECK_VENDOR_EXTENSION_ID));
    }
}
