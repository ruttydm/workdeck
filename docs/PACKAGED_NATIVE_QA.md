# Workdeck Packaged Native QA

Native QA targets exactly `dist/Workdeck.app`, never a Cargo-run process or stale bundle. Every accepted evidence manifest must record the current executable SHA-256 and bundle ID `app.gingermedia.workdeck`.

## Current status after product split

The source, browser, accessibility, and packaging gates remain reusable after the rename. Previous exact-package native evidence was captured from an Aya-branded executable and is historical only; it cannot validate `Workdeck.app`. Local legacy files are quarantined under ignored `artifacts/legacy-aya-evidence/` and are not release evidence.

Fresh Workdeck-native interaction and visual manifests are therefore pending.

## Driver policy

Use Codex Computer Use for the exact native interaction matrix. Do not substitute browser automation, AppleScript UI scripting, or coordinate-only automation for native package evidence.

## Before capture

1. Pass Rust, protocol, security, web interaction, Axe, and visual gates.
2. Package and ad-hoc sign `Workdeck.app`.
3. Verify the bundle ID and strict deep signature.
4. Snapshot read-only portfolio Git-state digests outside the repository.
5. Quit existing Workdeck processes and launch the exact bundle.

## Required visual matrix

Capture Updates, Workspaces, Git, Search, Pull requests, CI, Artifacts, and Review in light and dark appearance, including minimum and wide layouts. Inspect focus, clipping, selection, typography, diff and syntax contrast, progress/error/empty/offline states, canonical-tree indentation, and reduced motion.

## Required interaction matrix

- Application, Edit, View, and Window menus.
- Direct navigation shortcuts, command palette, Escape, editing, close, minimize, and fullscreen where safe.
- Rail and viewport-tab pointer activation.
- Navigator and changed-file pane resize, collapse, reset, and minimum-width behavior.
- Commit, PR, CI, artifact, search, and semantic-review navigation.
- Native folder and artifact chooser open/cancel with focus restoration.
- Artifact helper shutdown, window close, reopen, and clean application quit.

No test may modify a reviewed repository. File-panel and provider flows are cancel-only or use isolated temporary fixtures.

## Evidence paths

- `artifacts/native-visual-manifest.json`
- `artifacts/native-interaction-manifest.json`
- `artifacts/performance/workdeck-packaged.json`
- `artifacts/native-visual/`
- read-only repository digests captured before and after QA

The `artifacts/` directory is intentionally ignored. Native evidence is release-specific and must be archived with the matching package rather than treated as timeless source evidence.
