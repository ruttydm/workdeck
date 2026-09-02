# Workdeck

Workdeck is an advanced, keyboard-first review workbench that runs entirely in the terminal. Its primary surface is a continuous Hunk-style diff canvas backed by Rust, Ratatui, and crossterm; Git, Jujutsu, Sapling, files, issues, agents, projects, cycles, labels, events, and search remain in the same shell.

Workdeck ships one product surface: the `workdeck` TUI. Its headless subcommands expose the same repository model to scripts and integrations. It does not ship a desktop app, browser UI, or embedded web server.

## Product boundary

| Product | Authority |
| --- | --- |
| **Workdeck** | Terminal Git workflow, repository inspection, diffs, issues, handoffs, and review state |
| **Herder** | Agent sessions, PTYs, processes, worktree leases, logs, cancellation, and recovery |
| **Aya** | Graphical applications, missions, experiments, policy, approvals, and agentic control |

Workdeck can show imported Herder session metadata and attribution, but it never hosts the session. Aya can consume repository evidence, but Git remains the shared source of truth. See [Product Boundaries](docs/PRODUCT_BOUNDARIES.md).

## Features

- split, stack, and responsive continuous review layouts with syntax spans, live comments, watch mode, and agent notes;
- Ratatui/crossterm TUI optimized for narrow terminal panes;
- built-in Git, Jujutsu, and Sapling providers plus patch, stdin, pager, stash, show, and difftool inputs;
- repository status, changed-file trees, diffs, files, syntax previews, and search;
- branch, commit, stash, tag, remote, project, issue, and handoff context;
- repo-local issue and review metadata under `.agents/workdeck/`;
- authenticated local review sessions and trusted native JSON-RPC extensions;
- JSON and JSONL output for scripts and integrations;
- no Bun, React, OpenTUI, JavaScript engine, or WASM runtime.

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

### Nix

```sh
nix run github:ruttydm/workdeck -- --help
```

The flake package, Home Manager module, supported systems, and source-build commands are documented
in [nix/README.md](nix/README.md).

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

# Full review commands
workdeck diff --watch
workdeck show HEAD~1
workdeck patch change.patch
workdeck session list --json

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
cargo xtask verify
cargo xtask architecture check
cargo xtask nix check
cargo xtask skill check
cargo xtask site check
```

## Hunk semantic rebase

The pinned Hunk tree is being ported through a byte-exact ledger rather than merged as unrelated Git ancestry. `cargo xtask port fetch` recreates the namespaced upstream refs and source anchors; `cargo xtask port status` reports the honest remaining queue; strict `cargo xtask port audit` is the release gate. See [the semantic-port ledger](port/hunk/README.md).

The live Cargo and Rust module graph is checked against the product ownership boundaries by `cargo xtask architecture check`; the same check runs inside `cargo xtask verify`. See [Architecture](docs/ARCHITECTURE.md).

The bundled [review skill](skills/workdeck-review/SKILL.md) is generated from the typed native
session command and error catalogs. Regenerate it with `cargo xtask skill generate`; CI and
`cargo xtask verify` reject drift with `cargo xtask skill check`. See
[Agent workflows](docs/agent-workflows.md).

Workdeck is MIT licensed. Hunk-derived and other third-party portions retain their required notices in [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES).
