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
| `vercel.json` | Zola `site/config.toml`, Workdeck templates, and the Cargo-driven site check/export pipeline | `tooling_configs::verify` native deployment-boundary check |
| `website/src/content.config.ts` | Rust frontmatter/title validation in `xtask/src/site_markdown.rs` | `native_docs_collection_requires_valid_frontmatter_and_a_single_line_title` |
| `website/.gitignore` | root `.gitignore` plus validated disposable Zola staging | `cargo xtask site check` |
| `bunfig.toml` | Cargo.lock and the pinned Rust toolchain | `cargo xtask verify` dependency/toolchain checks |
| `.gitignore` | native Rust/Zola output and artifact ignores in the root `.gitignore` | `tooling_configs::verify` pinned-policy check |
| `.lintstagedrc.json` | Cargo formatting and Clippy gates run by CI and `cargo xtask verify` | `tooling_configs::verify` pinned-policy check |
| `knip.json` | Rust module reachability, Cargo dependency graph, compiler dead-code checks, and architecture gates | `cargo xtask verify` architecture check |
| `tsconfig.examples.json` | `examples/Cargo.toml` Rust example workspace and Ratatui example fixtures | `tooling_configs::verify` pinned-policy check |
| `tsconfig.opentui.json` | `crates/workdeck-tui/Cargo.toml` Ratatui renderer crate; no OpenTUI declarations are emitted | `tooling_configs::verify` pinned-policy check |
| `tsconfig.extension.json` | `crates/workdeck-extension-api/Cargo.toml` native extension API crate | `tooling_configs::verify` pinned-policy check |
| `tsconfig.json` | Cargo workspace, lockfile, and Rust module graph | `tooling_configs::verify` pinned-policy check |
| `test/cli/install-vm/.dockerignore` | Native installer VM context allowlist in `xtask/src/tooling_configs.rs` | `tooling_configs::verify` pinned-policy check |

`test/cli/install-vm/.dockerignore` is intentionally handled by the native
installer oracle and CI smoke-test plan (`xtask/src/install_oracle.rs`), not by
a Docker runtime. The source fixture is never executed by Workdeck.

Source files are from Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, MIT,
copyright Modem Labs Inc.; see `THIRD_PARTY_NOTICES`.
