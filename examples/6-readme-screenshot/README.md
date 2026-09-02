# 6-readme-screenshot

A screenshot-optimized Workdeck demo: a multi-file Ratatui refactor with inline agent rationale.

## Run

```bash
workdeck patch examples/6-readme-screenshot/change.patch \
  --agent-context examples/6-readme-screenshot/agent-context.json \
  --mode split \
  --theme github-dark-default
```

## Screenshot setup

- use a wide terminal so the sidebar and split diff are both visible
- keep the first file selected: `src/components/review_summary_card.rs`
- make sure agent notes are visible
- capture the first annotated hunk with the note popover open

## What it shows well

- inline agent rationale beside the changed code
- a clear mix of removed and added lines in one hunk
- a visible multi-file sidebar
- Rust prop renames, copy edits, helper extraction, and strong syntax color

The source trees, tests, patch, and sidecar are validated by `cargo test -p workdeck-examples`.
