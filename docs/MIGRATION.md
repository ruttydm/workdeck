# TUI-only split

This records the 2026-09-01 product split. The subsequent project-management cutover
uses repository-root `.workdeck/`; old `.agents/workdeck/` paths below describe the
prototype retained at that time. See the [current migration and compatibility
contract](project-management-compatibility.md) and [standalone implementation
plan](project-management-implementation-plan.md) for current behavior and acceptance status.

On 2026-09-01, Workdeck returned to a single terminal product surface and all graphical application work moved to the private Aya repository.

## Moved to Aya

- the Rust/Dioxus desktop and web workbench;
- its domain, database, Git, GitHub, analysis, artifact, API, presenter, and application-service crates;
- desktop packaging, icons, fonts, component assets, visual tests, screenshots, release metadata, and UI documentation;
- the former embedded Workdeck web UI, retained in Aya as migration reference.

Aya keeps its Swift-native application as its primary implementation. The moved Dioxus workspace is an Aya experiment and is independently buildable there.

## Removed from Workdeck

- `Workdeck.app` and the `workdeck-app` helper;
- every graphical support crate and asset;
- `workdeck web` and `workdeck --web`;
- desktop, browser, accessibility, visual-regression, and GUI packaging automation.

## Retained in Workdeck

- the `workdeck` Ratatui TUI;
- headless JSON and JSONL commands;
- repo-local issues, handoffs, and imported session metadata (then under `.agents/workdeck/`);
- Cargo and Homebrew installation, terminal release packaging, CI, and soak checks.

Workdeck does not depend on Aya or Herder private storage. Git and explicit versioned contracts are the integration boundary. See [Product Boundaries](PRODUCT_BOUNDARIES.md).
