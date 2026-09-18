# 3-agent-review-demo

A flagship Workdeck demo: a small command-palette refactor with inline agent rationale attached to the interesting hunks.

## Run

```bash
workdeck patch examples/3-agent-review-demo/change.patch \
  --agent-context examples/3-agent-review-demo/agent-context.json
```

## What to look for

- query normalization extracted into its own helper
- ranking logic that prefers strong matches over loose substring hits
- inline notes beside the changed hunks, not in a separate panel or PR description

The Rust trees, tests, patch, and agent sidecar are validated by `cargo test -p workdeck-examples`.
