# Launcher migration

Hunk's pinned `bin/hunk.cjs` is a 3,762-byte npm launcher. It resolves a
platform package, makes that package executable, supports `HUNK_BIN_PATH`, and
falls back to a bundled Bun entrypoint. Those branches are accounted for here
as a native replacement; the source is read from the protected Hunk ref by
`cargo xtask verify` and is not copied into the Workdeck tree or executed.

| Hunk launcher surface | Workdeck replacement | Boundary |
| --- | --- | --- |
| `skill path` special case | `workdeck skill path [name]`, with JSON output and atomic materialization of the bundled Rust-owned skills | `crates/workdeck-cli/src/main.rs` and `skills/` |
| executable permission repair | Cargo install/archive permissions and the native installer's atomic file publication | `crates/workdeck-cli/src/main.rs`, `xtask/src/install.rs` |
| `hostCandidates` and npm package lookup | Cargo's one `workdeck` binary selected by the release artifact for the host triple | `Cargo.toml`, `xtask/src/release_channel.rs`, `xtask/src/install_oracle.rs` |
| `HUNK_BIN_PATH` override | intentionally unsupported; Workdeck never delegates to an untrusted Hunk process | native CLI composition root and this policy |
| bundled Bun fallback | intentionally unsupported; no JavaScript runtime is shipped | `xtask/src/architecture.rs` |
| inherited arguments and exit status | Clap dispatch and `Result`/`ExitCode` handling at the native composition root | `crates/workdeck-cli/src/main.rs` |

The launcher itself is therefore a migrated tooling artifact, not a runtime
source mirror. `architecture::verify_legacy_launcher` checks the complete
pinned marker surface, the native ownership points, the explicit unsupported
runtime branches, and the absence of `bin/hunk.cjs` before the architecture
gate succeeds. Hunk's MIT copyright and the npm launcher's provenance remain
covered by `THIRD_PARTY_NOTICES`.
