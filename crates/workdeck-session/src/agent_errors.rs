//! Stable agent-facing error wording for the `workdeck session` surface.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    DaemonBuild, WORKDECK_BUILD_RELATION_NEWER, WORKDECK_BUILD_RELATION_OLDER,
    WORKDECK_DAEMON_RESTART_COMMAND, WORKDECK_WINDOW_RELAUNCH_CLAUSE, daemon_restart_disconnects,
    describe_attached_windows,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCommandConstraintKind {
    ExactlyOne,
    AtMostOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCommandConstraint {
    pub kind: AgentCommandConstraintKind,
    pub label: Option<&'static str>,
    pub documentation_scope: Option<&'static str>,
    pub flags: &'static [&'static str],
}

pub const NAVIGATE_TARGET_CONSTRAINT: AgentCommandConstraint = AgentCommandConstraint {
    kind: AgentCommandConstraintKind::ExactlyOne,
    label: Some("navigation target"),
    documentation_scope: Some("for `--file` navigation"),
    flags: &["--hunk <n>", "--old-line <n>", "--new-line <n>"],
};

pub const COMMENT_TARGET_CONSTRAINT: AgentCommandConstraint = AgentCommandConstraint {
    kind: AgentCommandConstraintKind::ExactlyOne,
    label: Some("comment target"),
    documentation_scope: None,
    flags: &["--old-line <n>", "--new-line <n>"],
};

pub const HIGHLIGHT_TARGET_CONSTRAINT: AgentCommandConstraint = AgentCommandConstraint {
    kind: AgentCommandConstraintKind::ExactlyOne,
    label: Some("highlight target"),
    documentation_scope: None,
    flags: &["--old-line <n>", "--new-line <n>"],
};

pub const COMMENT_DIRECTION_CONSTRAINT: AgentCommandConstraint = AgentCommandConstraint {
    kind: AgentCommandConstraintKind::AtMostOne,
    label: None,
    documentation_scope: None,
    flags: &["--next-comment", "--prev-comment"],
};

fn format_flag_choices(flags: &[&str]) -> String {
    match flags {
        [] => String::new(),
        [flag] => (*flag).into(),
        [left, right] => format!("{left} or {right}"),
        _ => format!(
            "{}, or {}",
            flags[..flags.len() - 1].join(", "),
            flags[flags.len() - 1]
        ),
    }
}

#[must_use]
pub fn constraint_violation_message(constraint: AgentCommandConstraint) -> String {
    match constraint.kind {
        AgentCommandConstraintKind::ExactlyOne => format!(
            "Specify exactly one {}: {}.",
            constraint.label.unwrap_or("target"),
            format_flag_choices(constraint.flags)
        ),
        AgentCommandConstraintKind::AtMostOne => format!(
            "Specify either {}, not both.",
            format_flag_choices(constraint.flags)
        ),
    }
}

pub const RELOAD_SEPARATOR_MESSAGE: &str = "Pass the replacement Workdeck command after `--`, for example `workdeck session reload <session-id> -- diff`.";
pub const COMMENT_APPLY_STDIN_MESSAGE: &str =
    "Pass --stdin to read batch comments from stdin JSON.";
pub const HIGHLIGHT_RANGE_MESSAGE: &str = "Highlight --end must be greater than --start; the range is [start, end) with an exclusive end.";
pub const NO_ACTIVE_SESSIONS_MESSAGE: &str = "No active Workdeck sessions are registered with the daemon. Open Workdeck and wait for it to connect.";

#[must_use]
pub fn no_diff_file_matches_message(file_path: &str) -> String {
    format!("No diff file matches {file_path}.")
}

#[must_use]
pub fn review_resource_unavailable_message(file_path: &str) -> String {
    format!("Could not read the raw diff for {file_path} from the live session.")
}

/// One build as the mismatch error describes it: the hello revision and the package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonBuildMismatchBuildInput {
    pub daemon_version: u64,
    pub app_version: String,
}

