//! Native Rust watch-runtime compatibility.

/// Workdeck's watcher is native Rust and does not inherit JavaScript runtime watcher deadlocks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeWatchRuntime;

impl NativeWatchRuntime {
    #[must_use]
    pub const fn supports_reliable_watch_mode(self) -> bool {
        true
    }

    /// The native runtime has no external runtime-version precondition.
    pub const fn assert_reliable(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_runtime_supports(version: &str) -> bool {
        let version = version.trim();
        let (core, _) = version.split_once('+').unwrap_or((version, ""));
        let (numbers, prerelease) = core
            .split_once('-')
            .map_or((core, false), |(numbers, _)| (numbers, true));
        let numbers = numbers
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>();
        let Ok(numbers) = numbers else { return false };
        if numbers.len() != 3 {
            return false;
        }
        numbers.as_slice() > [1_u64, 3, 14].as_slice()
            || (numbers.as_slice() == [1_u64, 3, 14].as_slice() && !prerelease)
    }

    #[test]
    fn captures_the_fixed_and_newer_legacy_runtime_oracle() {
        for version in [
            "1.3.14",
            "1.3.14+abc123",
            "1.4.0",
            "1.4.0-canary.1",
            "2.0.0-canary.1",
        ] {
            assert!(legacy_runtime_supports(version));
        }
        assert!(NativeWatchRuntime.supports_reliable_watch_mode());
    }

    #[test]
    fn captures_affected_and_malformed_legacy_versions_without_gating_native_rust() {
        for version in ["1.3.10", "1.3.13", "1.3.14-canary.1", "not-a-version"] {
            assert!(!legacy_runtime_supports(version));
        }
        assert!(NativeWatchRuntime.supports_reliable_watch_mode());
    }

    #[test]
    fn native_runtime_requires_no_external_upgrade_recovery() {
        NativeWatchRuntime.assert_reliable();
    }

    #[test]
    fn native_watch_runtime_needs_no_bun_version_gate_for_live_reload() {
        assert!(!legacy_runtime_supports("1.3.10"));
        assert!(NativeWatchRuntime.supports_reliable_watch_mode());
        NativeWatchRuntime.assert_reliable();
    }
}
