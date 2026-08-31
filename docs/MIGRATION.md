# Workdeck and Aya Split

The former uncommitted desktop review prototype used Aya product and crate names inside the Workdeck checkout. The split assigns that implementation to Workdeck because its responsibility is repository activity and review, while the independent Swift-native agent workspace becomes Aya.

## Workdeck result

- The original `workdeck` TUI and headless CLI remain the primary product surface.
- The Rust/Dioxus review application is the optional `Workdeck.app` desktop surface.
- Former `aya-*` crates are now `workdeck-*` crates.
- The desktop catalog helper is `workdeck-app`, avoiding a collision with the primary `workdeck` command.
- The desktop machine-local catalog remains separate from repo-local `.agents/workdeck/` data.
- Existing Aya-branded package evidence is not valid for the renamed Workdeck executable.

## External responsibilities

- Herder owns live agent sessions and worktree leases.
- Aya owns missions, experiments, policies, approvals, and adaptive agentic UI.
- Workdeck owns Git inspection and review.

No Aya source is vendored into Workdeck, and Workdeck does not depend on Aya or Herder private storage. See [Product Boundaries](PRODUCT_BOUNDARIES.md).
