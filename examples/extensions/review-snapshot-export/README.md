# Review snapshot export extension

This trusted native Rust extension exports Workdeck's authoritative saved review state as JSON. It demonstrates why the command context's review snapshot is more useful than accumulating note-created events: one immutable value contains every saved note currently retained by the shared review state, including stale and orphaned notes an exporter must handle explicitly.

Stage and run it from this checkout:

```bash
cargo xtask extension stage-example review-snapshot-export
cargo run -p workdeck-cli -- --extension ./target/workdeck-extension-examples/review-snapshot-export diff
```

Add one or more saved review notes, then choose **Extensions → Export review snapshot…** or press `F9`. Select a new output path. Relative paths resolve from the review command's working directory, and the extension uses create-new filesystem semantics so it never overwrites an existing file.

The JSON includes the opaque producer generation and review-state revision; every file's stable key, runtime navigation id, content identity, status, path, stats, and flags; and every saved live or reviewer note with its resolved old/new anchor and reconciliation status. Drafts and static sidecar annotations that never entered shared review state are absent. Notes retain authoritative arrival and creation order.

The command captures a snapshot before opening its host-rendered path dialog, then receives the current snapshot again on submission. If the generation or revision changed, it refuses stale output and asks the user to rerun the command. Native publishers can use the same guard before an irreversible network request.

## Trust

The extension writes the user-selected path directly with the user's permissions. Native Workdeck extensions are trusted programs, not security sandboxes. This differs from mediated workspace-write capabilities, which target reviewed files and require consent. Install and run only extensions you trust.
