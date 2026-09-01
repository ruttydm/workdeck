//! Immutable application contract and local-only session broker configuration.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use thiserror::Error;

pub const WORKDECK_SESSION_BROKER_APP_ID: &str = "dev.workdeck";
pub const SESSION_BROKER_PROTOCOL_REVISION: u32 = 1;
pub const WORKDECK_SESSION_DAEMON_VERSION: u32 = 12;
pub const WORKDECK_SESSION_BROKER_APP_REVISION: u32 = WORKDECK_SESSION_DAEMON_VERSION;
pub const WORKDECK_SESSION_BROKER_FEATURES: &[&str] = &[];

pub const DEFAULT_SESSION_BROKER_HOST: &str = "127.0.0.1";
pub const DEFAULT_SESSION_BROKER_PORT: u32 = 47_657;
pub const SESSION_BROKER_HOST_ENV: &str = "WORKDECK_MCP_HOST";
pub const SESSION_BROKER_PORT_ENV: &str = "WORKDECK_MCP_PORT";
pub const LEGACY_MCP_PATH: &str = "/mcp";
pub const SESSION_BROKER_SOCKET_PATH: &str = "/session";
pub const UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV: &str = "WORKDECK_MCP_UNSAFE_ALLOW_REMOTE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerAppContract {
    pub app_revision: u32,
    pub features: &'static [&'static str],
}

pub const WORKDECK_SESSION_BROKER_APP_CONTRACT: BrokerAppContract = BrokerAppContract {
    app_revision: WORKDECK_SESSION_BROKER_APP_REVISION,
    features: WORKDECK_SESSION_BROKER_FEATURES,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSessionBrokerConfig {
    pub host: String,
    pub port: u32,
    pub http_origin: String,
    pub ws_origin: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionBrokerConfigError {
    #[error(
        "Session broker refuses to bind {host}:{port} because it is local-only by default. Use a loopback host such as 127.0.0.1, localhost, or ::1, or set {unsafe_env}=1 if you intentionally want remote access."
    )]
    RemoteBind {
        host: String,
        port: u32,
        unsafe_env: &'static str,
    },
}

/// Return whether one bind host stays on the local loopback interface.
#[must_use]
pub fn is_loopback_host(host: &str) -> bool {
    let normalized = host.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    if matches!(normalized.as_str(), "localhost" | "::1" | "0:0:0:0:0:0:0:1") {
        return true;
    }
    if let Some(inner) = normalized
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        return is_loopback_host(inner);
    }
    if let Some(mapped) = normalized.strip_prefix("::ffff:") {
        return is_loopback_host(mapped);
    }
    normalized
        .parse::<Ipv4Addr>()
        .is_ok_and(|address| address.octets()[0] == 127)
}

#[must_use]
pub fn allows_unsafe_remote_session_broker(env: &BTreeMap<String, String>) -> bool {
    env.get(UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV)
        .is_some_and(|value| value == "1")
}

/// Resolve one broker address from an explicit environment snapshot.
pub fn resolve_session_broker_config(
    env: &BTreeMap<String, String>,
) -> Result<ResolvedSessionBrokerConfig, SessionBrokerConfigError> {
    let host = env
        .get(SESSION_BROKER_HOST_ENV)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_SESSION_BROKER_HOST)
        .to_owned();
    let port = env
        .get(SESSION_BROKER_PORT_ENV)
        .and_then(|value| parse_js_positive_integer(value))
        .unwrap_or(DEFAULT_SESSION_BROKER_PORT);
    if !is_loopback_host(&host) && !allows_unsafe_remote_session_broker(env) {
        return Err(SessionBrokerConfigError::RemoteBind {
            host,
            port,
            unsafe_env: UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV,
        });
    }
    let url_host = if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]")
    } else {
        host.clone()
    };
    Ok(ResolvedSessionBrokerConfig {
        host,
        port,
        http_origin: format!("http://{url_host}:{port}"),
        ws_origin: format!("ws://{url_host}:{port}"),
    })
}

