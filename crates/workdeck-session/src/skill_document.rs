//! Deterministic renderer for the bundled `workdeck-review` agent skill.
//!
//! Command synopses, examples, and common errors come from the same typed
//! session surface used by the native command runner. Keep narrative guidance
//! here and regenerate the checked-in artifact with `cargo xtask skill generate`.

use crate::{AGENT_ERROR_DOCS, AgentCommandSpec, SessionDaemonAction, session_agent_command};

fn bash_fence<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    let mut block = String::from("```bash\n");
    for line in lines {
        block.push_str(line);
        block.push('\n');
    }
    block.push_str("```");
    block
}

fn push_section(document: &mut String, section: &str) {
    if !document.is_empty() {
        document.push_str("\n\n");
    }
    document.push_str(section.trim_matches('\n'));
}

fn synopses(specs: &[&AgentCommandSpec]) -> String {
    bash_fence(specs.iter().flat_map(|spec| spec.synopsis.iter().copied()))
}

fn examples(specs: &[&AgentCommandSpec]) -> String {
    bash_fence(specs.iter().flat_map(|spec| spec.examples.iter().copied()))
}

fn navigate_examples(spec: &AgentCommandSpec, predicate: impl Fn(&str) -> bool) -> String {
    bash_fence(
        spec.examples
            .iter()
            .copied()
            .filter(|example| predicate(example)),
    )
}

