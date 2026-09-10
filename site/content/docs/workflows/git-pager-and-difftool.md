+++
title = "Git pager and difftool"
description = "Use the native reviewer for Git pager output or explicit file pairs."
template = "docs.html"
+++

Pager mode inspects stdin: patch-like content opens the review UI, while ordinary pager text uses Workdeck's plain-text fallback.

These commands change your personal Git configuration. Record existing values
before applying them, and restore those values when undoing an integration.
They are instructions, not changes performed by opening this guide.

## Configure the Git pager

```bash
git config --global core.pager "workdeck pager"
```

Afterward, commands such as `git diff` and `git show` can open in Workdeck. Git controls pager input, so untracked files are not synthesized in this mode.

Keep your normal pager and add opt-in aliases instead:

```bash
git config --global alias.wdiff '-c core.pager="workdeck pager" diff'
git config --global alias.wshow '-c core.pager="workdeck pager" show'
```

## Plain-text fallback

Output that is not a unified diff never opens the review UI; Workdeck streams it to a plain-text pager instead. The pager command comes from `WORKDECK_TEXT_PAGER`, then `PAGER`, then falls back to `less -R`. A value that resolves back to `workdeck` is ignored so Git can never recurse into `workdeck pager` itself.

## Configure Git difftool

Tell Git how to invoke Workdeck for each temporary file pair:

```bash
git config --global diff.tool workdeck
git config --global difftool.workdeck.cmd 'workdeck difftool "$LOCAL" "$REMOTE" "$MERGED"'
git config --global difftool.prompt false
```

Then run:

```bash
git difftool
```

Difftool is pair-oriented because Git invokes the command once per file. Prefer `workdeck diff` when you want Workdeck's native full-changeset stream.

## Undo the integration

```bash
git config --global --unset core.pager
git config --global --remove-section difftool.workdeck
```

Without an interactive terminal, native pager routing can use static diff output
or pass-through instead of opening the reviewer. `TERM=dumb` and captured pager
hosts also affect that decision. The integration snippets assume an interactive
terminal and a `workdeck` executable on `PATH`.

The undo commands above remove the shown entries; they do not restore previous
values, remove the optional `wdiff`/`wshow` aliases, or reset `diff.tool` and
`difftool.prompt`. Restore your recorded settings for a complete rollback.

Adapted from Hunk's pinned MIT guide, Copyright Modem Labs Inc. Complete pager,
terminal and cross-platform parity remains unverified; the source stays unmapped.

