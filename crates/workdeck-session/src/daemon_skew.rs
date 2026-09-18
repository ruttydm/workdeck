//! Turns a daemon's admin status into a skew direction and the notice a window should show.

use serde::{Deserialize, Serialize};

use crate::{
    SessionBrokerAdminStatusV1, WORKDECK_DAEMON_CLIENT_NEWER_MESSAGE,
    WORKDECK_DAEMON_CLIENT_OLDER_MESSAGE, WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE,
    WORKDECK_SESSION_DAEMON_VERSION, WorkdeckDaemonAdminProbe,
};

/// Which side of a version skew this client is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonSkewDirection {
    ClientNewer,
    ClientOlder,
    Matched,
}

/// The direction or `unknown` before the admin probe has answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonSkewKnowledge {
    Known(DaemonSkewDirection),
    Unknown,
}

/// One build as the skew surface describes it: the hello revision and the package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonBuild {
    pub daemon_version: u64,
    pub app_version: String,
}

/// The notice for one probe result, plus which side of the skew it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonSkewNotice {
    pub direction: DaemonSkewKnowledge,
    pub notice: String,
}

/// What a surface needs to know about the daemon link: connected, or disconnected with the
/// notice to keep on screen and, once the admin probe has answered, which side of the skew
/// this window is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkdeckDaemonConnectionState {
    Connected,
    Disconnected {
        notice: String,
        direction: DaemonSkewKnowledge,
    },
}

/// The build this process speaks.
#[must_use]
pub fn current_daemon_build() -> DaemonBuild {
    DaemonBuild {
        daemon_version: u64::from(WORKDECK_SESSION_DAEMON_VERSION),
        app_version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// Compare a daemon's revision to this build's.
#[must_use]
pub fn compare_daemon_build(daemon_version: u64, client_version: u64) -> DaemonSkewDirection {
    if daemon_version == client_version {
        DaemonSkewDirection::Matched
    } else if daemon_version < client_version {
        DaemonSkewDirection::ClientNewer
    } else {
        DaemonSkewDirection::ClientOlder
    }
}

/// Narrow one admin status to the two build facts the notices need.
#[must_use]
pub fn daemon_build_from_status(status: &SessionBrokerAdminStatusV1) -> DaemonBuild {
    DaemonBuild {
        daemon_version: status.daemon_version,
        app_version: status.app_version.clone(),
    }
}

/// Resolve the notice for a refused hello from what the admin scope reported.
///
/// A daemon that does not speak the admin scope, or none at all, keeps the generic wait
/// message. "Client newer" is recoverable in place with `workdeck daemon restart`; "client
/// older" is not, so that window must be relaunched.
#[must_use]
pub fn daemon_skew_notice(
    probe: &WorkdeckDaemonAdminProbe,
    client: &DaemonBuild,
) -> DaemonSkewNotice {
    let WorkdeckDaemonAdminProbe::Status(status) = probe else {
        return DaemonSkewNotice {
            direction: DaemonSkewKnowledge::Unknown,
            notice: WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE.into(),
        };
    };
    match compare_daemon_build(status.daemon_version, client.daemon_version) {
        DaemonSkewDirection::ClientNewer => DaemonSkewNotice {
            direction: DaemonSkewKnowledge::Known(DaemonSkewDirection::ClientNewer),
            notice: WORKDECK_DAEMON_CLIENT_NEWER_MESSAGE.into(),
        },
        DaemonSkewDirection::ClientOlder => DaemonSkewNotice {
            direction: DaemonSkewKnowledge::Known(DaemonSkewDirection::ClientOlder),
            notice: WORKDECK_DAEMON_CLIENT_OLDER_MESSAGE.into(),
        },
        DaemonSkewDirection::Matched => DaemonSkewNotice {
            // The hello was refused for a reason other than the revision; say what we know.
            direction: DaemonSkewKnowledge::Known(DaemonSkewDirection::Matched),
            notice: WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SessionBrokerAdminSessionV1, WORKDECK_DAEMON_REGISTRATION_REJECTED_MESSAGE};

    fn client() -> DaemonBuild {
        DaemonBuild {
            daemon_version: 15,
            app_version: "0.22.0".into(),
        }
    }

    fn status_probe(daemon_version: u64, app_version: &str) -> WorkdeckDaemonAdminProbe {
        WorkdeckDaemonAdminProbe::Status(SessionBrokerAdminStatusV1 {
            admin_scope_version: 1,
            daemon_version,
            app_version: app_version.into(),
            pid: 4242,
            started_at: "2026-01-01T00:00:00.000Z".into(),
            uptime_ms: 1_000,
            sessions: Vec::<SessionBrokerAdminSessionV1>::new(),
        })
    }

    #[test]
    fn compares_revisions_from_the_client_point_of_view() {
        assert_eq!(
            compare_daemon_build(12, 15),
            DaemonSkewDirection::ClientNewer
        );
        assert_eq!(
            compare_daemon_build(16, 15),
            DaemonSkewDirection::ClientOlder
        );
        assert_eq!(compare_daemon_build(15, 15), DaemonSkewDirection::Matched);
    }

    #[test]
    fn names_the_remedy_without_versions_when_the_daemon_is_older() {
        assert_eq!(
            daemon_skew_notice(&status_probe(12, "0.21.1"), &client()),
            DaemonSkewNotice {
                direction: DaemonSkewKnowledge::Known(DaemonSkewDirection::ClientNewer),
                notice: "Session daemon is an older Workdeck build. Run `workdeck daemon restart`."
                    .into(),
            }
        );
    }

    #[test]
    fn keeps_the_same_wording_when_package_versions_match_but_revisions_differ() {
        assert_eq!(
            daemon_skew_notice(&status_probe(14, "0.22.0"), &client()).notice,
            WORKDECK_DAEMON_CLIENT_NEWER_MESSAGE
        );
        assert_eq!(
            daemon_skew_notice(&status_probe(16, "0.22.0"), &client()).notice,
            WORKDECK_DAEMON_CLIENT_OLDER_MESSAGE
        );
    }

    #[test]
    fn tells_an_older_window_to_relaunch() {
        assert_eq!(
            daemon_skew_notice(&status_probe(16, "0.23.0"), &client()),
            DaemonSkewNotice {
                direction: DaemonSkewKnowledge::Known(DaemonSkewDirection::ClientOlder),
                notice: WORKDECK_DAEMON_CLIENT_OLDER_MESSAGE.into(),
            }
        );
    }

    #[test]
    fn keeps_the_generic_wait_message_when_the_daemon_predates_the_admin_scope() {
        for probe in [
            WorkdeckDaemonAdminProbe::Unsupported,
            WorkdeckDaemonAdminProbe::Unavailable,
            status_probe(15, "0.22.0"),
        ] {
            assert_eq!(
                daemon_skew_notice(&probe, &client()).notice,
                WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE
            );
        }
        assert_eq!(
            daemon_skew_notice(&WorkdeckDaemonAdminProbe::Unsupported, &client()).direction,
            DaemonSkewKnowledge::Unknown
        );
    }

    #[test]
    fn the_registration_rejection_notice_is_distinct_from_skew_notices() {
        assert_ne!(
            WORKDECK_DAEMON_REGISTRATION_REJECTED_MESSAGE,
            WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE
        );
    }
}
