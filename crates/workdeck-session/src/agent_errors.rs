//! Stable agent-facing error wording for the `workdeck session` surface.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentErrorDoc {
    pub quote: &'static str,
    pub remedy: &'static str,
}

pub const AGENT_ERROR_DOCS: [AgentErrorDoc; 12] = [
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
        SelectableSession, SessionBrokerListedSession, SessionSelector, resolve_session_target,
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
            review_resource_unavailable_message("src/App.tsx"),
        ];
        assert_eq!(real_messages.len(), AGENT_ERROR_DOCS.len());
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
}
