# Workdeck

Workdeck is an advanced, keyboard-first Git workbench that runs entirely in the terminal. It keeps the repository—not the agent conversation—at the center while making large, fast-moving worktrees easier to inspect and review.

Workdeck ships one product surface: the `workdeck` TUI. Its headless subcommands expose the same repository model to scripts and integrations. It does not ship a desktop app, browser UI, or embedded web server.

## Product boundary

| Product | Authority |
| --- | --- |
| **Workdeck** | Terminal Git workflow, repository inspection, diffs, issues, handoffs, and review state |
| **Herder** | Agent sessions, PTYs, processes, worktree leases, logs, cancellation, and recovery |
| **Aya** | Graphical applications, missions, experiments, policy, approvals, and agentic control |

Workdeck can show imported Herder session metadata and attribution, but it never hosts the session. Aya can consume repository evidence, but Git remains the shared source of truth. See [Product Boundaries](docs/PRODUCT_BOUNDARIES.md).

## Features

- Ratatui/crossterm TUI optimized for narrow terminal panes;
- repository status, changed-file trees, diffs, files, syntax previews, and search;
- branch, commit, stash, tag, remote, project, issue, and handoff context;
- repo-local issue and review metadata under `.agents/workdeck/`;
- JSON and JSONL output for scripts and integrations.

Read-only commands do not create repository-local state. Initialization and mutations are always explicit.

## Install

### Cargo

```sh
cargo install --git https://github.com/ruttydm/workdeck --package workdeck-cli --locked
workdeck --version
```

### Homebrew HEAD

```sh
brew tap ruttydm/workdeck https://github.com/ruttydm/workdeck
brew install --HEAD ruttydm/workdeck/workdeck
workdeck --version
```

### Local checkout

```sh
cargo install --path crates/workdeck-cli --locked
workdeck
```

## Quick start

```sh
# Open the TUI in the current repository
workdeck

# Headless repository inspection
workdeck status --json
workdeck changes list --group status --json
workdeck files list . --json
workdeck search authentication --target files,changes --json

# Explicitly initialize repo-local Workdeck state
workdeck --init
workdeck doctor
```

## Validate

```sh
cargo fmt --all --check
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --release --package workdeck-cli --bin workdeck
scripts/soak.sh
```

Workdeck is MIT licensed.