/// Render the complete bundled review skill from the native session contract.
#[must_use]
pub fn render_workdeck_review_skill() -> String {
    let list = session_agent_command(SessionDaemonAction::List);
    let get = session_agent_command(SessionDaemonAction::Get);
    let context = session_agent_command(SessionDaemonAction::Context);
    let review = session_agent_command(SessionDaemonAction::Review);
    let navigate = session_agent_command(SessionDaemonAction::Navigate);
    let reload = session_agent_command(SessionDaemonAction::Reload);
    let comment_add = session_agent_command(SessionDaemonAction::CommentAdd);
    let comment_apply = session_agent_command(SessionDaemonAction::CommentApply);
    let comment_list = session_agent_command(SessionDaemonAction::CommentList);
    let comment_rm = session_agent_command(SessionDaemonAction::CommentRm);
    let comment_clear = session_agent_command(SessionDaemonAction::CommentClear);
    let highlight_add = session_agent_command(SessionDaemonAction::HighlightAdd);
    let highlight_clear = session_agent_command(SessionDaemonAction::HighlightClear);

    let mut document = String::new();
    push_section(
        &mut document,
        r#"---
name: workdeck-review
description: Interacts with live Workdeck diff review sessions via CLI. Inspects review focus, navigates files, hunks, and exact lines, reloads session contents, adds inline review comments, and paints attention marks on character ranges. Use when the user has a Workdeck session running or wants to review diffs interactively.
---

# Workdeck Review

Workdeck is an interactive terminal diff reviewer. The TUI is for the user -- do not run `workdeck diff`, `workdeck show`, or other interactive commands directly from an agent. Use `workdeck session *` CLI commands to inspect and control live review sessions through the authenticated local daemon.

If no session exists, ask the user to launch Workdeck in their terminal first. Workdeck owns review state and inline notes; Herder owns agents, PTYs, and process lifecycle."#,
    );
    push_section(
        &mut document,
        r#"## Workflow

```text
1. workdeck session list                                    # find live sessions
2. workdeck session get --repo .                            # inspect path / repo / source
3. workdeck session review --repo . --json                  # inspect file/hunk structure first
4. workdeck session review --repo . --include-patch --json  # opt into raw diff text only when needed
5. workdeck session context --repo .                        # check current focus when needed
6. workdeck session navigate ...                            # move to the right place
7. workdeck session reload -- <command>                     # swap contents if needed
8. workdeck session comment add ...                         # leave one review note
9. workdeck session comment apply ...                       # apply many agent notes in one stdin batch
10. workdeck session highlight add ...                      # light up the exact range you are explaining
```"#,
    );
    push_section(
        &mut document,
        r#"## Session selection

Most session commands accept:

- `--repo <path>` -- match the live session by its current loaded repo root (most common)
- `<session-id>` -- match by exact ID (use when multiple sessions share a repo)
- If only one session exists, it auto-resolves

`reload` also supports:

- `--session-path <path>` -- match the live Workdeck window by its current working directory
- `--source <path>` -- load the replacement `diff` / `show` command from a different directory

Use `--source` only for advanced reloads where the live session you want to control is not already associated with the checkout you want to load next. For a normal worktree session, prefer selecting it directly with `--repo /path/to/worktree`."#,
    );
    push_section(&mut document, "## Commands");

    push_section(
        &mut document,
        &format!(
            r#"### Inspect

{}

- `get` shows the session `Path`, `Repo`, and `Source`, which helps when choosing between `--repo` and `--session-path`
- `Repo` is what `--repo` matches; `Path` is what `--session-path` matches
- `review --json` returns file and hunk structure by default; add `--include-patch` only when a caller truly needs raw unified diff text
- `review --include-notes` also returns the live review notes alongside the file and hunk structure"#,
            synopses(&[list, get, context, review])
        ),
    );

    push_section(
        &mut document,
        &format!(
            r#"### Navigate

{}

Absolute navigation requires `--file` and exactly one of `--hunk`, `--new-line`, or `--old-line`:

{}

Exact comment navigation uses the `commentId` returned by `workdeck session comment list --json` and does not require `--file`:

{}

Relative comment navigation jumps between annotated hunks and does not require `--file`:

{}

- `--hunk <n>` is 1-based
- `--new-line` / `--old-line` are 1-based line numbers on that diff side
- A line target lands the user's viewport on that exact line (falling back to its hunk when the line is inside a collapsed region); `--hunk` lands on the hunk
- Use either `--next-comment` or `--prev-comment`, not both"#,
            synopses(&[navigate]),
            navigate_examples(navigate, |example| example.contains("--file")),
            navigate_examples(navigate, |example| example.contains("--comment ")),
            navigate_examples(navigate, |example| {
                example.contains("--next-comment") || example.contains("--prev-comment")
            })
        ),
    );

    push_section(
        &mut document,
        &format!(
            r#"### Reload

Swaps the live session's contents. Pass a Workdeck review command after `--`:

{}

Examples:

{}

- Always include `--` before the nested Workdeck command
- `--repo` or `<session-id>` usually selects the session you want
- `--source` is advanced: it does not select the session; it only changes where the replacement review command runs
- If the live session is already showing the target worktree, prefer `workdeck session reload --repo /path/to/worktree -- diff`
- `--session-path` targets the live window when you need to keep session selection separate from reload source"#,
            synopses(&[reload]),
            examples(&[reload])
        ),
    );

    push_section(
        &mut document,
        &format!(
            r#"### Comments

{}

Examples:

{}

- `comment list --type user` shows human-authored inline notes; without `--type`, `comment list` preserves the legacy live-agent-comment view
- `comment add` is best for one note; `comment apply` is best when an agent already has several notes ready
- `comment add` requires `--file`, `--summary`, and exactly one of `--old-line` or `--new-line`
- `comment apply` payload items require `filePath`, `summary`, and exactly one target such as `hunk`, `hunkNumber`, `oldLine`, or `newLine`
- `comment apply` reads a JSON batch from stdin and validates the full batch before mutating the live session
- Pass `--focus` when you want to jump to the new note or the first note in a batch
- `comment list` and `comment clear` accept optional `--file`
- Quote `--summary` and `--rationale` defensively in the shell"#,
            synopses(&[
                comment_add,
                comment_apply,
                comment_list,
                comment_rm,
                comment_clear,
            ]),
            examples(&[comment_add, comment_apply])
        ),
    );

    push_section(
        &mut document,
        &format!(
            r#"### Attention marks

Highlights paint character ranges inside the diff lines the user is looking at -- use them to light up the exact expression you are explaining while you narrate.

{}

Examples:

{}

- `highlight add` requires `--file`, exactly one of `--old-line` or `--new-line`, and the `--start` / `--end` offsets
- `--start` is a 0-based inclusive offset into the line's text and `--end` is exclusive, counted in UTF-16 code units -- the same `[start, end)` range extensions use
- Tones: `match` (default), `info`, `warning`, `error`, `dim`; `current` renders as reverse video and is best reserved for the one range under discussion
- Pass `--focus` to also land the viewport on the marked line
- Marks survive scrolling, navigation, and reloads that leave the marked file's content unchanged; a reload that changes that file drops its marks, and `highlight clear` removes them explicitly (optionally per `--file`)
- Marks are visual only -- pair them with a `comment add` when the explanation should persist as a note"#,
            synopses(&[highlight_add, highlight_clear]),
            examples(&[highlight_add, highlight_clear])
        ),
    );

    push_section(
        &mut document,
        r#"### Experimental rich markup notes (STML)

Only use STML when `workdeck session context --json` lists `stml` in `experimentalFeatures`. The user opts into that experience by launching the review with `--experimental`; do not ask a normal session to render markup.

For an opted-in session, `--markup` (or a `markup` field on apply items) renders the note body as STML -- a small HTML-like markup for terminal UI (boxes, rows, gauges, badges, lists, code). Keep `--summary` a real sentence: it is the fallback and the `comment list` text.

Before writing markup, run `workdeck markup guide` once -- it has copy-paste patterns and the width rules. The session context also reports `noteMarkupWidth` (the live render width); preview with `workdeck markup render - --width <that>`. Comment responses echo `markupWidth` and return `markupNotes` when markup degraded -- fix what they flag."#,
    );

    push_section(
        &mut document,
        &format!(
            r#"## New files in working-tree reviews

`workdeck diff` includes untracked files by default. If the user wants tracked changes only, reload with `--exclude-untracked`:

{}"#,
            bash_fence(["workdeck session reload --repo . -- diff --exclude-untracked"])
        ),
    );

    push_section(
        &mut document,
        r#"## Guiding a review

The user may ask you to walk them through a changeset or review code using Workdeck. Start with `workdeck session review --json` to understand the file/hunk structure without inflating agent context, then use `--include-patch` only for the files you truly need to read in raw diff form. Use `context` and `navigate` to line up the user's current view before adding comments.

Your role is to narrate: steer the user's view to what matters and leave comments that explain what they are looking at.

Typical flow:

1. Load the right content (`reload` if needed)
2. Navigate to the first interesting file / hunk
3. Add a comment explaining what is happening and why
4. If you already have several notes ready, prefer one `comment apply` batch over many separate shell invocations
5. Summarize when done

Guidelines:

- Work in the order that tells the clearest story, not necessarily file order
- Navigate before commenting so the user sees the code you are discussing
- Use `highlight add --focus` to steer the user's eyes to the exact expression while you explain it, and `highlight clear` before moving to the next topic
- Use `comment apply` for agent-generated batches and `comment add` for one-off notes
- Use `--focus` sparingly when the note itself should actively steer the review
- Keep comments focused: intent, structure, risks, or follow-ups
- Do not comment on every hunk -- highlight what the user would not spot themselves"#,
    );

    let mut errors = String::from("## Common errors\n");
    for error in AGENT_ERROR_DOCS {
        errors.push_str(&format!("\n- **\"{}\"** -- {}", error.quote, error.remedy));
    }
    push_section(&mut document, &errors);
    document.push('\n');
    document
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::{AUXILIARY_AGENT_OPTIONS, SESSION_AGENT_COMMAND_LIST, agent_option_flag_name};

    fn workspace_file(relative: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

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

    fn documented_flags() -> BTreeSet<&'static str> {
        SESSION_AGENT_COMMAND_LIST
            .iter()
            .flat_map(|spec| spec.options.iter().map(agent_option_flag_name))
            .chain(
                AUXILIARY_AGENT_OPTIONS
                    .iter()
                    .map(|(_, option)| agent_option_flag_name(option)),
            )
            .collect()
    }

    #[test]
    fn checked_in_skill_matches_the_generated_document() {
        let checked_in = fs::read_to_string(workspace_file("skills/workdeck-review/SKILL.md"))
            .expect("read checked-in review skill")
            .replace("\r\n", "\n");
        assert_eq!(checked_in, render_workdeck_review_skill());
    }

    #[test]
    fn generated_skill_only_mentions_declared_agent_flags() {
        let rendered = render_workdeck_review_skill();
        let mentioned = referenced_flags(&rendered);
        assert!(!mentioned.is_empty());
        let documented = documented_flags();
        for flag in mentioned {
            assert!(documented.contains(flag), "undeclared flag {flag}");
        }
    }

    #[test]
    fn agent_workflow_guide_only_mentions_declared_agent_flags() {
        let guide = fs::read_to_string(workspace_file("docs/agent-workflows.md"))
            .expect("read agent workflow guide");
        let mentioned = referenced_flags(&guide);
        assert!(!mentioned.is_empty());
        let documented = documented_flags();
        for flag in mentioned {
            assert!(documented.contains(flag), "undeclared flag {flag}");
        }
    }

    #[test]
    fn generated_skill_documents_every_session_command_synopsis() {
        let rendered = render_workdeck_review_skill();
        for spec in SESSION_AGENT_COMMAND_LIST.iter() {
            for synopsis in &spec.synopsis {
                assert!(rendered.contains(synopsis), "missing {synopsis}");
            }
        }
    }

    #[test]
    fn generated_skill_has_workdeck_branding_and_product_boundary() {
        let rendered = render_workdeck_review_skill();
        assert!(rendered.starts_with("---\nname: workdeck-review\n"));
        assert!(rendered.contains("Herder owns agents, PTYs, and process lifecycle"));
        assert!(!rendered.contains("`hunk "));
    }
}
