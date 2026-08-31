# Workdeck: Dioxus Rewrite and Production Cutover

Status: implemented baseline and historical completion specification; current authority lives in the README and product-boundary document
Repository: `ruttydm/workdeck`
Product: Workdeck
Application bundle: `Workdeck.app` (`app.gingermedia.workdeck`)
Executables: `workdeck-desktop`, `workdeck-app`; packaged with the primary `workdeck` TUI

## 1. Product outcome

Workdeck is a desktop-first development activity environment for a world in which coding agents move repositories faster than people can follow them. The product makes commits, pull requests, CI, artifacts, repositories, and linked worktrees legible across a portfolio without introducing a parallel deck or checkpoint taxonomy.

The final cutover is a Rust/Dioxus application using the macOS system WebView. The renderer also builds for the web against deterministic fixtures, enabling ordinary browser automation and visual iteration. Hosted and mobile implementations are not part of this delivery, but the renderer-independent API boundary must make them possible without another UI rewrite.

## 2. Immutable constraints

- Keep the repository folder and remote name `workdeck`.
- Repositories selected for desktop review are read-only input. State-changing tests use isolated temporary repositories.
- All state-changing tests use temporary repositories and `WORKDECK_DATA_DIR` catalogs.
- The desktop begins with a fresh machine-local catalog, currently schema 2, and never reads, renames, or deletes the TUI's repo-local `.agents/workdeck/` data.
- Native final QA uses Codex Computer Use only. Never invoke `cua-driver`.
- Shipping renderer code is Rust. No hand-authored JavaScript or TypeScript, `eval`, `use_eval`, Node runtime, Canvas program, or unreviewed browser package enters `Workdeck.app`.
- Git and provider operations are read-only. No mutation controls are permitted.

## 3. Naming and workspace architecture

The active workspace contains unpublished crates:

```text
workdeck-api         renderer-independent serialized protocol
workdeck-domain      durable activity cursors and semantic evidence types
workdeck-db          schema-2 SQLite catalog and integrity checks
workdeck-git         read-only discovery, status, graph, range, and search
workdeck-analysis    tree-sitter units, calls, AST, canonical trees, highlighting
workdeck-github      bounded read-only GitHub provider
workdeck-artifacts   secure import and exact-origin preview helper
workdeck-core        application paths and authoritative catalog service
workdeck-presenter   single-writer runtime and LocalWorkdeckClient
workdeck-ui          Dioxus renderer and fixture-backed web target
workdeck-desktop     macOS application host
workdeck-app-cli     JSON automation for the desktop catalog
workdeck-cli         primary terminal workbench and headless repository commands
```

Desktop packages are `publish = false`; `workdeck-cli` remains independently installable. Product and source identifiers consistently use Workdeck.

## 4. Renderer boundary

`workdeck-ui` imports only `workdeck-api` plus renderer libraries. It may not import SQLite, Git, GitHub, filesystem, artifact-server, or native-dialog crates.

```rust
pub trait WorkdeckClient: Clone + 'static {
    fn request(
        &self,
        request: WorkdeckRequest,
    ) -> Pin<Box<dyn Future<Output = Result<WorkdeckResponse, WorkdeckError>>>>;

    fn subscribe(&self) -> WorkdeckEventStream;
    fn cancel(&self, operation: OperationId);
}
```

Every response envelope includes `RequestId` and source `Revision`. Async consumers ignore obsolete requests/revisions. Protocol models include portfolio, unread commit/PR activity, workspace hierarchy, Git graphs, change evidence, pull requests, CI, artifacts, grouped search, preferences, errors, and task progress. Semantic analysis records may remain internal evidence, but no public workflow requires a deck, checkpoint, or per-file completion state.

Two implementations are required:

- `LocalWorkdeckClient`: bounded in-process requests to the native runtime, central single-writer SQLite ownership, bounded workers, cancellation, and generation-safe events.
- `FixtureWorkdeckClient`: deterministic polished, empty, offline, loading, partial, unavailable, cancelled, and failure states for browser iteration.

## 5. Runtime and data

Workdeck uses `ProjectDirs::from("app", "Ginger Media", "Workdeck")`, `WORKDECK_DATA_DIR` for isolation, and `workdeck.sqlite3`. Objects, artifacts, logs, caches, and preferences are central.

The runtime owns:

- all SQLite writes and activity read-cursor transitions;
- read-only repository opening and command validation;
- fixed-capacity request queues and worker pools;
- incremental inbox, search, syntax, provider, Git, and artifact work;
- cancellation handles and bounded results;
- monotonic revisions and task events;
- helper startup/shutdown and loopback lifetime.

