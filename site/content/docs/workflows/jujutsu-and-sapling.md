+++
title = "Jujutsu and Sapling"
template = "docs.html"
+++

Workdeck detects Git, Jujutsu (`jj`), and Sapling (`sl`) repositories. `workdeck diff [target]` and `workdeck show [target]` pass native revsets to the detected backend.

## Jujutsu

```bash
workdeck diff
workdeck diff @-
workdeck show @
```

Configure Workdeck as jj's pager and request Git-format diffs:

```toml
[ui]
pager = ["workdeck", "pager"]
diff-formatter = ":git"
```

Edit user settings with `jj config edit --user`.

## Sapling

```bash
workdeck diff
workdeck diff .^
workdeck show .
```

Configure pager output with `sl config -u`:

```ini
[pager]
pager = workdeck pager
```

## Override detection

Set the backend in Workdeck config when a checkout is ambiguous:

```toml
vcs = "jj" # git, jj, or sl
```

Jujutsu and Sapling do not have Git's staging area, and stash review is Git-only. Their watch mode currently polls rather than observing repository files directly.

These integration examples are adapted from the pinned upstream documentation;
check your installed `jj` or `sl` version's configuration help before changing
personal settings. Workdeck requires the corresponding provider executable on
`PATH`. No provider settings are changed by reading this guide.

Adapted from Hunk's pinned MIT documentation, Copyright Modem Labs Inc.
Complete cross-provider, revision-expression and terminal parity remains a
release gate. This source interval stays unmapped pending migration verification.

