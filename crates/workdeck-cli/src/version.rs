//! Workdeck CLI version parsing and update comparison.

pub const UNKNOWN_CLI_VERSION: &str = "0.0.0-unknown";

pub fn resolve_cli_version() -> &'static str {
    let version = env!("CARGO_PKG_VERSION");
    if version.is_empty() {
        UNKNOWN_CLI_VERSION
    } else {
        version
    }
}

pub fn is_stable_version(version: &str) -> bool {
    numeric_core(version).is_some()
}

pub fn is_prerelease_version(version: &str) -> bool {
    let Some((core, prerelease)) = version.split_once('-') else {
        return false;
    };
    numeric_core(core).is_some()
        && !prerelease.is_empty()
        && prerelease
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}

pub fn is_comparable_version(version: &str) -> bool {
    version != UNKNOWN_CLI_VERSION && (is_stable_version(version) || is_prerelease_version(version))
}

pub fn is_newer_version(current: &str, candidate: &str) -> bool {
    semver::Version::parse(current)
        .and_then(|current| semver::Version::parse(candidate).map(|candidate| current < candidate))
        .unwrap_or(false)
}

fn numeric_core(version: &str) -> Option<[&str; 3]> {
    let mut parts = version.split('.');
    let result = [parts.next()?, parts.next()?, parts.next()?];
    (parts.next().is_none()
        && result
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())))
    .then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_stable_prerelease_unknown_and_invalid_versions() {
        assert!(is_stable_version("1.2.3"));
        assert!(!is_stable_version("1.2.3-beta.1"));
        assert!(is_prerelease_version("1.2.3-beta.1"));
        assert!(is_prerelease_version("1.2.3-rc-1"));
        assert!(!is_prerelease_version("1.2.3-"));
        assert!(!is_comparable_version(UNKNOWN_CLI_VERSION));
        assert!(!is_comparable_version("latest"));
        assert!(!resolve_cli_version().is_empty());
    }

    #[test]
    fn compares_versions_without_panicking_on_invalid_input() {
        assert!(is_newer_version("1.2.3", "1.2.4"));
        assert!(is_newer_version("1.2.3-beta.1", "1.2.3"));
        assert!(!is_newer_version("1.2.4", "1.2.3"));
        assert!(!is_newer_version("invalid", "1.2.3"));
    }
}
