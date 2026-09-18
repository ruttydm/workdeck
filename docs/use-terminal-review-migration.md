# `useTerminalReview` semantic migration

The pinned Hunk terminal-review hook is accounted for as a complete source contract.  The
baseline `src/ui/hooks/useTerminalReview.ts` blob at `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`
is 57,589 bytes (SHA-256
`7ebddab5d2cba4676dfe6d3a16decf568ea3fef80836a5d18b35fde7480c289a`).  The stable
`v0.20.1` blob at `4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` is 55,549 bytes (SHA-256
`6996dda23b5e622f15149072ff82bcd39d8a0f21ab12300fb07d708aefd3333a`).  The `xtask` verifier
reads both blobs directly from Git and requires their complete, non-overlapping source surfaces;
No TypeScript source mirror is retained.

## Contract projection

Every top-level function, interface, type, and constant is enumerated by
`xtask/src/use_terminal_review.rs`.  The baseline functions are `useReviewStoreSnapshot`,
`withMissingNoteMessage`, `revealRequestFor`, and `useTerminalReview`.  The stable-only helpers
`mergeAnnotationMaps` and `sameLineCursor` are also required.  `SourceLoadRequest`,
`LineCursorRevealRequest`, `ReviewSelectionOptions`, `TerminalReview`,
`AgentNoteGeometrySnapshot`, `RevealedLineResult`, and `EMPTY_AGENT_LINE_HIGHLIGHTS` are checked
by name and order.

The native ownership is deliberately split at the same semantic boundary rather than copying
React or OpenTUI architecture:

- `workdeck-core` projects immutable review documents and runtime file identity.
- `workdeck-review` owns the subscribed semantic store, snapshots/revisions, selection and reveal
  intents, note threading, annotation indexes, and source-status state.
- `workdeck-tui` owns Ratatui review rendering, measured line cursors, source-gap workers,
  viewport/selection reconciliation, note editing, copy/navigation actions, and reload handling.
- `workdeck-session`/`session_review_controller.rs` is the authenticated session bridge and owns live comments, batches,
  navigation, agent highlights, markup validation, and lifecycle ordering against the same store.

## Behavioral obligations

The executable parity evidence covers reload reconciliation and retired-content cleanup, file and
hunk selection/reveal anchors, line-cursor seeding and stepping, collapsed-gap source loading,
threaded user and live notes, filter-preserving navigation, agent highlight carry-over, STML
feedback, and session callback ordering.  Frozen review, source, cursor, note, reload, and
app-host oracles must name both pinned trees.  Focused verification is:

```text
cargo test --locked -p xtask native_rust_terminal_review_replaces_both_pinned_hook_contracts -- --nocapture
```

The hook is therefore represented by Rust behavior and executable tests, not by a dead compatibility
function or an unverified aggregate mapping.