First launch is an empty catalog with a root picker. Existing `~/Projects` and `~/Sites` directories are suggested when present. Discovery is idempotent and identifies repository identity separately from checkout and linked worktree. The 74-project/75-repository/109-worktree catalog is a deterministic scale fixture, never a forced live invariant.

## 6. Stack and license policy

- Dioxus `0.7.1`
- Dioxus Desktop `0.7.10`
- Dioxus CLI `0.7.9`
- Dioxus Components revision `bf007c15d0cf4d04d3181cc46cf12325aa773955`
- dioxus-free-icons `0.10.0`, Lucide only
- Tailwind standalone `4.3.3`
- rfd `0.17.2`
- Tao full-size macOS content/titlebar integration

Source-owned styled components adapt pure-Rust primitive semantics from Dioxus Components under MIT. Upstream focus-trap JavaScript is not copied or linked. Comet/Zeron, T3 Code, and Orca are permissive design/interaction references with component-level provenance. Waku is GPL behavior-only inspiration. GitKraken and Codex are visual/behavioral references only.

## 7. Design system

### Tokens

- Geist for UI; Geist Mono for code and identifiers.
- Four-point spacing grid.
- Graphite neutral surfaces and restrained sage/teal selection.
- One-pixel separators; 6/8/10 px radii.
- Semantic success, warning, danger, incoming, and diff roles.
- No gradients, oversized cards, or decorative dashboard chrome.
- Motion 100–180 ms; full reduced-motion equivalence.
- Light, dark, and system appearances with equivalent contrast.

### Chrome budget

Workdeck uses one persistent horizontal workspace header: native drag region, breadcrumb, contextual Review/Pull requests/CI tabs, refresh, command search, and progressive pane toggles in 52 px. There is no second global context-toolbar row. Content-owned filters remain inside the surface they affect.

### Geometry

```text
rail        44 px fixed
header      52 px
navigator  256 px default, 208–360 resizable/collapsible; Workspaces only
inspector  320 px default, 240–720 resizable/collapsible
status      24 px
minimum    900 × 600
```

Intermediate widths turn the inspector into an overlay. Narrow widths make navigator and inspector mutually exclusive drawers. Content always owns remaining width. Pane widths, collapsed state, tabs, selections, and scroll positions persist; stale snapshots do not.

## 8. Shell mockup

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   Workdeck / sampleapp       [Review] [Pull requests] [CI]    search ⌘K │
├────┬──────────────────────┬────────────────────────────────┬───────────────┤
│ In │ Projects             │ Changes · 18    Unified ▾      │ Structure     │
│ Ws │                      ├────────────────────────────────┤               │
│ Git│   SampleApp    8     │                                │ ▾ src         │
│ Sr │   Workdeck           3     │       review content           │   ▾ services  │
│    │   Nomad         2     │                                │     file.rs   │
│    │                      │                                │               │
│    │ Today                │                                │ Evidence      │
│    │   PR #42       5     │                                │ Checks · 3/4  │
│    │   Burst        2     │                                │ Comments · 2  │
│ Ar │                      │                                │               │
├────┴──────────────────────┴────────────────────────────────┴───────────────┤
│ Ready · portfolio current                         background tasks · 2     │
└────────────────────────────────────────────────────────────────────────────┘
```

The rail owns Updates, Workspaces, Git, Search, and bottom-anchored Artifacts. Commits, Pull requests, and CI are contextual workspace tabs, never duplicate rail destinations. Changes opens only from a commit range or PR. Only Workspaces exposes the compact project navigator; detailed hierarchy remains in its workbench. The command palette and Command-1…5 shortcuts expose the same global navigation.

## 9. Component inventory

Generic source-owned components:

- button/icon button; dialog/alert dialog; tooltip/popover;
- dropdown/context menu; tabs; select/combobox;
- checkbox/switch; collapsible; scroll area;
- toast/progress; separator/skeleton; toolbar; virtual list.

Workdeck components:

- application rail, project navigator, one-row workspace header, contextual tabs, status bar;
- resizable panes, responsive drawers, command palette, task center;
- treegrid, changed-file tree, Git graph, unified/split diff, source renderer;
- Markdown renderer, call flow, canonical structure, AST;
- update row, unread dot, branch commit burst, read cursor;
- PR master/detail, CI explorer/log viewer, artifact frame.

Every visible control must be implemented, removed, or honestly unavailable with actionable explanation. Pointer, Enter/Space, focus ring, accessible name, selected/expanded state, Escape/Back, and focus restoration are part of each component’s acceptance criteria.

## 10. Feature surfaces

### Updates

Updates answers only “what changed since I last looked?” It groups commits by branch and provider activity by pull request. The default view is Unread; All provides recent history. Opening or explicitly marking an item read persists its exact opaque source revision. A later commit, push, comment, or status update makes it unread again.

Scanning is incremental, bounded, cancellable, and visibly progressive. Unavailable worktree history is retained. A giant repository cannot block the interface or monopolize priority.

### Workspaces

Treegrid hierarchy:

```text
Project
  Repository identity
    Checkout
      Worktree
