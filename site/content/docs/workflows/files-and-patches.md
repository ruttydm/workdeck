+++
title = "Files and patches"
template = "docs.html"
+++

Use file comparison when you already have before and after content, and patch mode when another tool emits unified diff text.

## Compare files

```bash
workdeck diff --files before.rs after.rs
```

The explicit `--files` option keeps file comparison distinct from `workdeck diff <from> <to>`, which compares two VCS revisions. Add `--watch` to reload when either file changes:

```bash
workdeck diff --files before.rs after.rs --watch
```

## Open a patch file

```bash
workdeck patch changes.patch
```

A file-backed patch can use watch mode. It remains tied to that file path.

## Read a patch from stdin

```bash
git diff --no-color | workdeck patch -
```

Use `-` to make stdin explicit. Stdin is a snapshot, so it cannot use `--watch`; write the patch to a file when you need continuous reloads.

Patch-like input is parsed into the same file and workdeck model as repository input. Non-diff text belongs in [pager mode](/docs/workflows/git-pager-and-difftool/), where Workdeck can fall back to plain text.

Adapted from Hunk's pinned MIT documentation, Copyright Modem Labs Inc.
Native input and watch tests cover these command forms; complete provider,
reload-state and terminal parity remains unverified. This source interval stays
unmapped pending complete migration verification.

