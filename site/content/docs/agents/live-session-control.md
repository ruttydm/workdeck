+++
title = "Live session control"
description = "Inspect, target, navigate and reload native review sessions."
template = "docs.html"
+++

Each normal Workdeck TUI registers with one loopback daemon. `workdeck session ...` finds a registered window and sends it review actions.

## Find the session

```bash
workdeck session list
workdeck session get --repo .
workdeck session context --repo .
```

Use `--repo <path>` for normal worktrees. Use an explicit session ID when multiple windows share a repository.

## Inspect without overloading context

```bash
workdeck session review --repo . --json
```

This returns files and hunks. Add flags only when required:

```bash
workdeck session review --repo . --include-notes --json
workdeck session review --repo . --include-patch --json
```

## Navigate the visible window

```bash
workdeck session navigate --repo . --file src/main.rs --hunk 2
workdeck session navigate --repo . --file src/main.rs --new-line 372
workdeck session navigate --repo . --next-comment
```

Hunk numbers are 1-based. Absolute navigation needs a file and exactly one hunk, old-line, or new-line target.

## Reload the review

Always place `--` before the nested Workdeck command:

```bash
workdeck session reload --repo . -- diff
workdeck session reload --repo . -- show HEAD~1 -- README.md
```

Advanced reloads can target the live window by `--session-path` and load from a separate `--source` directory. Prefer `--repo` until those roles genuinely need to differ.

## Diagnose local access

If a visible Workdeck window does not appear in `session list`, an agent sandbox may block loopback access. Workdeck's daemon is intentionally local-only; retry with the agent's network/sandbox permission rather than exposing it remotely. `workdeck daemon serve` is available for manual startup or daemon debugging.

Session commands control the review, not agent processes or PTYs; those remain
Herder's responsibility. Opening this guide does not launch a daemon or mutate
a live review. Navigation and reload examples change the selected live session
when you explicitly run them.

Adapted from Hunk's pinned MIT guide, Copyright Modem Labs Inc. Native CLI help
matches these command forms; complete broker, lifecycle and platform parity is
still a release gate. This source interval remains unmapped.