```

Rows provide real disclosure, indentation, availability, branch, attention, last-seen, retained/prunable status, aligned columns, search, expand/collapse all, stable selection, and keyboard semantics.

### Search

Search covers projects, repositories, checkouts, worktrees, files, symbols, Markdown headings, commits, branches, PRs, CI, and artifacts. It is debounced, cancellable, grouped, bounded, and keyboard navigable. Saved searches and recent activated searches persist separately. Unavailable results route to retained history rather than dead ends.

### Changes

Lenses: unified diff, split diff, source, rendered Markdown, call flow, canonical structure, AST, artifact preview.

Requirements:

- Rust/tree-sitter syntax spans for every supported grammar;
- safe pulldown-cmark-to-RSX rendering; no untrusted HTML injection;
- canonical tree directly to the right of content;
- virtualized file/source/diff/tree/ledger lists;
- context and whitespace controls, copy/selection, next/previous change;
- moved and formatting-only states;
- commit/PR-owned read state; no file-by-file completion controls;
- per-tab/unit/mode/layout scroll restoration.

### Git

Rust computes topology and SVG for visible rows only. Git includes accurate lanes/merge lines, WIP, local/remote branches, tags, stashes, HEAD/detached HEAD, filtering, incremental history, inspector metadata, parent relationships, two-point range comparison, and selection/scroll restoration. It exposes no mutation controls.

### Pull requests

Dense T3-inspired master/detail layout with Unread/Open/Merged/Closed filters, Summary, Timeline, Code, Checks, Comments, Commits, Changed files, and durable activity read state. External GitHub links are validated; all provider mutation is excluded.

### CI

Runs, jobs, steps, progressive bounded logs, partial data, retry, and local load cancellation. Workdeck never retries or mutates provider runs.

### Artifacts

ZIP import rejects traversal, absolute paths, symlinks, duplicate destinations, bombs, excess size/count, and unsupported layouts. Preview is loopback-only, exact-origin, CSP restricted, sandboxed, and has no filesystem URL, external network, or app-runtime bridge. Helper lifetime is tied to Back, Escape, tab/window close, and quit.

## 11. CLI

```text
workdeck catalog scan|refresh|list|show
workdeck updates list|read-commit|read-pr
workdeck git status|changes|commits|graph
workdeck search
workdeck github prs|pr|runs|run|jobs|logs|artifacts
workdeck artifact import|list|inspect|open
workdeck doctor
workdeck fixture
```

All automation commands support `--json`, stable exit codes, and read-only repository behavior. TUI, embedded web UI, issues, sessions, cycles, labels, and project-management commands are removed. The temporary `workdeck` executable delegates supported commands, prints a deprecation warning, and never resurrects deleted commands. `.agents/workdeck` remains untouched.

## 12. Web iteration and visual QA

The fixture-backed web target is the normal UI development loop:

- component gallery;
- query-addressable fixture and surface routes;
- deterministic fonts, clock, provider state, progress, and content;
- Playwright pointer and keyboard tests;
- Axe accessibility tests;
- light/dark/system and reduced-motion snapshots;
- minimum/intermediate/wide/ultrawide viewports;
- modal, empty, loading, offline, partial, unavailable, cancelled, and error cases;
- Rust SSR structure and reducer/hook tests.

The visual matrix contains at least ten primary surfaces × four viewport classes × light/dark, plus critical modals and failure states. Snapshot updates require deliberate review.

## 13. Native integration

- full-size macOS content and transparent titlebar via Tao;
- native app/edit/view/window/help and contextual menus;
- standard macOS shortcuts, dialogs, Dock icon, About panel;
- validated external URLs and justified secondary windows;
- process-lifetime native menu clone to work around Dioxus Desktop issue #5753;
- repeated secondary-window close/menu-use regression;
- clean worker and artifact-helper shutdown.

After web gates pass, build and ad-hoc sign exact `dist/Workdeck.app`, record its executable hash, and use Codex Computer Use only to validate titlebar, menus, shortcuts, focus, dialogs/file panels, WebView accessibility, appearance, minimum size, artifacts, secondary windows, close, and quit. Evidence manifests must bind to that exact hash.

## 14. Performance budgets at 74/75/109 scale

| Measure | Budget |
|---|---:|
| cold usable launch | ≤1.2 s |
| warm shell paint | ≤350 ms |
| first actionable Inbox content | ≤300 ms |
| complete incremental Inbox scan | ≤7 s |
| cancellation acknowledgement | ≤150 ms |
| cached Git graph | ≤250 ms |
| cold Git graph | ≤750 ms |
| area switch p95 | ≤50 ms |
| first diff viewport | ≤250 ms |
| cached search result after debounce | ≤50 ms |
| scrolling main-thread work p95 | ≤8 ms |
| idle memory after fixture load | ≤350 MB |

Virtual lists retain visible rows plus no more than four viewport heights of overscan.

## 15. Implementation phases and exit criteria

### Phase A — foundation and early replacement

- Rename active crates/source identity; add `workdeck-api`, local/fixture clients, fresh state paths/schema.
- Render a real portfolio snapshot in Dioxus.
- Switch default build and packaging to Workdeck; freeze GPUI as non-shipping parity source.
- Add logging, crash-safe persistence, task registry, component gallery, original icon, favicon, and T3 metadata.

Exit: active workspace and `wasm32` compile without GPUI in the dependency graph.

### Phase B — shell and interaction system

- Rail, navigator, tabs, toolbar, inspector, status/task center, splitters, responsive overlays, menus, command palette, shortcuts, focus restoration, preferences.
- Complete loading/empty/offline/partial/unavailable/cancelled/error states before feature density.

Exit: every shell control is functional via pointer and keyboard at 900×600 through wide layouts.

### Phase C — portfolio workflows

- Workspaces hierarchy, Updates scanning/read cursors, Search index/history/routing.

Exit: 74/75/109 fixture remains responsive, idempotent, and fully navigable.

### Phase D — changes and Git

- Complete change lenses, syntax, safe Markdown, virtualization, Git topology/range comparison, and commit/PR read-state integration.

Exit: fixture and isolated real Git comparisons cover changed/moved/format-only/removed behavior, while activity revisions reopen unread state.

### Phase E — provider and artifacts

- PR, CI/logs, artifact downloads/import/security/preview/lifecycle.

Exit: deterministic provider fixtures, live read-only probes, and artifact attack corpus pass.

### Phase F — native/CLI and cutover

- Complete CLI JSON contracts, menus/dialogs/shortcuts/titlebar/About, menu lifetime regression, packaging/signing.
- Delete GPUI UI/desktop, compatibility crates, Zed/Longbridge dependencies, Swift shell, and old Workdeck assets/identifiers.

Exit: only Workdeck ships; repository path remains unchanged.

## 16. Deterministic defect loop

For each defect:

1. reproduce and capture evidence;
2. identify the underlying cause;
3. implement the complete fix;
4. add a deterministic regression test or harness;
5. rebuild/repackage the exact product;
6. rerun affected browser and packaged-native flows in light and dark.

Do not hide defects with retries, arbitrary delays, enlarged limits, silent fallback, cosmetic overlays, or permanently disabled controls.

## 17. Final gate matrix

- `cargo fmt --all --check`
- locked workspace, desktop, web, all-target, all-feature, and WASM checks
- all unit/integration/security/schema/fixture/UI tests
- Clippy and rustdoc with warnings denied
- cargo-deny advisories/licenses/sources
- Tailwind version/checksum/output verification
- provenance, copied-source, and license bundle checks
- no-shipped-JavaScript and render-thread-I/O gates
- accessibility contract
- Playwright interaction, Axe, and visual suites
- CLI smoke/JSON contracts
- 74/75/109 scale fixture and fresh live discovery report
- read-only Git mutation guard and live read-only GitHub probes
- SQLite integrity/foreign keys and artifact security corpus
- menu lifetime, packaging, strict deep codesign, helper shutdown
- exact-package Codex Computer Use matrix

Completion requires signed/runnable `dist/Workdeck.app`, packaged `workdeck`, fresh Workdeck discovery, full review/Git/PR/CI/artifact workflows, no GPUI dependency or old shell, no unapproved license material, zero known P0/P1 defects, and no unexplained P2 defects. Only Developer ID signing, notarization, and distribution credentials may remain external.