impl From<DaemonBuild> for DaemonBuildMismatchBuildInput {
    fn from(build: DaemonBuild) -> Self {
        Self {
            daemon_version: build.daemon_version,
            app_version: build.app_version,
        }
    }
}

/// One attached window as reported by the daemon's admin scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonBuildMismatchSession {
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub pid: u64,
}

/// The windows a restart would disconnect, when the daemon could report them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonBuildMismatchAttachedSessions {
    pub count: usize,
    pub sessions: Vec<DaemonBuildMismatchSession>,
}

/// What the daemon launch metadata says when the daemon itself could not be asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonBuildMismatchLaunch {
    pub pid: u64,
    pub command: String,
    pub launched_at: String,
}

/// What the agent should do about a mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonBuildMismatchAction {
    RestartDaemon,
    UseNewerWorkdeck,
}

/// Structured facts behind a daemon build mismatch, returned verbatim under `--json` so an
/// agent can decide (and ask) before restarting anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonBuildMismatchDetails {
    /// The running daemon's build, or `None` when it predates the admin scope and cannot say.
    pub daemon: Option<DaemonBuildMismatchBuildInput>,
    pub cli: DaemonBuildMismatchBuildInput,
    /// Attached windows when the admin scope answered; `None` when it could not report them.
    pub attached_sessions: Option<DaemonBuildMismatchAttachedSessions>,
    pub launch: Option<DaemonBuildMismatchLaunch>,
    pub recommended_action: DaemonBuildMismatchAction,
}

/// Prefix shared by every mismatch message so the skill can quote one line for both directions.
pub const DAEMON_BUILD_MISMATCH_PREFIX: &str = "The session daemon is";

/// The headline message for one mismatch.
#[must_use]
pub fn daemon_build_mismatch_message(details: &DaemonBuildMismatchDetails) -> String {
    if details.daemon.is_none() {
        let launch = details.launch.as_ref().map_or_else(String::new, |launch| {
            format!(
                " (pid {}, started {}, command {})",
                launch.pid, launch.launched_at, launch.command
            )
        });
        return format!(
            "{DAEMON_BUILD_MISMATCH_PREFIX} {WORKDECK_BUILD_RELATION_OLDER} that predates `workdeck daemon status` and refuses this CLI{launch}."
        );
    }
    let relation = if details.recommended_action == DaemonBuildMismatchAction::RestartDaemon {
        WORKDECK_BUILD_RELATION_OLDER
    } else {
        WORKDECK_BUILD_RELATION_NEWER
    };
    format!("{DAEMON_BUILD_MISMATCH_PREFIX} {relation} and refuses this CLI.")
}

/// The remedy lines that follow the headline in text output.
#[must_use]
pub fn daemon_build_mismatch_suggestions(details: &DaemonBuildMismatchDetails) -> Vec<String> {
    let count = details
        .attached_sessions
        .as_ref()
        .map(|attached| attached.count);
    match details.recommended_action {
        DaemonBuildMismatchAction::UseNewerWorkdeck => vec![format!(
            "Use the newer Workdeck build the daemon was started from, or run {WORKDECK_DAEMON_RESTART_COMMAND} from this build ({} would be disconnected and could not reconnect).",
            describe_attached_windows(count)
        )],
        DaemonBuildMismatchAction::RestartDaemon => vec![
            format!(
                "Run {WORKDECK_DAEMON_RESTART_COMMAND} to replace it, then re-run `workdeck session list`; windows that could not register attach automatically."
            ),
            format!(
                "{}; they {WORKDECK_WINDOW_RELAUNCH_CLAUSE} Closing them instead lets the daemon exit on its own after about a minute.",
                daemon_restart_disconnects(count)
            ),
        ],
    }
}

/// The error every `workdeck session` command raises when the daemon and this CLI disagree on
/// the build.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", daemon_build_mismatch_message(&self.details))]
pub struct DaemonBuildMismatchError {
    pub details: DaemonBuildMismatchDetails,
}

