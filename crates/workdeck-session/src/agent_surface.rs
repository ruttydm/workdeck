//! One declarative source for Workdeck's live-session CLI, help, and generated skill.

use std::sync::LazyLock;

use crate::{
    AgentCommandConstraint, COMMENT_DIRECTION_CONSTRAINT, COMMENT_TARGET_CONSTRAINT,
    HIGHLIGHT_TARGET_CONSTRAINT, NAVIGATE_TARGET_CONSTRAINT, SessionDaemonAction,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOptionParser {
    PositiveInt,
    NonNegativeInt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommandOption {
    pub flag: &'static str,
    pub description: &'static str,
    pub parse: Option<AgentOptionParser>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommandPositional {
    pub token: &'static str,
    pub description: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommandSpec {
    pub action: SessionDaemonAction,
    pub name: &'static str,
    pub summary: &'static str,
    pub positionals: Vec<AgentCommandPositional>,
    pub options: Vec<AgentCommandOption>,
    pub synopsis: Vec<&'static str>,
    pub constraints: Vec<AgentCommandConstraint>,
    pub examples: Vec<&'static str>,
    pub help_extra: Vec<&'static str>,
}

const fn option(flag: &'static str, description: &'static str) -> AgentCommandOption {
    AgentCommandOption {
        flag,
        description,
        parse: None,
        required: false,
    }
}

const fn parsed_option(
    flag: &'static str,
    description: &'static str,
    parse: AgentOptionParser,
) -> AgentCommandOption {
    AgentCommandOption {
        flag,
        description,
        parse: Some(parse),
        required: false,
    }
}

const fn required(mut option: AgentCommandOption) -> AgentCommandOption {
    option.required = true;
    option
}

const fn positional(
    token: &'static str,
    description: Option<&'static str>,
) -> AgentCommandPositional {
    AgentCommandPositional { token, description }
}

pub const AUXILIARY_AGENT_OPTIONS: [(&str, AgentCommandOption); 4] = [
    (
        "agentContext",
        option(
            "--agent-context <path>",
            "JSON sidecar with agent rationale",
        ),
    ),
    (
        "excludeUntracked",
        option(
            "--exclude-untracked",
            "exclude untracked files from working tree reviews",
        ),
    ),
    (
        "experimental",
        option(
            "--experimental",
            "enable experimental features (currently STML agent-note markup)",
        ),
    ),
    (
        "markupWidth",
        option("--width <n>", "layout width in columns"),
    ),
];

pub const SESSION_SELECTOR_SYNOPSIS: &str = "(<session-id> | --repo <path>)";
pub const RELOAD_SELECTOR_SYNOPSIS: &str = "(<session-id> | --repo <path> | --session-path <path>)";
pub const HIGHLIGHT_TONES: [&str; 6] = ["match", "current", "info", "warning", "error", "dim"];

#[must_use]
pub fn is_highlight_tone(value: &str) -> bool {
    HIGHLIGHT_TONES.contains(&value)
}

#[must_use]
pub fn constraint_synopsis(constraint: AgentCommandConstraint) -> String {
    format!("({})", constraint.flags.join(" | "))
}

#[must_use]
pub fn option_key_from_flag(flag: &str) -> String {
    let body = flag
        .split_once(' ')
        .map_or(flag, |(name, _)| name)
        .strip_prefix("--")
        .unwrap_or(flag);
    let mut key = String::with_capacity(body.len());
    let mut uppercase = false;
    for character in body.chars() {
        if character == '-' {
            uppercase = true;
        } else if uppercase {
            key.extend(character.to_uppercase());
            uppercase = false;
        } else {
            key.push(character);
        }
    }
    key
}

#[must_use]
pub fn agent_option_flag_name(option: &AgentCommandOption) -> &str {
    option
        .flag
        .split_once(' ')
        .map_or(option.flag, |(name, _)| name)
}

fn repo_option() -> AgentCommandOption {
    option(
        "--repo <path>",
        "target the live session whose repo root matches this path",
    )
}

fn json_option() -> AgentCommandOption {
    option("--json", "emit structured JSON")
}

fn diff_file_option() -> AgentCommandOption {
    option("--file <path>", "diff file path as shown by Workdeck")
}

fn old_line_option() -> AgentCommandOption {
    parsed_option(
        "--old-line <n>",
        "1-based line number on the old side",
        AgentOptionParser::PositiveInt,
    )
}

fn new_line_option() -> AgentCommandOption {
    parsed_option(
        "--new-line <n>",
        "1-based line number on the new side",
        AgentOptionParser::PositiveInt,
    )
}

fn spec(
    action: SessionDaemonAction,
    name: &'static str,
    summary: &'static str,
    positionals: Vec<AgentCommandPositional>,
    options: Vec<AgentCommandOption>,
    synopsis: Vec<&'static str>,
) -> AgentCommandSpec {
    AgentCommandSpec {
        action,
        name,
        summary,
        positionals,
        options,
        synopsis,
        constraints: Vec::new(),
        examples: Vec::new(),
        help_extra: Vec::new(),
    }
}

/// Every daemon action in display order, used by both CLI registration and generated docs.
pub static SESSION_AGENT_COMMAND_LIST: LazyLock<Vec<AgentCommandSpec>> = LazyLock::new(|| {
    let mut commands = Vec::with_capacity(13);
    commands.push(spec(
        SessionDaemonAction::List,
        "session list",
        "list live Workdeck sessions",
        vec![],
        vec![json_option()],
        vec!["workdeck session list [--json]"],
    ));
    commands.push(spec(
        SessionDaemonAction::Get,
        "session get",
        "show one live Workdeck session",
        vec![positional("[sessionId]", None)],
        vec![repo_option(), json_option()],
        vec!["workdeck session get (<session-id> | --repo <path>) [--json]"],
    ));
    commands.push(spec(
        SessionDaemonAction::Context,
        "session context",
        "show the selected file and hunk for one live Workdeck session",
        vec![positional("[sessionId]", None)],
        vec![repo_option(), json_option()],
        vec!["workdeck session context (<session-id> | --repo <path>) [--json]"],
    ));
    commands.push(spec(
        SessionDaemonAction::Review,
        "session review",
        "export the live review model for one Workdeck session",
        vec![positional("[sessionId]", None)],
        vec![
            repo_option(),
            option(
                "--include-patch",
                "include raw unified diff text for each file in review output",
            ),
            option(
                "--include-notes",
                "include live review notes in review output",
            ),
            json_option(),
        ],
        vec![
            "workdeck session review (<session-id> | --repo <path>) [--include-patch] [--include-notes] [--json]",
        ],
    ));

    let mut navigate = spec(
        SessionDaemonAction::Navigate,
        "session navigate",
        "move a live Workdeck session to one diff hunk",
        vec![positional("[sessionId]", None)],
        vec![
            diff_file_option(),
            repo_option(),
            parsed_option(
                "--hunk <n>",
                "1-based hunk number within the file",
                AgentOptionParser::PositiveInt,
            ),
            old_line_option(),
            new_line_option(),
            option("--comment <id>", "jump to the live comment with this id"),
            option("--next-comment", "jump to the next annotated hunk"),
            option("--prev-comment", "jump to the previous annotated hunk"),
            json_option(),
        ],
        vec![
            "workdeck session navigate (<session-id> | --repo <path>) --file <path> (--hunk <n> | --old-line <n> | --new-line <n>) [--json]",
            "workdeck session navigate (<session-id> | --repo <path>) --comment <id> [--json]",
            "workdeck session navigate (<session-id> | --repo <path>) (--next-comment | --prev-comment) [--json]",
        ],
    );
    navigate.constraints = vec![NAVIGATE_TARGET_CONSTRAINT, COMMENT_DIRECTION_CONSTRAINT];
    navigate.examples = vec![
        "workdeck session navigate --repo . --file src/App.tsx --hunk 2",
        "workdeck session navigate --repo . --file src/App.tsx --new-line 372",
        "workdeck session navigate --repo . --file src/App.tsx --old-line 355",
        "workdeck session navigate --repo . --comment comment-1",
        "workdeck session navigate --repo . --next-comment",
        "workdeck session navigate --repo . --prev-comment",
    ];
    commands.push(navigate);

    let mut reload = spec(
        SessionDaemonAction::Reload,
        "session reload",
        "replace the contents of one live Workdeck session",
        vec![positional("[sessionId]", None)],
        vec![
            repo_option(),
            option(
                "--session-path <path>",
                "target a live session rooted at a different path",
            ),
            option(
                "--source <path>",
                "load the diff from this directory instead of the session's own",
            ),
            json_option(),
        ],
        vec![
            "workdeck session reload (<session-id> | --repo <path> | --session-path <path>) [--source <path>] [--json] -- diff [ref] [-- <pathspec...>]",
            "workdeck session reload (<session-id> | --repo <path> | --session-path <path>) [--source <path>] [--json] -- show [ref] [-- <pathspec...>]",
        ],
    );
    reload.examples = vec![
        "workdeck session reload --repo . -- diff",
        "workdeck session reload --repo . -- diff main...feature -- src/ui",
        "workdeck session reload --repo . -- show HEAD~1",
        "workdeck session reload --repo . -- show HEAD~1 -- README.md",
        "workdeck session reload --repo /path/to/worktree -- diff",
        "workdeck session reload --session-path /path/to/live-window --source /path/to/other-checkout -- diff",
    ];
    commands.push(reload);

    let mut comment_add = spec(
        SessionDaemonAction::CommentAdd,
        "session comment add",
        "attach one live inline review note",
        vec![positional("[sessionId]", None)],
        vec![
            required(diff_file_option()),
            required(option("--summary <text>", "short review note")),
            repo_option(),
            old_line_option(),
            new_line_option(),
            option("--rationale <text>", "optional longer explanation"),
            option(
                "--markup <stml>",
                "experimental STML body (target session must opt in)",
            ),
            option("--author <name>", "optional author label"),
            option("--focus", "add the note and focus the viewport on it"),
            json_option(),
        ],
        vec![
            "workdeck session comment add (<session-id> | --repo <path>) --file <path> (--old-line <n> | --new-line <n>) --summary <text> [--rationale <text>] [--author <name>] [--markup <stml>] [--focus] [--json]",
        ],
    );
    comment_add.constraints = vec![COMMENT_TARGET_CONSTRAINT];
    comment_add.examples = vec![
        "workdeck session comment add --repo . --file README.md --new-line 103 --summary \"Tighten this wording\"",
    ];
    commands.push(comment_add);

    let mut comment_apply = spec(
        SessionDaemonAction::CommentApply,
        "session comment apply",
        "apply many live inline review notes from stdin JSON",
        vec![positional("[sessionId]", None)],
        vec![
            repo_option(),
            option("--stdin", "read the comment batch from stdin as JSON"),
            option("--focus", "apply the batch and focus the first note"),
            json_option(),
        ],
        vec![
            "workdeck session comment apply (<session-id> | --repo <path>) --stdin [--focus] [--json]",
        ],
    );
    comment_apply.examples = vec![
        "printf '%s\\n' '{\"comments\":[{\"filePath\":\"README.md\",\"newLine\":103,\"summary\":\"Tighten this wording\"}]}' | workdeck session comment apply --repo . --stdin",
    ];
    comment_apply.help_extra = vec![
        "Stdin JSON shape:",
        "  {",
        "    \"comments\": [",
        "      {",
        "        \"filePath\": \"README.md\",",
        "        \"hunk\": 2,",
        "        \"summary\": \"Explain this hunk\",",
        "        \"rationale\": \"Optional detail\",",
        "        \"author\": \"Pi\"",
        "      }",
        "    ]",
        "  }",
    ];
    commands.push(comment_apply);

    commands.push(spec(
        SessionDaemonAction::CommentList,
        "session comment list",
        "list live inline review notes",
        vec![positional("[sessionId]", None)],
        vec![
            repo_option(),
            option("--file <path>", "filter comments to one diff file"),
            option(
                "--type <type>",
                "filter to live, all, ai, agent, or user comments",
            ),
            json_option(),
        ],
        vec![
            "workdeck session comment list (<session-id> | --repo <path>) [--file <path>] [--type <live|all|ai|agent|user>] [--json]",
        ],
    ));
    commands.push(spec(
        SessionDaemonAction::CommentRm,
        "session comment rm",
        "remove one inline review note",
        vec![positional(
            "[targets...]",
            Some("<session-id> <comment-id>, or <comment-id> with --repo"),
        )],
        vec![repo_option(), json_option()],
        vec!["workdeck session comment rm (<session-id> | --repo <path>) <comment-id> [--json]"],
    ));
    commands.push(spec(
        SessionDaemonAction::CommentClear,
        "session comment clear",
        "clear inline review notes",
        vec![positional("[sessionId]", None)],
        vec![
            repo_option(),
            option("--file <path>", "clear only one diff file's comments"),
            option(
                "--include-user",
                "also clear human notes created with the TUI `c` action",
            ),
            option(
                "--all",
                "clear both live agent comments and human user notes",
            ),
            option("--yes", "confirm destructive comment clearing"),
            json_option(),
        ],
        vec![
            "workdeck session comment clear (<session-id> | --repo <path>) [--file <path>] [--include-user|--all] --yes [--json]",
        ],
    ));

    let mut highlight_add = spec(
        SessionDaemonAction::HighlightAdd,
        "session highlight add",
        "paint one attention mark inside a diff line",
        vec![positional("[sessionId]", None)],
        vec![
            required(diff_file_option()),
            required(parsed_option(
                "--start <n>",
                "0-based inclusive start offset into the line's text (UTF-16 code units)",
                AgentOptionParser::NonNegativeInt,
            )),
            required(parsed_option(
                "--end <n>",
                "exclusive end offset; must be greater than --start",
                AgentOptionParser::PositiveInt,
            )),
            repo_option(),
            old_line_option(),
            new_line_option(),
            option(
                "--tone <tone>",
                "mark tone: match, current, info, warning, error, dim (default match)",
            ),
            option("--focus", "add the mark and land the viewport on its line"),
            json_option(),
        ],
        vec![
            "workdeck session highlight add (<session-id> | --repo <path>) --file <path> (--old-line <n> | --new-line <n>) --start <n> --end <n> [--tone <tone>] [--focus] [--json]",
        ],
    );
    highlight_add.constraints = vec![HIGHLIGHT_TARGET_CONSTRAINT];
    highlight_add.examples = vec![
        "workdeck session highlight add --repo . --file src/App.tsx --new-line 42 --start 6 --end 19",
        "workdeck session highlight add --repo . --file src/App.tsx --new-line 42 --start 6 --end 19 --tone warning --focus",
    ];
    commands.push(highlight_add);

    let mut highlight_clear = spec(
        SessionDaemonAction::HighlightClear,
        "session highlight clear",
        "clear agent attention marks",
        vec![positional("[sessionId]", None)],
        vec![
            repo_option(),
            option("--file <path>", "clear only one diff file's marks"),
            json_option(),
        ],
        vec![
            "workdeck session highlight clear (<session-id> | --repo <path>) [--file <path>] [--json]",
        ],
    );
    highlight_clear.examples = vec!["workdeck session highlight clear --repo ."];
    commands.push(highlight_clear);
    commands
});

pub static SESSION_COMMENT_COMMAND_LIST: LazyLock<Vec<AgentCommandSpec>> = LazyLock::new(|| {
    SESSION_AGENT_COMMAND_LIST
        .iter()
        .filter(|spec| spec.name.starts_with("session comment "))
        .cloned()
        .collect()
});

pub static SESSION_HIGHLIGHT_COMMAND_LIST: LazyLock<Vec<AgentCommandSpec>> = LazyLock::new(|| {
    SESSION_AGENT_COMMAND_LIST
        .iter()
        .filter(|spec| spec.name.starts_with("session highlight "))
        .cloned()
        .collect()
});

#[must_use]
pub fn session_agent_command(action: SessionDaemonAction) -> &'static AgentCommandSpec {
    SESSION_AGENT_COMMAND_LIST
        .iter()
        .find(|spec| spec.action == action)
        .expect("every session daemon action has an agent command")
}

#[cfg(test)]
fn referenced_flags(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut flags = Vec::new();
    let mut cursor = 0;
    while cursor + 2 < bytes.len() {
        let Some(relative) = text[cursor..].find("--") else {
            break;
        };
        let start = cursor + relative;
        if !bytes[start + 2].is_ascii_lowercase() {
            cursor = start + 2;
            continue;
        }
        let mut end = start + 3;
        while end < bytes.len() && (bytes[end].is_ascii_lowercase() || bytes[end] == b'-') {
            end += 1;
        }
        flags.push(&text[start..end]);
        cursor = end;
    }
    flags
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn covers_every_daemon_action_with_a_unique_command() {
        assert_eq!(SESSION_AGENT_COMMAND_LIST.len(), 13);
        assert_eq!(
            SESSION_AGENT_COMMAND_LIST
                .iter()
                .map(|spec| format!("{:?}", spec.action))
                .collect::<BTreeSet<_>>()
                .len(),
            13
        );
        assert_eq!(
            SESSION_AGENT_COMMAND_LIST
                .iter()
                .map(|spec| spec.name)
                .collect::<BTreeSet<_>>()
                .len(),
            13
        );
        assert_eq!(
            SESSION_COMMENT_COMMAND_LIST
                .iter()
                .map(|spec| spec.name)
                .collect::<Vec<_>>(),
            [
                "session comment add",
                "session comment apply",
                "session comment list",
                "session comment rm",
                "session comment clear",
            ]
        );
        assert_eq!(
            SESSION_HIGHLIGHT_COMMAND_LIST
                .iter()
                .map(|spec| spec.name)
                .collect::<Vec<_>>(),
            ["session highlight add", "session highlight clear"]
        );
    }

    #[test]
    fn each_option_flag_is_declared_once_per_command() {
        for spec in SESSION_AGENT_COMMAND_LIST.iter() {
            let names = spec
                .options
                .iter()
                .map(agent_option_flag_name)
                .collect::<Vec<_>>();
            assert_eq!(names.iter().collect::<BTreeSet<_>>().len(), names.len());
        }
    }

    #[test]
    fn every_declared_option_appears_in_the_synopsis() {
        for spec in SESSION_AGENT_COMMAND_LIST.iter() {
            let synopsis = spec.synopsis.join(" ");
            for option in &spec.options {
                assert!(
                    synopsis.contains(agent_option_flag_name(option)),
                    "{}",
                    spec.name
                );
            }
        }
    }

    #[test]
    fn synopsis_and_examples_only_reference_declared_flags() {
        for spec in SESSION_AGENT_COMMAND_LIST.iter() {
            let declared = spec
                .options
                .iter()
                .map(agent_option_flag_name)
                .collect::<BTreeSet<_>>();
            for line in spec.synopsis.iter().chain(&spec.examples) {
                for flag in referenced_flags(line) {
                    assert!(declared.contains(flag), "{} references {flag}", spec.name);
                }
            }
        }
    }

    #[test]
    fn synopsis_lines_are_runnable_workdeck_invocations() {
        for spec in SESSION_AGENT_COMMAND_LIST.iter() {
            for line in &spec.synopsis {
                assert!(line.starts_with(&format!("workdeck {}", spec.name)));
            }
        }
    }

    #[test]
    fn camelizes_flag_definitions_into_option_keys() {
        assert_eq!(option_key_from_flag("--old-line <n>"), "oldLine");
        assert_eq!(option_key_from_flag("--next-comment"), "nextComment");
        assert_eq!(option_key_from_flag("--json"), "json");
    }

    #[test]
    fn every_constraint_flag_is_a_declared_option() {
        for spec in SESSION_AGENT_COMMAND_LIST.iter() {
            let declared = spec
                .options
                .iter()
                .map(agent_option_flag_name)
                .collect::<BTreeSet<_>>();
            for constraint in &spec.constraints {
                for flag in constraint.flags {
                    assert!(declared.contains(flag.split(' ').next().unwrap()));
                }
            }
        }
    }

    #[test]
    fn required_and_parsed_option_shapes_are_manifest_derived() {
        let add = session_agent_command(SessionDaemonAction::CommentAdd);
        let required = add
            .options
            .iter()
            .filter(|option| option.required)
            .map(agent_option_flag_name)
            .collect::<BTreeSet<_>>();
        assert_eq!(required, BTreeSet::from(["--file", "--summary"]));
        let navigate = session_agent_command(SessionDaemonAction::Navigate);
        for option in &navigate.options {
            if ["--hunk", "--old-line", "--new-line"].contains(&agent_option_flag_name(option)) {
                assert_eq!(option.parse, Some(AgentOptionParser::PositiveInt));
            }
        }
    }

    #[test]
    fn highlight_offsets_have_zero_based_start_and_positive_end() {
        let highlight = session_agent_command(SessionDaemonAction::HighlightAdd);
        let parser = |name| {
            highlight
                .options
                .iter()
                .find(|option| agent_option_flag_name(option) == name)
                .and_then(|option| option.parse)
        };
        assert_eq!(parser("--start"), Some(AgentOptionParser::NonNegativeInt));
        assert_eq!(parser("--end"), Some(AgentOptionParser::PositiveInt));
    }

    #[test]
    fn recognizes_exactly_the_six_shared_highlight_tones() {
        for tone in HIGHLIGHT_TONES {
            assert!(is_highlight_tone(tone));
        }
        assert!(!is_highlight_tone("loud"));
    }
}
