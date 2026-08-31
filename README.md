# Workdeck

Workdeck is an advanced, keyboard-first Git workbench for reviewing fast-moving repositories and agent-produced changes. It keeps the repository—not the agent conversation—at the center.

The primary experience is the `workdeck` TUI. A local web view and the optional native `Workdeck.app` provide more room for portfolio activity, pull requests, CI, artifacts, semantic source views, and large diffs without changing the product's responsibility.

## Product boundary

| Product | Authority |
| --- | --- |
| **Workdeck** | Repositories, Git state, diffs, commits, pull requests, CI evidence, and review state |
| **Herder** | Agent sessions, PTYs, processes, worktree leases, logs, cancellation, and recovery |
| **Aya** | Missions, experiments, policy, approvals, and adaptive agentic presentation |

Workdeck can attribute a change to a Herder session and jump to that session, but it never hosts the session. Aya can display Workdeck evidence, but Workdeck remains the review authority. See [Product Boundaries](docs/PRODUCT_BOUNDARIES.md).

## Surfaces

### Terminal workbench

The `workdeck` binary provides:

- a Ratatui/crossterm TUI optimized for narrow terminal panes;
- repository status, changed-file trees, diffs, files, syntax previews, and search;
- local branch, commit, stash, tag, remote, project, issue, and handoff context;
- JSON/JSONL commands for scripts and integrations;
- a lightweight local web view through `workdeck web`.

Agent entries are imported session metadata and attribution only. Herder owns live session lifecycle.

### Optional desktop workbench

`Workdeck.app` is a Rust/Dioxus macOS application for denser review across repositories and linked worktrees. It adds:

- unread commit and pull-request activity;
- three-column commit and PR review;
- CI, provider, search, and secure HTML artifact surfaces;
- tree-sitter syntax, Markdown, calls, structure, and AST evidence;
- a machine-local SQLite catalog that never writes into reviewed repositories.

The current desktop implementation is read-only toward input repositories. Future Git mutations must remain explicit user actions and share Workdeck's command policy; they must never be inferred from agent output.

`workdeck-app` is the desktop catalog's JSON-friendly automation helper. It is intentionally distinct from the primary `workdeck` TUI command until their overlapping command surfaces can be consolidated without breaking compatibility.

## Install the TUI

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
# TUI in the current repository
workdeck

# lightweight local web UI
workdeck web

# headless repository inspection
workdeck status --json
workdeck changes list --group status --json
workdeck files list . --json
workdeck search authentication --target files,changes --json
```

Repo-local issue, review, and imported-session metadata lives under `.agents/workdeck/` only after explicit initialization or mutation:

```sh
workdeck --init
workdeck doctor
```

Read-only commands do not create repository-local state.

## Desktop development

Prerequisites are the pinned Rust toolchain, Xcode on macOS, the `wasm32-unknown-unknown` target, Dioxus CLI 0.7.9, the pinned Tailwind standalone binary, and Node for development-only Playwright tests.

```sh
cargo run --locked --package workdeck-desktop --bin workdeck-desktop
cargo run --locked --package workdeck-app-cli --bin workdeck-app -- --help
```

The desktop workspace is layered as:

```text
workdeck-api          renderer protocol and deterministic fixtures
workdeck-domain       identities, activity cursors, and review evidence
workdeck-db           machine-local SQLite catalog
workdeck-git          read-only discovery, history, changes, and graph
workdeck-analysis     syntax and semantic evidence
workdeck-github       bounded read-only GitHub integration
workdeck-artifacts    secure artifact import and preview
workdeck-core         application services
workdeck-presenter    native runtime and protocol mapping
workdeck-ui           Dioxus components and feature surfaces
workdeck-desktop      macOS host and native menus
workdeck-app-cli      desktop-catalog automation helper
```

The desktop catalog uses `WORKDECK_DATA_DIR` for isolated tests and a platform application-support directory otherwise. It does not read or replace the TUI's repo-local `.agents/workdeck/` data.

## Validate

```sh
# TUI-focused checks
cargo test --locked --package workdeck-cli

# complete source, desktop, web, accessibility, and security gates
scripts/quality-gates.sh

# package the optional macOS app
scripts/package-macos.sh --replace
open dist/Workdeck.app
```

The macOS package is ad-hoc signed for local use. Public desktop distribution still requires Developer ID signing, hardened-runtime review, notarization, and stapling.

Workdeck is MIT licensed. See [Architecture](docs/ARCHITECTURE.md), [Product Vision](docs/PRODUCT_VISION.md), [Implementation](docs/IMPLEMENTATION.md), and [Dependency Policy](docs/DEPENDENCY_POLICY.md).
