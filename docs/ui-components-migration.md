# Hunk UI component test migration

The pinned Hunk UI component suite is `src/ui/components/ui-components.test.tsx` at
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.  Its 144,670 bytes and SHA-256
`22572d8600014118c9c8c87fb491938b26d28075d6208a9697d60650b21926a9` are verified directly
with `git show`; the source is not copied into the Workdeck tree.  The same file at the
stable anchor is 130,408 bytes (`3ba7abbb7c8bdd8423f61cd91a4264779bdc8820d707eb2eb8ddcbe8aeb99a82`).

All 84 upstream tests in the baseline (and all 76 stable tests, which are a subset of the baseline
names) have an explicit native Rust test mapping in `xtask/src/ui_components.rs`.  The verifier
requires the exact source test-name order, unique names, non-empty Rust anchors, parseable
`#[test]` functions, and the complete set of frozen component oracles.  The source interval is
non-overlapping and is not considered covered merely because a renderer exists: every test name must resolve to an
executable test.

The native implementation is split across the Ratatui public review primitives, review stream
and input owner, row/section planners, source loader, line cursor/highlight coordinators, note
painters, menu, status bar, and help dialog.  The frozen oracles cover sidebar modes, file headers,
row and section geometry, inline notes, menus, status precedence, help, and viewport highlight
prefetch.  Workdeck branding and Ratatui cell ownership are the only intentional presentation
adaptations; geometry, content, navigation, callbacks, and state transitions remain tested.

Run the focused gate with:

```text
cargo test --locked -p xtask native_ui_components_replaces_the_complete_pinned_test_corpus -- --nocapture
```

The strict port audit invokes the same verifier before accepting the ledger disposition. No
TypeScript source mirror, JavaScript runtime, or Hunk executable is part of this translation.
No TypeScript source mirror is committed or executed.
