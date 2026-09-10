+++
title = "Troubleshooting"
description = "Diagnose input, session, terminal and configuration problems."
template = "docs.html"
+++

## Workdeck shows no changes

Confirm the VCS and input first:

```bash
git status --short
workdeck diff --help
```

A Git working-tree review includes untracked files by default. Pager input does not, because Git decides what enters the pipe. Check `vcs` when a directory contains markers for more than one backend.

## A live session is not found

Keep the Workdeck TUI open, then run:

```bash
workdeck session list
workdeck session get --repo .
```

Use the repository root that the live review loaded. If an agent sandbox blocks loopback networking, grant local network access and retry. Do not expose the local daemon publicly. If the daemon itself prevents startup, set `WORKDECK_MCP_DISABLE=1` to run the TUI without session registration, then report the failure.

## Watch mode is rejected

`--watch` needs file- or VCS-backed input Workdeck can reopen. It cannot replay stdin patches or `--agent-context -`. Save the input to a file or use `workdeck diff` / `workdeck show` directly.

## Theme detection looks wrong

Some terminals do not answer background-color queries. `theme = "auto"` then falls back to `github-dark-default`; choose a theme explicitly if needed. Disable transparency if terminal compositing makes contrast unpredictable.

## Layout or text is hard to read

Try stack mode and wrapping in a narrow terminal:

```bash
workdeck diff --mode stack --wrap
```

Press `?` for shortcuts, `t` for themes, and `l` for line numbers. See [terminal compatibility](/docs/help/compatibility/) for mouse, color, and clipboard limits.

## Useful environment variables

| Variable           | Purpose                                                                                                                      |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------- |
| `WORKDECK_DEBUG=1`     | Enable available debug failure details; inspect for sensitive data before sharing.                                                          |
| `WORKDECK_MCP_DISABLE` | Set to `1` to start the TUI without registering a live session (escape hatch for daemon trouble).                            |
| `WORKDECK_TEXT_PAGER`  | Choose the plain-text pager used for non-diff output; see [Git pager and difftool](/docs/workflows/git-pager-and-difftool/). |

## Get command-specific help

```bash
workdeck --help
workdeck diff --help
workdeck session --help
```

When reporting a bug, include Workdeck version, OS, terminal name/version, shell, command shape, `WORKDECK_DEBUG=1` output, and a minimal safe patch when possible.

The upstream npm launcher's `HUNK_BIN_PATH` has no native equivalent: invoke the
required `workdeck` executable by its explicit path. Workdeck does not ship that
launcher. Never include credentials, private patches or raw sensitive session
content in a bug report.

Adapted from Hunk's pinned MIT documentation, Copyright Modem Labs Inc. This
page remains unmapped while complete migration and behavioral evidence are
verified; successful diagnosis of one environment does not qualify every platform.
