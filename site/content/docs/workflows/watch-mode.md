+++
title = "Watch mode"
template = "docs.html"
+++

Watch mode turns a review into a continuous view of a changing source.

## Start a watched review

```bash
workdeck diff --watch
```

Workdeck observes direct-file and Git-backed inputs for prompt refreshes and keeps periodic polling as a fallback. It polls Jujutsu and Sapling input.

Other reopenable inputs also work:

```bash
workdeck show HEAD~1 --watch
workdeck diff --files before.rs after.rs --watch
workdeck patch changes.patch --watch
```

## Know what can reload

Watch mode requires input Workdeck can open again. Stdin-backed patches and stdin agent context cannot be watched:

```bash
# Snapshot only; --watch would fail
some-command | workdeck patch -
```

Save changing output to a file or use a repository-backed command instead.

## Refresh manually

Press `r` for a reloadable review when you need an immediate refresh without continuous watch mode. A live agent can also use `workdeck session reload` to replace the session's entire input.

Adapted from Hunk's pinned MIT documentation, Copyright Modem Labs Inc.
Native input and watch tests cover these command forms; complete provider,
reload-state and terminal parity remains unverified. This source interval stays
unmapped pending complete migration verification.

