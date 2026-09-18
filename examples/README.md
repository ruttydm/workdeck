# Examples

Ready-to-run Rust demos for Workdeck and its exported Ratatui review components.

Each folder tells a small review story and includes the exact command to run from the repository root. The workspace crate compiles every source tree and executes its parity assertions.

## Quick menu

| Example | Best for | Command |
| --- | --- | --- |
| `1-hello-diff` | fastest first run | `workdeck difftool examples/1-hello-diff/before.rs examples/1-hello-diff/after.rs` |
| `2-mini-app-refactor` | realistic multi-file review | `workdeck patch examples/2-mini-app-refactor/change.patch` |
| `3-agent-review-demo` | inline agent rationale | `workdeck patch examples/3-agent-review-demo/change.patch --agent-context examples/3-agent-review-demo/agent-context.json` |
| `4-ui-polish` | screenshot-friendly Ratatui diff | `workdeck difftool examples/4-ui-polish/before.rs examples/4-ui-polish/after.rs` |
| `5-pager-tour` | line scrolling, paging, and hunk jumps | `workdeck difftool examples/5-pager-tour/before.rs examples/5-pager-tour/after.rs --pager` |
| `6-readme-screenshot` | README screenshot with agent notes | `workdeck patch examples/6-readme-screenshot/change.patch --agent-context examples/6-readme-screenshot/agent-context.json --mode split --theme github-dark-default` |
| `7-ratatui-component` | embedding the public Workdeck diff view | `cargo test -p workdeck-examples ratatui_component` |
| `8-ratatui-primitives` | composing Workdeck's public Ratatui primitives | `cargo test -p workdeck-examples ratatui_primitives` |
| `9-agent-markup-notes` | STML rendered inside notes | `workdeck patch examples/9-agent-markup-notes/change.patch --agent-context examples/9-agent-markup-notes/agent-context.json` |

Run every compiled example and fixture assertion with:

```bash
cargo test -p workdeck-examples --all-targets
```

## Native extension examples

The semantic-port queue retains these baseline extension stories and rewrites each as a compiled native extension with `workdeck-extension.toml` and newline-delimited JSON-RPC:

- `extensions/github-pr/` adds a dependency-free `workdeck gh 123` command that fetches GitHub pull-request diffs and delegates them into Workdeck.
- `extensions/cli-tools/` demonstrates minimal CLI exit and delegation contracts.
- `extensions/review-triage/` adds a session-local hunk-triage sidebar.
- `extensions/review-note-navigator/` inventories saved review notes and navigates to visible authoritative anchors.
- `extensions/review-snapshot-export/` exports stable file identities and saved review notes with a stale-work guard.
- `extensions/rendered-markdown/` adds an optional parsed Markdown file presentation.
- `extensions/inline-edit/` edits the file under review through a file-view mode, layout refresh, and host-mediated workspace writes.
- `extensions/declarative-file-view/` is the smallest fixed-row declarative file-view proof of concept.
- `extensions/file-view-gallery/` provides constrained native presentations for checked-in Rust, CSS, and manifest diffs: an impact atlas, real color swatches, and highlighted dependency versions.

Extension examples are not bundled into the Workdeck executable. Each migrated README explains how to build and trust-gate its folder explicitly. Native extension ports are added only when their complete baseline records and tests are ready; legacy script extensions are never executed.

## Notes

- Patch-based examples include checked-in `change.patch` files, so they open without creating a temporary repository.
- Agent demos include `agent-context.json` sidecars to show inline review notes beside the diff.
- The pager tour is intentionally taller than a typical viewport so you can try `↑`, `↓`, `PageUp`, `PageDown`, `Home`, `End`, and `[` / `]` immediately.
- The Ratatui component example includes both `from_files.rs` and `from_patch.rs`, demonstrating the same surface from before/after contents and raw unified diff text.
- The Ratatui primitives example assembles a custom review UI from Workdeck's exported navigator, header, body, and stream renderers while keeping state and input in the host.
