---
name: workdeck-review
description: Interacts with live Workdeck diff review sessions via CLI. Inspects review focus, navigates files, hunks, and exact lines, reloads session contents, adds inline review comments, and paints attention marks on character ranges. Use when the user has a Workdeck session running or wants to review diffs interactively.
---

# Workdeck Review

Workdeck is an interactive terminal diff reviewer. The TUI is for the user -- do not run `workdeck diff`, `workdeck show`, or other interactive commands directly from an agent. Use `workdeck session *` CLI commands to inspect and control live review sessions through the authenticated local daemon.

If no session exists, ask the user to launch Workdeck in their terminal first. Workdeck owns review state and inline notes; Herder owns agents, PTYs, and process lifecycle.

## Workflow

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
```

## Session selection

Most session commands accept:

- `--repo <path>` -- match the live session by its current loaded repo root (most common)
- `<session-id>` -- match by exact ID (use when multiple sessions share a repo)
- If only one session exists, it auto-resolves

`reload` also supports:

- `--session-path <path>` -- match the live Workdeck window by its current working directory
- `--source <path>` -- load the replacement `diff` / `show` command from a different directory

Use `--source` only for advanced reloads where the live session you want to control is not already associated with the checkout you want to load next. For a normal worktree session, prefer selecting it directly with `--repo /path/to/worktree`.

## Commands

### Inspect

```bash
workdeck session list [--json]
workdeck session get (<session-id> | --repo <path>) [--json]
workdeck session context (<session-id> | --repo <path>) [--json]
workdeck session review (<session-id> | --repo <path>) [--include-patch] [--include-notes] [--json]
```

- `get` shows the session `Path`, `Repo`, and `Source`, which helps when choosing between `--repo` and `--session-path`
- `Repo` is what `--repo` matches; `Path` is what `--session-path` matches
- `review --json` returns file and hunk structure by default; add `--include-patch` only when a caller truly needs raw unified diff text
- `review --include-notes` also returns the live review notes alongside the file and hunk structure

### Navigate

```bash
workdeck session navigate (<session-id> | --repo <path>) --file <path> (--hunk <n> | --old-line <n> | --new-line <n>) [--json]
workdeck session navigate (<session-id> | --repo <path>) --comment <id> [--json]
workdeck session navigate (<session-id> | --repo <path>) (--next-comment | --prev-comment) [--json]
```

Absolute navigation requires `--file` and exactly one of `--hunk`, `--new-line`, or `--old-line`:

```bash
workdeck session navigate --repo . --file src/App.tsx --hunk 2
workdeck session navigate --repo . --file src/App.tsx --new-line 372
workdeck session navigate --repo . --file src/App.tsx --old-line 355
```

Exact comment navigation uses the `commentId` returned by `workdeck session comment list --json` and does not require `--file`:

```bash
workdeck session navigate --repo . --comment comment-1
```

Relative comment navigation jumps between annotated hunks and does not require `--file`:

```bash
workdeck session navigate --repo . --next-comment
workdeck session navigate --repo . --prev-comment
```

- `--hunk <n>` is 1-based
- `--new-line` / `--old-line` are 1-based line numbers on that diff side
- A line target lands the user's viewport on that exact line (falling back to its hunk when the line is inside a collapsed region); `--hunk` lands on the hunk
- Use either `--next-comment` or `--prev-comment`, not both

### Reload

Swaps the live session's contents. Pass a Workdeck review command after `--`:

```bash
workdeck session reload (<session-id> | --repo <path> | --session-path <path>) [--source <path>] [--json] -- diff [ref] [-- <pathspec...>]
workdeck session reload (<session-id> | --repo <path> | --session-path <path>) [--source <path>] [--json] -- show [ref] [-- <pathspec...>]
```

Examples:

```bash
workdeck session reload --repo . -- diff
workdeck session reload --repo . -- diff main...feature -- src/ui
workdeck session reload --repo . -- show HEAD~1
workdeck session reload --repo . -- show HEAD~1 -- README.md
workdeck session reload --repo /path/to/worktree -- diff
workdeck session reload --session-path /path/to/live-window --source /path/to/other-checkout -- diff
```

- Always include `--` before the nested Workdeck command
- `--repo` or `<session-id>` usually selects the session you want
- `--source` is advanced: it does not select the session; it only changes where the replacement review command runs
- If the live session is already showing the target worktree, prefer `workdeck session reload --repo /path/to/worktree -- diff`
- `--session-path` targets the live window when you need to keep session selection separate from reload source

### Comments

```bash
workdeck session comment add (<session-id> | --repo <path>) --file <path> (--old-line <n> | --new-line <n>) --summary <text> [--rationale <text>] [--author <name>] [--markup <stml>] [--focus] [--json]
workdeck session comment apply (<session-id> | --repo <path>) --stdin [--focus] [--json]
workdeck session comment list (<session-id> | --repo <path>) [--file <path>] [--type <live|all|ai|agent|user>] [--json]
workdeck session comment rm (<session-id> | --repo <path>) <comment-id> [--json]
workdeck session comment clear (<session-id> | --repo <path>) [--file <path>] [--include-user|--all] --yes [--json]
```

Examples:

```bash
workdeck session comment add --repo . --file README.md --new-line 103 --summary "Tighten this wording"
printf '%s\n' '{"comments":[{"filePath":"README.md","newLine":103,"summary":"Tighten this wording"}]}' | workdeck session comment apply --repo . --stdin
```

- `comment list --type user` shows human-authored inline notes; without `--type`, `comment list` preserves the legacy live-agent-comment view
- `comment add` is best for one note; `comment apply` is best when an agent already has several notes ready
- `comment add` requires `--file`, `--summary`, and exactly one of `--old-line` or `--new-line`
- `comment apply` payload items require `filePath`, `summary`, and exactly one target such as `hunk`, `hunkNumber`, `oldLine`, or `newLine`
- `comment apply` reads a JSON batch from stdin and validates the full batch before mutating the live session
- Pass `--focus` when you want to jump to the new note or the first note in a batch
- `comment list` and `comment clear` accept optional `--file`
- Quote `--summary` and `--rationale` defensively in the shell

### Attention marks

Highlights paint character ranges inside the diff lines the user is looking at -- use them to light up the exact expression you are explaining while you narrate.

```bash
workdeck session highlight add (<session-id> | --repo <path>) --file <path> (--old-line <n> | --new-line <n>) --start <n> --end <n> [--tone <tone>] [--focus] [--json]
workdeck session highlight clear (<session-id> | --repo <path>) [--file <path>] [--json]
```

Examples:

```bash
workdeck session highlight add --repo . --file src/App.tsx --new-line 42 --start 6 --end 19
workdeck session highlight add --repo . --file src/App.tsx --new-line 42 --start 6 --end 19 --tone warning --focus
workdeck session highlight clear --repo .
```

- `highlight add` requires `--file`, exactly one of `--old-line` or `--new-line`, and the `--start` / `--end` offsets
- `--start` is a 0-based inclusive offset into the line's text and `--end` is exclusive, counted in UTF-16 code units -- the same `[start, end)` range extensions use
- Tones: `match` (default), `info`, `warning`, `error`, `dim`; `current` renders as reverse video and is best reserved for the one range under discussion
- Pass `--focus` to also land the viewport on the marked line
- Marks survive scrolling, navigation, and reloads that leave the marked file's content unchanged; a reload that changes that file drops its marks, and `highlight clear` removes them explicitly (optionally per `--file`)
- Marks are visual only -- pair them with a `comment add` when the explanation should persist as a note

### Experimental rich markup notes (STML)

Only use STML when `workdeck session context --json` lists `stml` in `experimentalFeatures`. The user opts into that experience by launching the review with `--experimental`; do not ask a normal session to render markup.

For an opted-in session, `--markup` (or a `markup` field on apply items) renders the note body as STML -- a small HTML-like markup for terminal UI (boxes, rows, gauges, badges, lists, code). Keep `--summary` a real sentence: it is the fallback and the `comment list` text.

Before writing markup, run `workdeck markup guide` once -- it has copy-paste patterns and the width rules. The session context also reports `noteMarkupWidth` (the live render width); preview with `workdeck markup render - --width <that>`. Comment responses echo `markupWidth` and return `markupNotes` when markup degraded -- fix what they flag.

## New files in working-tree reviews

`workdeck diff` includes untracked files by default. If the user wants tracked changes only, reload with `--exclude-untracked`:

```bash
workdeck session reload --repo . -- diff --exclude-untracked
```

## Guiding a review

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
- Do not comment on every hunk -- highlight what the user would not spot themselves

## Common errors

- **"No diff file matches ..."** -- the file is not in the loaded review. Check `context`, then `reload` if needed.
- **"No active Workdeck sessions"** -- if Workdeck is visibly running, localhost may be blocked by the agent sandbox; retry with network/sandbox escalation. Otherwise ask the user to open Workdeck.
- **"Multiple active sessions match"** -- pass `<session-id>` explicitly.
- **"No active session matches session path ..."** -- for advanced split-path reloads, verify the live window `Path` via `workdeck session get` or `list`, then use `--session-path`.
- **"Pass the replacement Workdeck command after `--`"** -- include `--` before the nested `diff` / `show` command.
- **"Pass --stdin to read batch comments from stdin JSON."** -- `comment apply` only reads its batch payload from stdin.
- **"Specify exactly one navigation target"** -- pick one of `--hunk`, `--old-line`, or `--new-line`.
- **"Specify exactly one comment target"** -- pass `comment add` one of `--old-line` or `--new-line`.
- **"Specify exactly one highlight target"** -- pass `highlight add` one of `--old-line` or `--new-line`.
- **"Highlight --end must be greater than --start"** -- offsets are `[start, end)` UTF-16 code units into the line text; end is exclusive.
- **"Specify either --next-comment or --prev-comment, not both."** -- choose one comment-navigation direction.
- **"Could not read the raw diff for ..."** -- the session reloaded or closed while `--include-patch` was reading it. Re-run `review`; drop `--include-patch` if you only need file and hunk structure.
