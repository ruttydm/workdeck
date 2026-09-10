+++
title = "Review with an agent"
description = "Let an agent inspect and guide a human-owned live review."
template = "docs.html"
+++

The Workdeck window stays with you. Your agent uses non-interactive `workdeck session` commands from another terminal to inspect the same review, navigate it, and leave inline notes.

## Start the review

```bash
workdeck diff
```

Keep that window open. Normal Workdeck sessions register with a local loopback daemon so the session CLI can find them.

## Give the agent the skill

In the agent's shell, locate the skill bundled with the installed Workdeck version:

```bash
workdeck skill path
```

Ask the agent to load that file and use it for the review. A portable prompt is:

```text
Load the Workdeck skill and use it for this review. Run `workdeck skill path` to get the skill path.
```

The skill tells agents not to launch the interactive TUI themselves. It teaches them to use the session surface instead.

## What the agent does

A typical agent flow is:

```bash
workdeck session list
workdeck session get --repo .
workdeck session review --repo . --json
workdeck session navigate --repo . --file src/main.rs --hunk 2
workdeck session comment add --repo . --file src/main.rs --new-line 42 --summary "Check this boundary"
```

`review --json` exposes structure without forcing the full patch into agent context. The agent should request `--include-patch` only when it actually needs raw unified diff text.

The upstream agent-rationale screenshot still needs its native migration and
visual verification. It is not presented here as a Workdeck capture.

Agent notes remain spatially attached to the code they explain. Use `{` and `}` to move between annotated hunks while keeping the full changeset visible.

## Give the agent the docs

`cargo xtask site build` generates plain Markdown for agents alongside the HTML.
The following paths are build artifacts; public deployment is not yet verified:

- [/llms.txt](/llms.txt) — index of every page, for pulling only what is needed.
- [/llms-small.txt](/llms-small.txt) — compact corpus for tight context budgets.
- [/llms-full.txt](/llms-full.txt) — the currently migrated docs in one file (not the complete upstream corpus).

For migrated pages, replace the trailing slash with `.md`: for example,
`/docs/agents/live-session-control.md`. The native CLI reference page and full
corpus are still being migrated. Development-server export refresh is pending;
use the built output when checking these artifacts.

## Keep control

The agent can guide the visible selection and add agent-authored notes, but you remain in the review stream and can navigate normally. Ask it to summarize when finished, then use `{` and `}` to walk annotated hunks.

Workdeck's session controls do not own agent processes or PTYs; Herder remains
responsible for those. Adapted from Hunk's pinned MIT guide, Copyright Modem Labs
Inc. The source interval remains unmapped until its missing visual and complete
migration evidence are accounted for.

