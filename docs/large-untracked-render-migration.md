# Large untracked render migration

`cargo xtask benchmark large-untracked-render [line-count] [tracked]` is the native Ratatui
replacement for Hunk's `scripts/test-large-untracked-render.tsx` workload. It creates an isolated
Git repository, loads the file through the bundled Git provider, and renders the real `ReviewApp`
cell buffer at 120×30. Both untracked additions and tracked replacements use the same bounded
large-file policy: the path and `File exceeds review limits` skip message remain visible while the
full body is not materialized into diff rows.

The protected baseline and stable source blobs are checked for 3,135 bytes, 94 lines, and
SHA-256 `f3b7b71e44149c753d8dfafea08b1c29976e17c79660b8a37364512f4868b4f0`. The native fixture
reports the same JSON fields as the source workload and never starts a JavaScript or React runtime.
