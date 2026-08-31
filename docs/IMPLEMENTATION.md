# Workdeck Implementation and A–Z Matrix

The binding scope is [WORKDECK_DIOXUS_IMPLEMENTATION_PLAN.md](WORKDECK_DIOXUS_IMPLEMENTATION_PLAN.md). This document maps it to executable code and gates.

| Area | Implementation | Deterministic evidence |
| --- | --- | --- |
| Fresh launch | schema-2 `ApplicationPaths`, empty onboarding, suggested portfolio roots | core/db tests; empty fixture; Playwright |
| Shell | 44px rail, tabs, two-row chrome, pane persistence, responsive drawers | UI SSR tests; 70-state visual matrix |
| Workspaces | Project → Repository → Checkout → Worktree treegrid | presenter tests; hierarchy interaction; Axe |
| Updates | bounded scan, task events, cancellation, commit/PR read cursors | domain/db/API tests; interaction suite |
| Search | grouped/bounded results and saved/recent queries | API/presenter tests; keyboard UI |
| Changes | unified/split/source/Markdown/calls/structure/AST and canonical right tree | analysis tests; change interaction; visual matrix |
| Git | read-only references, stashes, WIP, graph SVG, pagination, ranges, immediate three-column diff | temporary Git repositories; UI tests |
| Pull requests | dense unread list, immediate diff, hierarchical changed files | GitHub fixtures; Playwright/Axe |
| CI | runs/jobs/steps/logs, partial/error/load states | GitHub fixtures; UI tests |
| Artifacts | hardened ZIP, loopback preview, sandboxed iframe, shutdown | artifact security and lifecycle tests |
| TUI and headless CLI | status/files/changes/search plus local issue, project, and imported-session context | `workdeck-cli` tests and soak |
| Desktop helper | catalog/updates/git/search/GitHub/artifact/doctor/fixture | isolated feature matrix; JSON assertions |
| Packaging | Workdeck.app, TUI/helper binaries, icon/fonts/notices/SBOM, ad-hoc signing | package and codesign gates |

## Strict defect loop

Every product defect is reproduced, fixed at its owning boundary, covered by a deterministic test, rebuilt in the web fixture where relevant, visually inspected, and then rerun against the packaged app when native behavior is involved. Retrying, increasing limits, or blessing stale snapshots is not a fix.

## Gate layers

1. Rust format, locked metadata, desktop/all-target/wasm checks.
2. Unit, integration, security, schema, fixture, Clippy, and rustdoc.
3. Cargo-deny, provenance, Tailwind checksum/output, no-authored-JS, render I/O, and accessibility contracts.
4. Isolated CLI A–Z and 74/75/109 deterministic scale fixture.
5. Dioxus web build plus Playwright pointer, keyboard, Axe, light/dark, minimum/intermediate/wide/ultrawide, reduced-motion, empty, offline, and component-gallery coverage.
6. Release package, strict codesign, SQLite integrity, helper shutdown, live read-only provider probe.
7. Codex Computer Use only for exact-package titlebar, menus, shortcuts, dialogs, file panels, focus, appearance, resize, close, and quit.

Source gates run with `scripts/quality-gates.sh`. Release gates add `--release`; hash-bound native evidence adds `--native-evidence`; current live provider probes add `--live`.
