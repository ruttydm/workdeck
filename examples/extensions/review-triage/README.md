# Review triage extension

A session-local hunk review board for Workdeck. It records visited hunks and lets you mark the selected hunk **approved**, **investigate**, or **blocked**, with an optional rationale.

Build and stage the native extension from this checkout:

```console
cargo xtask extension stage-example review-triage
workdeck diff --extension target/workdeck-extension-examples/review-triage
```

Open **Extensions -> Toggle review triage** (`y`). The right pane lists every visible file and hunk; click a row to navigate the review. Use **Mark selected hunk...** (`x`) to choose a decision. Centering, review focus, and clearing decisions remain available in the Extensions menu.

State belongs to the running review only. Reload reconciliation drops decisions, visit marks, and note counts whose parsed hunk no longer exists, so changed code never inherits a stale decision.

The example exercises native commands, resizable declarative panes and pane actions, input/select/confirmation dialogs, notifications, semantic review navigation, lifecycle events, and namespaced inter-extension events. The host owns rendering and input throughout.