impl DaemonBuildMismatchError {
    #[must_use]
    pub fn new(details: DaemonBuildMismatchDetails) -> Self {
        Self { details }
    }

    /// The `--json` error body: the message plus every structured fact.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let details = serde_json::to_value(&self.details).unwrap_or_else(|_| json!({}));
        let mut body = json!({
            "message": daemon_build_mismatch_message(&self.details),
            "kind": "daemon-build-mismatch",
        });
        if let serde_json::Value::Object(record) = details {
            let target = body.as_object_mut().expect("body is an object");
            for (key, value) in record {
                target.insert(key, value);
            }
        }
        body
    }

    /// The remedy lines shown in text output.
    #[must_use]
    pub fn suggestions(&self) -> Vec<String> {
        daemon_build_mismatch_suggestions(&self.details)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentErrorDoc {
    pub quote: &'static str,
    pub remedy: &'static str,
}

pub const AGENT_ERROR_DOCS: [AgentErrorDoc; 13] = [
    AgentErrorDoc {
        quote: "No diff file matches ...",
        remedy: "the file is not in the loaded review. Check `context`, then `reload` if needed.",
    },
    AgentErrorDoc {
        quote: "No active Workdeck sessions",
        remedy: "if Workdeck is visibly running, localhost may be blocked by the agent sandbox; retry with network/sandbox escalation. Otherwise ask the user to open Workdeck.",
    },
    AgentErrorDoc {
        quote: "Multiple active sessions match",
        remedy: "pass `<session-id>` explicitly.",
    },
    AgentErrorDoc {
        quote: "No active session matches session path ...",
        remedy: "for advanced split-path reloads, verify the live window `Path` via `workdeck session get` or `list`, then use `--session-path`.",
    },
    AgentErrorDoc {
        quote: "Pass the replacement Workdeck command after `--`",
        remedy: "include `--` before the nested `diff` / `show` command.",
    },
    AgentErrorDoc {
        quote: COMMENT_APPLY_STDIN_MESSAGE,
        remedy: "`comment apply` only reads its batch payload from stdin.",
    },
    AgentErrorDoc {
        quote: "Specify exactly one navigation target",
        remedy: "pick one of `--hunk`, `--old-line`, or `--new-line`.",
    },
    AgentErrorDoc {
        quote: "Specify exactly one comment target",
        remedy: "pass `comment add` one of `--old-line` or `--new-line`.",
    },
    AgentErrorDoc {
        quote: "Specify exactly one highlight target",
        remedy: "pass `highlight add` one of `--old-line` or `--new-line`.",
    },
    AgentErrorDoc {
        quote: "Highlight --end must be greater than --start",
        remedy: "offsets are `[start, end)` UTF-16 code units into the line text; end is exclusive.",
    },
    AgentErrorDoc {
        quote: "Specify either --next-comment or --prev-comment, not both.",
        remedy: "choose one comment-navigation direction.",
    },
    AgentErrorDoc {
        quote: "The session daemon is ...",
        remedy: "a `daemon-build-mismatch` (the `--json` error carries `daemon`, `cli`, `attachedSessions`, and `recommendedAction`). Tell the user which build is newer and how many windows are attached, then **ask** before running `workdeck daemon restart --yes`; never restart unprompted. After the restart, windows that failed to register attach on their own, so re-run `workdeck session list` instead of relaunching anything. When `recommendedAction` is `use-newer-workdeck`, the daemon is the newer build: use that Workdeck instead.",
    },
    AgentErrorDoc {
        quote: "Could not read the raw diff for ...",
        remedy: "the session reloaded or closed while `--include-patch` was reading it. Re-run `review`; drop `--include-patch` if you only need file and hunk structure.",
    },
];

#[must_use]
pub fn agent_error_quote_prefix(doc: &AgentErrorDoc) -> &str {
    doc.quote.strip_suffix(" ...").unwrap_or(doc.quote)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{
        DaemonSkewDirection, SelectableSession, SessionBrokerListedSession, SessionSelector,
        compare_daemon_build, resolve_session_target,
    };

    use super::*;

    #[derive(Debug, Clone)]
    struct TestSession {
        session: SelectableSession,
        title: String,
    }

    impl SessionBrokerListedSession for TestSession {
        fn selectable_session(&self) -> SelectableSession {
            self.session.clone()
        }

        fn title(&self) -> &str {
            &self.title
        }

        fn snapshot_updated_at(&self) -> &str {
            "2026-01-01T00:00:00.000Z"
        }
    }

    fn session(session_id: &str) -> TestSession {
        TestSession {
            session: SelectableSession {
                session_id: session_id.into(),
                cwd: PathBuf::from(format!("/tmp/{session_id}")),
                repo_root: Some(PathBuf::from("/tmp/shared-repo")),
            },
            title: format!("title-{session_id}"),
        }
    }

    fn mismatch_details(
        daemon: Option<DaemonBuildMismatchBuildInput>,
        attached: Option<usize>,
        recommended_action: DaemonBuildMismatchAction,
        launch: Option<DaemonBuildMismatchLaunch>,
    ) -> DaemonBuildMismatchDetails {
        DaemonBuildMismatchDetails {
            daemon,
            cli: DaemonBuildMismatchBuildInput {
                daemon_version: 15,
                app_version: "0.22.0".into(),
            },
            attached_sessions: attached.map(|count| DaemonBuildMismatchAttachedSessions {
                count,
                sessions: (1..=count)
                    .map(|index| DaemonBuildMismatchSession {
                        session_id: format!("session-{index}"),
                        title: format!("review {index}"),
                        cwd: "/repo".into(),
                        pid: 100 + index as u64,
                    })
                    .collect(),
            }),
            launch,
            recommended_action,
        }
    }

    #[test]
    fn formats_exactly_one_constraints_with_oxford_comma() {
        assert_eq!(
            constraint_violation_message(NAVIGATE_TARGET_CONSTRAINT),
            "Specify exactly one navigation target: --hunk <n>, --old-line <n>, or --new-line <n>."
        );
        assert_eq!(
            constraint_violation_message(COMMENT_TARGET_CONSTRAINT),
            "Specify exactly one comment target: --old-line <n> or --new-line <n>."
        );
    }

    #[test]
    fn formats_at_most_one_constraints_as_either_or() {
        assert_eq!(
            constraint_violation_message(COMMENT_DIRECTION_CONSTRAINT),
            "Specify either --next-comment or --prev-comment, not both."
        );
    }

    #[test]
    fn every_documented_quote_binds_to_one_real_message() {
        let sessions = [session("one"), session("two")];
        let mismatch = DaemonBuildMismatchError::new(mismatch_details(
            Some(DaemonBuildMismatchBuildInput {
                daemon_version: 12,
                app_version: "0.21.1".into(),
            }),
            Some(0),
            DaemonBuildMismatchAction::RestartDaemon,
            None,
        ));
        let real_messages = [
            no_diff_file_matches_message("src/App.tsx"),
            NO_ACTIVE_SESSIONS_MESSAGE.into(),
            resolve_session_target(
                &sessions,
                &SessionSelector {
                    repo_root: Some("/tmp/shared-repo".into()),
                    ..SessionSelector::default()
                },
            )
            .unwrap_err()
            .to_string(),
            resolve_session_target(
                &sessions,
                &SessionSelector {
                    session_path: Some("/tmp/missing".into()),
                    ..SessionSelector::default()
                },
            )
            .unwrap_err()
            .to_string(),
            RELOAD_SEPARATOR_MESSAGE.into(),
            COMMENT_APPLY_STDIN_MESSAGE.into(),
            constraint_violation_message(NAVIGATE_TARGET_CONSTRAINT),
            constraint_violation_message(COMMENT_TARGET_CONSTRAINT),
            constraint_violation_message(HIGHLIGHT_TARGET_CONSTRAINT),
            HIGHLIGHT_RANGE_MESSAGE.into(),
            constraint_violation_message(COMMENT_DIRECTION_CONSTRAINT),
            daemon_build_mismatch_message(&mismatch.details),
            review_resource_unavailable_message("src/App.tsx"),
        ];
        assert_eq!(real_messages.len(), AGENT_ERROR_DOCS.len());
        assert!(real_messages.contains(
            &"The session daemon is an older Workdeck build and refuses this CLI.".to_owned()
        ));
        assert!(
            AGENT_ERROR_DOCS
                .iter()
                .any(|doc| doc.quote == "The session daemon is ...")
        );
        for doc in &AGENT_ERROR_DOCS {
            let prefix = agent_error_quote_prefix(doc);
            assert!(
                real_messages
                    .iter()
                    .any(|message| message.starts_with(prefix)),
                "{prefix}"
            );
        }
        for message in &real_messages {
            assert_eq!(
                AGENT_ERROR_DOCS
                    .iter()
                    .filter(|doc| message.starts_with(agent_error_quote_prefix(doc)))
                    .count(),
                1,
                "{message}"
            );
        }
    }

    #[test]
    fn omits_absent_pre_admin_launch_metadata() {
        let error = DaemonBuildMismatchError::new(mismatch_details(
            None,
            None,
            DaemonBuildMismatchAction::RestartDaemon,
            None,
        ));
        assert_eq!(
            error.to_string(),
            "The session daemon is an older Workdeck build that predates `workdeck daemon status` and refuses this CLI."
        );
    }

    #[test]
    fn keeps_equal_package_versions_in_json_only_and_singular_counts_in_remedies() {
        for recommended_action in [
            DaemonBuildMismatchAction::RestartDaemon,
            DaemonBuildMismatchAction::UseNewerWorkdeck,
        ] {
            let details = mismatch_details(
                Some(DaemonBuildMismatchBuildInput {
                    daemon_version: if recommended_action
                        == DaemonBuildMismatchAction::RestartDaemon
                    {
                        14
                    } else {
                        16
                    },
                    app_version: "0.22.0".into(),
                }),
                Some(1),
                recommended_action,
                None,
            );
            let error = DaemonBuildMismatchError::new(details.clone());
            assert_eq!(
                error.to_string(),
                format!(
                    "The session daemon is {} and refuses this CLI.",
                    if recommended_action == DaemonBuildMismatchAction::RestartDaemon {
                        WORKDECK_BUILD_RELATION_OLDER
                    } else {
                        WORKDECK_BUILD_RELATION_NEWER
                    }
                )
            );
            assert_eq!(
                error.suggestions(),
                if recommended_action == DaemonBuildMismatchAction::RestartDaemon {
                    vec![
                        String::from(
                            "Run `workdeck daemon restart` to replace it, then re-run `workdeck session list`; windows that could not register attach automatically.",
                        ),
                        String::from(
                            "Restarting disconnects 1 attached window; they must be relaunched, losing their notes. Closing them instead lets the daemon exit on its own after about a minute.",
                        ),
                    ]
                } else {
                    vec![String::from(
                        "Use the newer Workdeck build the daemon was started from, or run `workdeck daemon restart` from this build (1 attached window would be disconnected and could not reconnect).",
                    )]
                }
            );
            let mut expected =
                json!({"message": error.to_string(), "kind": "daemon-build-mismatch"});
            let serialized = serde_json::to_value(&details).unwrap();
            if let (Some(target), Some(serde_json::Value::Object(record))) =
                (expected.as_object_mut(), Some(serialized))
            {
                for (key, value) in record {
                    target.insert(key, value);
                }
            }
            assert_eq!(error.to_json(), expected);
        }
    }

    #[test]
    fn the_recommended_action_follows_the_skew_direction() {
        assert_eq!(
            compare_daemon_build(12, 15),
            DaemonSkewDirection::ClientNewer
        );
        assert_eq!(
            compare_daemon_build(16, 15),
            DaemonSkewDirection::ClientOlder
        );
    }
}
