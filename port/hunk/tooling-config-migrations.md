# Hunk tooling configuration migrations

These records are configuration-only files from Hunk's JavaScript toolchain.
Workdeck has no Bun, Oxlint, Oxfmt, Astro, or Node runtime, so their behavior
is represented by the native Cargo/Zola verification commands below. The
source bytes are read from `git show` at the pinned baseline by the ledger
audit; this document records the per-file disposition and replacement rather
than retaining an executable JavaScript configuration mirror.

| Pinned file | Native replacement | Verification |
| --- | --- | --- |
| `CLAUDE.md` | `docs/ARCHITECTURE.md` and the repository's canonical Rust architecture docs | `cargo xtask verify` architecture check |
| `.oxfmtrc.json` | `rustfmt` from `rust-toolchain.toml` | `cargo fmt --all -- --check` |
| `.oxlintrc.json` | Clippy's native lint configuration and CI invocation | `cargo clippy --locked --workspace --all-targets -- -D warnings` |
| `.env.test` | `xtask::isolate_test_git_config` and `workspace_test_command` | `test_git_isolation_replaces_inherited_global_and_system_configuration` |
| `website/tsconfig.json` | Zola's `site/config.toml` and Rust site pipeline | `cargo xtask site check` |
| `website/.gitignore` | root `.gitignore` plus validated disposable Zola staging | `cargo xtask site check` |
| `bunfig.toml` | Cargo.lock and the pinned Rust toolchain | `cargo xtask verify` dependency/toolchain checks |

`test/cli/install-vm/.dockerignore` is intentionally handled by the native
installer oracle and CI smoke-test plan (`xtask/src/install_oracle.rs`), not by
a Docker runtime. The source fixture is never executed by Workdeck.

Source files are from Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, MIT,
copyright Modem Labs Inc.; see `THIRD_PARTY_NOTICES`.
