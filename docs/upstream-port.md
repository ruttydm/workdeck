# Hunk upstream semantic port

Workdeck keeps the pinned Hunk history in protected `hunk-upstream/*` refs and
does not copy its TypeScript runtime into the shipped tree.  The 92 commits
after baseline `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` are recorded in
`port/hunk/upstream-ledger.jsonl`.

Each record is an independently authenticated port receipt.  It records the
full commit ID and parent, the exact subject, SHA-256 digests of Git's patch and
changed-path stream, a semantic ownership area, a native Rust destination, and
executable evidence.  `cargo xtask port audit` recomputes those values through
`git show` and checks the destination/evidence paths.  A changed upstream
object, reordered commit, missing destination, or missing test fails the audit.

## Ownership areas

| Area | Native owner | Evidence |
| --- | --- | --- |
| session | `workdeck-session` | broker lifecycle tests |
| extensions | `workdeck-extension-api` and host | registration tests |
| pager | CLI terminal pager | pager integration tests |
| update/install | CLI update and install modules | transaction/update tests |
| media | Rust terminal-media tooling | terminal-media documentation |
| watch/VCS | `workdeck-vcs` | watcher and Git integration tests |
| performance/tooling | Cargo/xtask and CI | benchmark and CI inputs |
| release/website | Rust site and release pipeline | release/site verifiers |
| diff/TUI | `workdeck-diff` and `workdeck-tui` | geometry and Ratatui parity tests |

The ledger is intentionally commit-granular rather than a blanket “upstream is
equivalent” claim.  Documentation and release-only commits still retain their
source object and patch digest, while their destination names the generated or
migrated Rust/content owner.  Temporary oracle execution is allowed during
verification; no JavaScript source mirror is part of the final repository.
