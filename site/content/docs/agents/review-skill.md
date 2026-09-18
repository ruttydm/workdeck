+++
title = "Workdeck review skill"
description = "Load versioned machine guidance for the live review protocol."
template = "docs.html"
+++

Workdeck generates a `workdeck-review` skill for native installations. It is the authoritative machine-facing workflow for session selection, efficient review inspection, navigation, reloads, and comments.

## Locate the installed skill

```bash
workdeck skill path
```

Load or symlink the returned file according to your coding agent's skill mechanism. Resolve the path again after upgrades so the guidance stays aligned with the installed CLI.

For agents that need a stable web-readable URL, use the [generated Workdeck review skill](/docs/workdeck-review-skill.md). The website artifact and installed skill are rendered by the same function; neither is a handwritten copy.

## Why it is generated

The checked-in `skills/workdeck-review/SKILL.md` is rendered from typed command metadata and agent error definitions in Workdeck's source. Parser help, examples, constraints, and common remedies therefore share ownership instead of drifting as separate handwritten copies.

Do not edit the generated skill directly. Contributors change `crates/workdeck-session/src/skill_document.rs`, `crates/workdeck-session/src/agent_surface.rs`, or `crates/workdeck-session/src/agent_errors.rs`, then run:

```bash
cargo xtask skill generate
cargo xtask skill check
```

## Use it safely

The skill instructs agents to avoid launching interactive commands such as `workdeck diff` themselves. The user owns the TUI; the agent talks to an already-live review through `workdeck session *`.

Keep a human-owned `workdeck diff` session open while the agent uses the session
CLI. See [Review with an agent](/docs/agents/review-with-an-agent/). Workdeck session
control does not take over Herder's agent-process or PTY ownership.

Adapted from Hunk's pinned MIT guide, Copyright Modem Labs Inc. The website
artifact is generated in the repository; deployment and all-platform installation
qualification remain separate release gates. This source interval stays unmapped.
