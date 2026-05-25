# workdeck-cli

`workdeck` is a terminal-native sidecar for agentic coding. It shows a narrow-pane friendly overview of Git changes, files, local TOML-backed tasks, agent sessions, previews, and fuzzy search. The default tab is a folder-structured Changes tree, with status grouping available from the TUI.

```sh
cargo install --path crates/workdeck-cli
workdeck
```

Initialize repo-local data under `.agents/workdeck/`:

```sh
workdeck --init
```

Print a read-only JSON status snapshot:

```sh
workdeck --status-json
```