fn parse_js_positive_integer(value: &str) -> Option<u32> {
    let trimmed = value.trim_start();
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'+') => (false, &trimmed[1..]),
        Some(b'-') => (true, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if negative {
        return None;
    }
    let digit_count = digits.bytes().take_while(u8::is_ascii_digit).count();
    (digit_count > 0)
        .then(|| digits[..digit_count].parse::<u32>().ok())
        .flatten()
        .filter(|port| *port > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
        entries
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into()))
            .collect()
    }

    #[test]
    fn exports_one_fixed_phase_one_workdeck_contract() {
        assert_eq!(WORKDECK_SESSION_BROKER_APP_ID, "dev.workdeck");
        assert_eq!(SESSION_BROKER_PROTOCOL_REVISION, 1);
        assert_eq!(
            WORKDECK_SESSION_BROKER_APP_REVISION,
            WORKDECK_SESSION_DAEMON_VERSION
        );
        assert!(WORKDECK_SESSION_BROKER_APP_CONTRACT.features.is_empty());
    }

    #[test]
    fn resolves_exported_defaults_and_environment_metadata() {
        let defaults = resolve_session_broker_config(&BTreeMap::new()).unwrap();
        assert_eq!(defaults.host, DEFAULT_SESSION_BROKER_HOST);
        assert_eq!(defaults.port, DEFAULT_SESSION_BROKER_PORT);
        let configured = resolve_session_broker_config(&env(&[
            (SESSION_BROKER_HOST_ENV, "localhost"),
            (SESSION_BROKER_PORT_ENV, "49000"),
        ]))
        .unwrap();
        assert_eq!(configured.host, "localhost");
        assert_eq!(configured.port, 49_000);
    }

    #[test]
    fn formats_ipv6_literal_origins_with_authority_brackets() {
        let configured = resolve_session_broker_config(&env(&[
            (SESSION_BROKER_HOST_ENV, "::1"),
            (SESSION_BROKER_PORT_ENV, "49000"),
        ]))
        .unwrap();
        assert_eq!(configured.http_origin, "http://[::1]:49000");
        assert_eq!(configured.ws_origin, "ws://[::1]:49000");
    }

    #[test]
    fn recognizes_only_loopback_hosts_without_an_override() {
        for host in [
            "127.0.0.1",
            "127.1.2.3",
            "localhost",
            "::1",
            "0:0:0:0:0:0:0:1",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_loopback_host(host), "{host}");
        }
        for host in ["0.0.0.0", "192.168.1.20", "example.com"] {
            assert!(!is_loopback_host(host), "{host}");
        }
    }

    #[test]
    fn refuses_remote_binds_without_an_explicit_unsafe_override() {
        let remote = env(&[
            (SESSION_BROKER_HOST_ENV, "0.0.0.0"),
            (SESSION_BROKER_PORT_ENV, "49000"),
        ]);
        assert!(
            resolve_session_broker_config(&remote)
                .unwrap_err()
                .to_string()
                .contains("local-only by default")
        );
        let allowed = resolve_session_broker_config(&env(&[
            (SESSION_BROKER_HOST_ENV, "0.0.0.0"),
            (SESSION_BROKER_PORT_ENV, "49000"),
            (UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV, "1"),
        ]))
        .unwrap();
        assert_eq!(allowed.host, "0.0.0.0");
        assert_eq!(allowed.port, 49_000);
    }

    #[test]
    fn unsafe_remote_access_requires_the_exact_value_one() {
        assert!(!allows_unsafe_remote_session_broker(&BTreeMap::new()));
        assert!(!allows_unsafe_remote_session_broker(&env(&[(
            UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV,
            "0"
        ),])));
        assert!(allows_unsafe_remote_session_broker(&env(&[(
            UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV,
            "1"
        ),])));
    }

    #[test]
    fn port_parsing_matches_number_parse_int_prefix_behavior() {
        assert_eq!(parse_js_positive_integer("  +49000tail"), Some(49_000));
        assert_eq!(parse_js_positive_integer("0"), None);
        assert_eq!(parse_js_positive_integer("-1"), None);
        assert_eq!(parse_js_positive_integer("nope"), None);
    }
}
