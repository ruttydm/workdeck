# Agent workflows

Use Workdeck with external agents in two ways:

- **Recommended:** steer a live Workdeck review from another terminal with `workdeck session ...`.
- **Alternative:** load prewritten agent notes from a file with `--agent-context`.

Workdeck owns repository review state and inline notes. Herder owns agents, PTYs, and process
lifecycle; a Workdeck review session must not be treated as an agent process.

## Recommended workflow: steer a live Workdeck window

1. Open Workdeck in one terminal with a normal review command such as `workdeck diff` or
   `workdeck show`.
2. Load the Workdeck review skill from `workdeck skill path workdeck-review`.
3. Ask the agent to use the skill and review the current session.

A good generic prompt is:

```text
Load the Workdeck review skill and use it for this review. Run
`workdeck skill path workdeck-review` to get the skill path.
```

That skill teaches the agent how to inspect a live review session, navigate it, reload it, and
leave inline comments.

## How live session control works

When a Workdeck TUI starts, it registers with an authenticated local loopback daemon.
`workdeck session ...` talks to that daemon to discover the right live window and control it. The
TUI manages daemon discovery and lifecycle automatically.

If `workdeck session list` reports no sessions while Workdeck is visibly running, the agent
sandbox may be blocking loopback access. Rerun `workdeck session list --json` with the agent's
network or sandbox escalation. Do not probe the session API with raw HTTP: session controls use an
automatically discovered, owner-private caller credential and signed responses, and Workdeck
intentionally exposes no credential flags.

## The commands you will use most

### Inspect the current review

Start here before navigating or commenting:

```bash
workdeck session list
workdeck session get --repo .
workdeck session review --repo . --json
```

- `list` shows the active Workdeck windows.
- `get --repo .` confirms which live session matches the current repository.
- `review --json` returns the loaded file and hunk structure without dumping the full raw patch.

Only add `--include-patch` when an agent truly needs raw unified diff text:

```bash
workdeck session review --repo . --include-patch --json
```

### Move the live window to the right place

Use `navigate` to jump to the file or hunk you want the user to see:

```bash
workdeck session navigate --repo . --file src/App.tsx --hunk 2
workdeck session navigate --repo . --next-comment
```

To jump to an exact comment returned by the JSON list, copy its `commentId` into `--comment`:

```bash
workdeck session comment list --repo . --json
workdeck session navigate --repo . --comment <comment-id> --json
```

Use `reload` when you want the already-open window to show a different diff or commit:

```bash
workdeck session reload --repo . -- diff
workdeck session reload --repo . -- show HEAD~1 -- README.md
```

Notes:

- Always include `--` before the nested Workdeck command in `reload`.
- `--hunk` is 1-based.
- `--next-comment` and `--prev-comment` are useful when an agent walks the user through notes.

### Add comments

For one note, use `comment add`:

```bash
workdeck session comment add --repo . --file README.md --new-line 103 --summary "Tighten this wording"
```

For multiple notes, use one stdin batch with `comment apply`:

```bash
printf '%s\n' '{"comments":[{"filePath":"README.md","newLine":103,"summary":"Tighten this wording"}]}' \
  | workdeck session comment apply --repo . --stdin
```

`comment apply` payload items need:

- `filePath`
- `summary`
- Exactly one target such as `hunk`, `hunkNumber`, `oldLine`, or `newLine`.

If you want the UI to jump to the new note, add `--focus` to `comment add` or `comment apply`.

For comment cleanup and inspection, use:

```bash
workdeck session comment list --repo .
workdeck session comment rm --repo . <comment-id>
workdeck session comment clear --repo . --file README.md --yes
workdeck session comment clear --repo . --all --yes
```

The last command also clears human notes created by the TUI. Agents can remove or bulk-clear human
notes for cleanup, but cannot create or edit them through the session CLI.

## Session targeting

Most commands can target the live session in a few ways:

- `--repo <path>`: most common; matches the live session by its current repository root.
- `<session-id>`: useful when multiple Workdeck windows are open for the same repository.
- If only one session exists, Workdeck can auto-resolve it.

`reload` also supports advanced selectors:

- `--session-path <path>` targets the live Workdeck window by its current working directory.
- `--source <path>` changes where the replacement `diff` or `show` command runs.

For normal worktree use, prefer `--repo /path/to/worktree`. Reach for `--session-path` and
`--source` only when you need to repoint an already-open window to another checkout or path.

## Alternative workflow: load agent comments from a file

Use `--agent-context` when you already have agent-written rationale or notes in a JSON sidecar and
want to render them beside the diff.

```bash
workdeck diff --agent-context notes.json
workdeck patch change.patch --agent-context notes.json
```

For a compact example, see
[`examples/3-agent-review-demo/agent-context.json`](../examples/3-agent-review-demo/agent-context.json).

## Opt into experimental rich notes

STML note bodies are experimental and disabled by default. Start a new review with
`--experimental` to render sidecar `markup` fields and accept live comments that carry markup:

```bash
workdeck diff --experimental --agent-context notes.json
```

Normal reviews keep using each annotation's required plain-text `summary` fallback. Opted-in live
sessions list `stml` in `workdeck session context --json` under `experimentalFeatures`; reload
commands cannot change the launch opt-in.

## Practical defaults

- Start with `workdeck session review --repo . --json`.
- Only add `--include-patch` when the raw patch is actually needed.
- Use `comment add` for one-off notes and `comment apply` for batches.
- Prefer `--repo` over `--session-path` unless you have a specific advanced reload case.
