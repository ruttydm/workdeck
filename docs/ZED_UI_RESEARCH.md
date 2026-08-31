# Zed, GitKraken, and Codex UI research and Workdeck decisions

Status: historical GPUI prototype research. The shipping desktop workspace now uses Dioxus and rejects GPUI/Zed dependencies in its release graph.

This note records the source-level review and framework decision used for the former Workdeck GPUI prototype. Statements below describing direct GPUI use are historical and do not describe the current shipping graph.

## Inspected source

- Repository: `zed-industries/zed`
- Current source audit: `bc6095f2c0addabc37e2e8cb761adbe240d031ac`
  (2026-08-16)
- Reproducible GPUI build commit: `cc053a4a6fa2fd0e8793201ed9099466af1be0b1`
- gpui-component commit: `da4f93696dc2b2b4d91bcc42412b9053a3d24de8`
- Areas inspected: GPUI window/entity/list APIs and Zed's workspace, project-panel, picker, command-palette, Git, theme, and settings patterns

The repository is primarily GPL-3.0-or-later. The inspected `ui`, `workspace`, `project_panel`, `picker`, `command_palette`, `git_ui`, `theme`, and `settings_ui` crates declare GPL-3.0-or-later. The `gpui` crate declares Apache-2.0.

Workdeck directly uses the Apache-2.0 GPUI, gpui-component, and gpui-wry crates through pinned Git revisions. It does not copy Zed editor UI, icons, themes, or assets. The selected GPUI graph currently includes the GPL-3.0-or-later `zlog`, `ztracing`, and `ztracing_macro` crates; `deny.toml` makes those exceptions explicit. Local prototype use is covered, but any redistributed binary requires a GPL compliance review rather than assuming the workspace's MIT declaration is sufficient.

## What makes the interface feel fluent

### Rendering is never the work queue

Zed keeps filesystem, matching, and derived-list work away from the foreground render path. Workdeck follows that architecture with one dedicated service thread and typed request channels. GPUI receives immutable workspace, deck, and unit snapshots; capture, provider, database, and Git operations never run while rendering.

### Focus and selection are stable application state

Zed's workspaces, panels, pickers, and editors explicitly own focus handles and selected indices. Refreshing data does not imply resetting the user's context. Workdeck now keys deck and review-unit selection by typed IDs, preserves those IDs across asynchronous refreshes, and falls back to the highest-priority open unit only when the previous selection no longer exists.

### Commands are contextual and discoverable

Zed's picker installs a key context and dispatches actions such as next, previous, confirm, cancel, and preview changes. Workdeck maps context-specific review actions into the standard macOS menu bar and toolbar, with discoverable keyboard shortcuts and system search placement.

### Dense lists still have structure

Zed's dense rows combine indentation, selection, focus, hover treatment, secondary status, and accessibility metadata. Workdeck uses gpui-component primitives with stable accessibility IDs and a virtualized mixed-height semantic tree, so decks with hundreds or thousands of agent-created units remain responsive.

### Elevation and typography are semantic

Zed distinguishes app background, surface, editor surface, elevated surface, and modal surface. Workdeck delegates those categories to macOS semantic colors and materials so light mode, dark mode, contrast, vibrancy, active-window state, and accent colors behave like the rest of the system. Source content remains monospaced while UI text uses the system face.

### The workspace has layers, not pages

Zed composes activity navigation, docks, panes, tabs, editors, overlays, and status UI rather than navigating through full-screen pages. Workdeck similarly keeps project/repository context, the review inbox, semantic tree, reading surface, inspector, and status ledger visible in one continuous workspace.

### Diff is a document, not a stack of cards

Zed's multi-diff view builds one multibuffer, registers file excerpts and hunks, expands diffs, and gives the editor full focus. Workdeck now renders the selected review unit as a focused source document with stable old/new line gutters, semantic insertion/deletion backgrounds, path and change statistics, and a full Diff view instead of appending every unit's diff into one long feed.

## Workdeck-specific extension: the review ledger

Workdeck adds a concept Zed does not need: a durable human review boundary while agents continue changing sources.

Every deck surface exposes:

- the current immutable checkpoint revision;
- the latest fully reviewed checkpoint;
- reviewed and open unit counts;
- live sources that advanced after capture;
- per-unit transition and inherited-review state;
- checkpoint history with exact open counts.

Live advances never rewrite the earlier review result. They raise attention on the deck until the reviewer deliberately captures a new checkpoint.

## GPUI implementation decision

Workdeck now uses GPUI directly. Builds select the full Xcode developer directory so Metal shaders compile even when the machine-wide `xcode-select` still points at Command Line Tools. Exact revisions are locked and asserted by the quality script because both GPUI and gpui-component are pre-1.0 and move quickly.

The framework-specific surface is intentionally limited to `workdeck-ui` and `workdeck-desktop`. Domain, catalog, Git, analysis, GitHub, artifact, and core crates remain UI-independent. The presenter boundary means another renderer can be added without changing the review ledger or introducing a command-line JSON bridge.

## GitKraken and Codex reference decisions

The August 2026 visual pass also checked the current official GitKraken
Desktop interface, command-palette, shortcut, and Launchpad references, plus
OpenAI's official Codex app launch and desktop migration guidance:

- <https://help.gitkraken.com/gitkraken-desktop/interface/>
- <https://help.gitkraken.com/gitkraken-desktop/command-palette/>
- <https://support.gitkraken.com/gitkraken-desktop/keyboard-shortcuts/>
- <https://help.gitkraken.com/gitkraken-desktop/gitkraken-launchpad/>
- <https://openai.com/index/introducing-the-codex-app/>
- <https://help.openai.com/en/articles/20001276/>

Workdeck adopts GitKraken's collapsible reference hierarchy, legible graph
lanes, selectable WIP/commit rows, contextual inspector, two-point range
selection, and keyboard-first switching. It deliberately omits Git mutation
controls: portfolio repositories remain review-only inputs.

From Codex, Workdeck adopts the project/task siderail, content-first review
surface, diff beside task context, worktree awareness, compact contextual
actions, and artifact-in-workspace model. It does not reproduce the chat-first
information architecture: Workdeck's primary object is the durable human
decision boundary across agent bursts, repositories, plans, PRs, and CI.

The combined density rule is simple: one destination row, one contextual
review row, progressive disclosure for infrequent actions, and a canonical
tree immediately to the right of the document. This is why the earlier
four-row review header and always-expanded portfolio tree were removed.
