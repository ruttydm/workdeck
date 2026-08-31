# Workdeck UI/UX Redesign Blueprint

Status: historical GPUI redesign blueprint; superseded by the Dioxus implementation and current product boundary

Date: 2026-08-25

Scope: archived clean-sheet redesign of the former native Rust/GPUI macOS prototype

Implementation status: **plan only; no redesign implementation is authorized by this document alone**

Current authority: [README](../README.md), [Product Boundaries](PRODUCT_BOUNDARIES.md), and [Dioxus implementation](WORKDECK_DIOXUS_IMPLEMENTATION_PLAN.md)

## 1. Executive decision

Workdeck must become a calm review operating system, not a dense Git dashboard and not a collection of disconnected feature tabs.

The product exists to answer five questions in order:

1. What genuinely requires my decision now?
2. What changed since the last point I understood?
3. What is the smallest coherent unit I can review next?
4. What evidence explains that change across code, plans, commits, PRs, CI, and artifacts?
5. When I return later, where exactly did I leave off?

The redesign therefore adopts this shell:

- A narrow **global siderail** switches durable product areas.
- A contextual **navigator sidebar** shows the hierarchy for the selected area.
- A single **viewport tab strip** contains actual open review contexts, Git views, searches, and artifacts—not static product features.
- One **context toolbar** contains breadcrumbs, the local lens switcher, and the primary decision action.
- The central **workbench** is content-first.
- A right-side **Atlas/Inspector pane** shows the canonical tree, outline, evidence, or metadata and is immediately adjacent to the reviewed content.
- No workflow may create more than two persistent horizontal chrome rows: the viewport tab/title row and the context toolbar.

This changes the mental model from:

```text
destination → review set → seven feature tabs → checkpoint row → file title → view tabs
```

to:

```text
area → open context tab → selected review unit → synchronized lens + evidence
```

The internal Deck/Atlas/Lens model remains valuable. The public UI uses plain language:

| Internal term | User-facing term | Meaning |
| --- | --- | --- |
| Deck | Review | A durable review target with a frozen checkpoint |
| Atlas | Structure | The canonical cross-linked hierarchy and evidence map |
| Lens | View | Diff, Source, Rendered, Calls, Structure, Run, or Artifact |
| ReviewUnit | Review item | The smallest stable semantic unit that can carry understanding |
| Snapshot | Checkpoint | The frozen content boundary being reviewed |

## 2. Non-negotiable product invariants

The redesign may replace every visual component and navigation pattern, but it must preserve these behaviors:

- The catalog is global, not bound to the process working directory.
- The verified production catalog baseline is 74 projects, 75 repository identities, 109 worktrees, seven intentional empty projects, and four retained unavailable/prunable worktrees.
- Project discovery, repository identity resolution, linked-worktree discovery, migration, refresh, and restart are idempotent.
- Repositories selected for desktop review are read-only inputs.
- Workdeck state stays in the machine-local application catalog, never in reviewed repositories.
- A review freezes a checkpoint while agents continue working.
- New commits, PR updates, CI runs, documents, and artifacts appear as incoming evidence without silently moving the checkpoint.
- Unchanged semantic review items carry understanding forward; changed items reopen; moves, format-only changes, and uncertain matches remain explicit.
- A path, SHA, branch name, PR number, or CI run number is evidence—not durable review identity.
- Git parsing, indexing, diffing, AST work, call analysis, provider requests, and scanning run in cancellable background work. GPUI renders cached immutable state.
- Static call-flow inference is never labeled as an observed runtime stack.
- HTML artifacts remain loopback-only, origin restricted, traversal safe, CSP hardened, and tied to application lifetime.
- Every visible control works, has an honest unavailable state, or is removed.
- Light, dark, system appearance, reduced motion, keyboard use, and narrow windows are first-class.

## 3. Investigation record

### 3.1 Current Workdeck evidence

The investigation covered the exact current packaged `dist/Workdeck.app`, the Rust workspace, existing product documents, the presenter/domain model, packaging gates, and the production catalog assumptions.

Observed product strengths:

- The semantic review model is materially more valuable than a conventional Git client.
- SampleApp and Workdeck active reviews are already represented as distinct durable review sets.
- The canonical review tree is correctly placed to the right of the content.
- The app already has read-only Git history, WIP, references, range selection, syntax highlighting, source/Markdown/calls/AST modes, GitHub attachments, CI, and sandboxed artifacts.
- The worker-thread boundary is directionally correct.
- The existing native app has real keyboard labels, progress state, durable preferences, and packaging infrastructure.

Observed structural UX problems:

- `crates/workdeck-ui/src/lib.rs` contains roughly 10,738 lines and owns application state, navigation, rendering, async orchestration, components, dialogs, diff construction, highlighting, accessibility labels, and utility functions in one module.
- The Review destination can stack four navigation/chrome rows: global feature tabs, checkpoint/source state, item identity, and local view tabs.
- Static feature tabs (`Overview`, `Changes`, `Commits`, `Plans`, `Pull requests`, `CI`, `Artifacts`) compete with global destinations and with per-item views.
- The sidebar, screen header, and center cards often repeat the same project/review identity.
- Inbox cards are too tall, repeat `Graph`, `Review`, and `More`, and make a large portfolio feel like an undifferentiated wall.
- Raw changed-file counts attract more attention than the reason a reviewer should act.
- Inventory and decisions are mixed: 109 worktrees are useful in Workspaces, but harmful as a flat decision queue.
- Context is frequently expressed as text rows instead of stable spatial structure.
- Dense Git/review surfaces and spacious overview cards do not feel like the same product.
- Selection, pane, and tab concepts exist, but their hierarchy is not obvious from visual weight.
- The single-module UI makes fine-grained rendering invalidation, visual ownership, testing, and iterative polish harder than necessary.

### 3.2 Comet investigation

Local reference:

```text
inspiration/comet
origin: https://github.com/zeronsh/comet
audited commit: 2ebe6ed06f8e7e1ed911c0adf31ec91ad2e94274
commit subject: v0.2.29
license: MIT
```

High-value findings:

- The sidebar is the durable data hierarchy; horizontal tabs are a device-local viewport onto opened contexts.
- Closing a tab does not destroy/archive the underlying entity.
- A unified app-owned titlebar carries traffic-light spacing, sidebar toggle, history, add action, and the tab strip without an extra header.
- Pane collapse animates a clipped outer width while the inner content retains stable geometry, preventing mid-transition reflow.
- The UI has explicit motion primitives, reduced-motion behavior, edge fades, frosted/layered surfaces, popovers, skeletons, and bounded syntax caches.
- Large transcripts, diffs, and history are virtualized.
- Diff rendering is decomposed into file, hunk, line, gutter, comment, and split-row concepts.
- History owns real graph lane layout, reference badges, WIP, pagination, and width budgeting.
- Theme colors are paint; layout metrics stay stable across themes.
- Render paths read cached state while background work performs expensive operations.

Candidate Comet sources are cataloged in [Component provenance and reuse](#11-component-provenance-and-reuse).

### 3.3 Waku investigation

Local reference:

```text
inspiration/waku
origin: https://github.com/egoist/waku
audited commit: cc8b2cb0ffe9074c7a622e0b2caee22d708ed307
commit subject: Chain nested scroll only at boundaries
license: GPL-3.0-only
```

High-value findings:

- Resizable side and right panels slide with stable content widths and explicit cached pane islands.
- Large lists are virtualized with uniform rows and bounded reveal batches.
- Sidebar state includes grouping, ordering, collapse, search, hierarchy guides, recency, status, and selection restoration.
- The right panel is a local tabbed surface with per-session persistence, dirty state, close behavior, horizontal overflow fades, selected-tab reveal, and context-aware default width.
- Command palette search is debounced, cancellable in effect, grouped, keyboard complete, and preserves previous results while a new search is pending.
- Task switching uses a bounded visual grid with recent-context ordering.
- Background work is summarized, bounded, and refreshed on controlled cadences.
- Keyboard parity, focus visibility, reduce-motion support, and “no I/O in render” are explicit engineering requirements.
- Native webviews have special compositing and lifecycle handling rather than being treated like ordinary GPUI elements.

Because Waku is GPL-3.0-only, these are behavioral observations only. No Waku source, layout constants, theme values, icons, fonts, assets, tests, strings, or component implementations may be copied into a proprietary-capable Workdeck branch.

### 3.4 External primary-source findings

Apple’s current guidance supports:

- sidebars for broad, flat navigation;
- disclosure controls to bound large hierarchies;
- no more than two sidebar hierarchy levels before introducing another split-view pane;
- a user-visible Show/Hide Sidebar command;
- automatic collapse at narrow widths;
- thin, draggable split dividers with sensible minimum sizes;
- toolbars reserved for a small number of contextual actions, with less common actions in overflow;
- menu-bar equivalents for toolbar commands;
- adaptive layouts across wide, intermediate, and narrow windows.

Git-client research reinforces:

- a graph with real merge lanes, references, WIP, filtering, and inspector detail;
- a WIP node per worktree when multiple agents/worktrees are active;
- opening worktrees/reviews in separate tabs rather than replacing context;
- portfolio Launchpad/Inbox behavior with pinning, snoozing, saved views, and actionable grouping;
- clear separation between repository inventory and actionable pull requests/WIP.

These patterns are inputs, not the product thesis. Workdeck’s differentiator remains durable semantic review across agent bursts.

## 4. Licensing and commercial-product posture

This section is an engineering policy, not legal advice. Before commercial distribution, counsel should confirm the final dependency and asset graph.

### 4.1 Reuse classes

Every inspired element must be assigned one class before implementation:

| Class | Meaning | Allowed action | Required evidence |
| --- | --- | --- | --- |
| A — direct permissive reuse | Source is MIT/Apache-2.0/BSD/ISC or another approved permissive license | Copy and adapt source | Pinned upstream revision, exact source paths, preserved copyright/license notice, diff review |
| B — permissive pattern adaptation | Source is permissive, but Workdeck’s architecture differs | Reimplement with traceable reference | Design note, upstream license, test proving Workdeck behavior |
| C — independent behavior implementation | Source is GPL or otherwise incompatible, but behavior is publicly observable | Specify behavior, then implement without copying code/assets/constants | Behavior spec written before implementation, separate provenance record, reviewer attestation |
| D — excluded | License/provenance is unclear, trademarked, noncommercial, or incompatible | Do not ship | Explicit exclusion in the ledger |

### 4.2 Repository rules

- `inspiration/` is ignored by Workdeck Git and excluded from every Cargo workspace, package, archive, app bundle, test fixture, and generated source scan.
- Do not add path dependencies pointing into `inspiration/`.
- Do not paste Waku source into issues, commits, tests, comments, generated files, or prompts used to implement production code.
- Comet reuse must be file-level, never “copy the UI crate.” Each adopted file receives an SPDX/source header or an entry in `THIRD_PARTY_NOTICES.md`.
- Brand marks and provider logos require separate trademark/asset review even when their surrounding repository is permissively licensed.
- Solar icons in Comet are CC BY 4.0 and require attribution. Prefer gpui-component’s Lucide set or independently sourced SF Symbols-compatible custom symbols rather than importing the Comet icon set wholesale.
- Comet’s Geist/Geist Mono font files are OFL-1.1 and may be bundled with their license. Workdeck should nevertheless default to the macOS system UI font for chrome and use a bundled monospaced font only where deterministic code shaping is required.
- Waku’s JetBrains Mono and upstream Material file icons have their own permissive/OFL sources, but Workdeck must acquire them directly from upstream and record their versions if selected; never copy them from the Waku checkout.

### 4.3 Current GPUI release blocker

Workdeck currently depends on Zed GPUI at a lockfile commit that pulls `zlog`, `ztracing`, and `ztracing_macro`, each marked GPL-3.0-or-later. `deny.toml` currently carries explicit exceptions for them. The upstream GPUI crate declares Apache-2.0, but open upstream reports identify the hard transitive path through `sum_tree`.

Before any proprietary/commercial binary is distributed:

1. Remove the GPL exceptions from the release policy.
2. Move to a reviewed GPUI revision/fork where the GPL tracing path is eliminated or replaced with permissively licensed `tracing`.
3. Pin every GPUI and gpui-component dependency by exact revision in `Cargo.toml`, not only in `Cargo.lock`.
4. Run `cargo deny check licenses sources` against every target and feature.
5. Generate a machine-readable SBOM and human-readable third-party notices from the exact packaged binary graph.
6. Fail release packaging if any unapproved copyleft crate, unpinned Git source, unknown license, or untracked embedded asset appears.
7. Have legal counsel review the final graph before commercial distribution.

Neither Comet’s fork nor Waku’s fork automatically solves this: both audited lockfiles also contain the same GPL tracing crates. Their fork-specific patches are therefore design/technical references, not drop-in commercial dependencies.

## 5. First-principles product model

### 5.1 The unit of value

The user does not want to “view a repository.” The user wants to reach and record a justified decision with the least rereading possible.

The value loop is:

```text
notice → orient → choose a coherent delta → inspect evidence → decide → preserve understanding → resume
```

Every screen and control must shorten one of these transitions.

### 5.2 Core jobs to be done

| Job | Desired outcome | Common failure to eliminate |
| --- | --- | --- |
| Triage | Know what needs a decision today | 109 worktrees shown as 109 equal cards |
| Resume | Return to the exact prior context | Selection resets or live branch replaces frozen review |
| Understand | See semantic intent and impact | Raw line/file counts without causal structure |
| Compare | Inspect only what arrived since understanding | Rereading an entire rebased PR or burst |
| Validate | Correlate plan, code, PR, CI, and artifact | Disconnected tabs with unrelated cursors |
| Decide | Mark reviewed/questioned/accepted with confidence | Marks tied only to SHA/path and lost on change |
| Audit | Explain why something was accepted | No durable evidence/checkpoint trail |
| Navigate portfolio | Move among active products quickly | Repository inventory dominates active work |

### 5.3 Primary objects

```text
Portfolio
├── Project
│   ├── Repository identity
│   │   ├── Checkout
│   │   └── Worktree
│   └── Review
│       ├── Frozen checkpoint
│       ├── Incoming source advances
│       ├── Review items
│       ├── Evidence: commits / plan / PR / CI / artifact
│       └── Review history
└── Saved view / preference / provider connection
```

The UI must not force repository hierarchy and decision hierarchy into the same list.

### 5.4 Calmness rules

- Counts are secondary metadata, never the primary label.
- A badge must mean an action, state, or exception—not simply that data exists.
- Use one accent color for selection/action; reserve semantic colors for success, warning, danger, additions, and deletions.
- A row is preferred over a card unless comparison or rich preview materially benefits from a card.
- Use progressive disclosure for repositories, source metadata, full commit lists, and analysis details.
- Keep only the primary decision action persistently visible.
- Put infrequent actions in a native menu or context menu.
- Never show the same identity in the siderail, sidebar header, content heading, and status bar simultaneously.
- Empty space is useful when it protects reading width, not when it separates repeated controls.

## 6. New information architecture

### 6.1 Global areas

The global siderail contains five primary areas and two utilities:

```text
Primary                         Utility
───────                         ───────
Inbox                           Artifacts
Workspaces                      Settings
Git
Search
```

`Review` is not a rail destination. A review is an open context represented by a viewport tab. This removes the current duplicate concept of global Review destination plus review feature tabs.

Rail behavior:

- 44 px visual width; 36–40 px hit targets.
- Icon plus tooltip in expanded windows; selected state uses a quiet filled capsule or accent bar, not a large colored block.
- `⌘1` Inbox, `⌘2` Workspaces, `⌘3` Git, `⌘4` Search, `⌘5` Artifacts.
- Settings at bottom with `⌘,`.
- Badge only for actionable inbox items or failed provider state.
- At narrow width, the contextual sidebar becomes an overlay; the siderail remains.
- A user may reorder/pin primary areas later, but version one keeps stable shortcuts.

### 6.2 Viewport tabs

Viewport tabs represent open contexts:

- a durable review (`SampleApp · main`);
- a Git graph scope (`workdeck · all refs`);
- a saved search (`changed plans`);
- an artifact (`checkout.html`);
- portfolio Inbox or Workspaces home when pinned.

Tab semantics:

- Opening from the sidebar focuses an existing matching tab unless the user explicitly chooses Open in New Tab.
- Closing a tab closes only the viewport; it never deletes, archives, marks, or advances a review.
- Reopening restores last selected item, lens, pane visibility, pane widths, scroll anchors, filters, and checkpoint position.
- Tabs can be reordered, pinned, duplicated, and restored after restart.
- Overflow scrolls horizontally with edge fades and always reveals the active tab.
- Incoming changes show a small dot/count on the tab without moving focus.
- A tab’s identity is a stable `TabId` and typed target, not a label.
- `⌘W` closes the active tab; closing the last tab keeps the window open at the current area’s home.
- `⌃Tab` / `⌃⇧Tab` cycle tabs; `⌘1…9` remain area shortcuts, not tab indexes.
- `⌘⇧[` / `⌘⇧]` may also cycle tabs for Mac familiarity.

### 6.3 Contextual navigator sidebar

The second column changes by area:

| Area/context | Sidebar content |
| --- | --- |
| Inbox | Saved views, Today/Updated/Waiting/Snoozed, pinned projects, compact decision list |
| Workspaces | Project → repository summary; selected repository’s worktrees in the content pane or expandable third level only when width permits |
| Git | Worktrees, local branches, remotes, tags, stashes, saved graph filters |
| Search | Query history, saved searches, result groups |
| Review tab | Review outline: Incoming, Open, Questioned, Reviewed, evidence sources |
| Artifact tab | Artifact collection and versions |

Rules:

- Default width 252 px; minimum 208 px; maximum 360 px.
- Collapsible from titlebar control and View menu; persisted per area, not globally.
- Resize handle has an 8–10 px hit target and 1 px visible divider.
- Rows are 28–36 px depending on information density.
- Hierarchy uses 12 px indentation steps, disclosure chevrons, vertical guides only when needed, and aligned trailing metadata columns.
- Project and repository rows never center their icons/text; everything aligns to a shared leading grid.
- Only two hierarchy levels appear by default. Deeper worktree inventory is moved to the main Workspaces pane or shown on explicit expansion.
- Unavailable worktrees remain visible with a muted status and retained-history explanation.

### 6.4 Workbench and right pane

The workbench is the primary reading surface. The right pane has three mutually exclusive tabs:

- **Structure** — canonical file/semantic tree or Markdown outline.
- **Evidence** — commits, PR, CI, source advances, linked artifacts.
- **Inspect** — metadata, review history, confidence, raw identifiers, provider detail.

The selected right-pane tab is contextual and persists per viewport tab. For code review, Structure opens by default. For Git, Inspect opens by default. For a plan, Structure shows the document outline. For CI, Evidence shows jobs and logs.

Right-pane rules:

- Default 320 px; minimum 240 px; maximum 48% of the window.
- Immediately adjacent to the center content.
- Collapsible with `⌘⌥0`, toolbar button, and View menu.
- At intermediate width, it becomes an overlay sheet anchored to the right.
- At narrow width, it becomes a full-height temporary inspector and Escape returns to content.
- It may have local tabs, but never a second independent destination hierarchy.

### 6.5 Horizontal chrome budget

Persistent rows are capped:

1. **Title/tab row** — window controls, sidebar/history controls, viewport tabs, add/overflow controls.
2. **Context toolbar** — breadcrumb/identity, frozen/live status, relevant local views, primary decision action, overflow.

Everything else belongs in:

- the contextual sidebar;
- the right pane;
- a scroll-edge content header that collapses away;
- a popover/menu;
- or the content itself.

## 7. Global shell ASCII mockups

### 7.1 Wide review workspace — 1512 × 982

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ◉ ◉ ◉  ◫  ‹ ›  │ [ SampleApp · main  ●2 ] [ Workdeck · main ] [ Git: SampleApp ]                         +  ⋯  ◧ │
├────┬───────────────────────┬──────────────────────────────────────────────────────────────┬─────────────────────────────────┤
│ ▣  │ REVIEW                │ SampleApp / main / R3 → live     2 incoming      Diff Source│ Structure  Evidence  Inspect  ◧ │
│    │                       │                                          Needs attention  ⋯ │                                 │
│ ◎  │ ▾ Incoming       2    ├──────────────────────────────────────────────────────────────┤ ▾ app                           │
│    │   Auth callback       │ app/Http/Controllers/AuthController.php                     │   ▾ Http                        │
│ ◫  │   Pricing schema      │ ┌──── old ─────────────────┬──── new ─────────────────────┐ │     ▾ Controllers             │
│    │                       │ │  88  return redirect...  │  88  return redirect...      │ │       ● AuthController.php  1│
│ ⎇  │ ▾ Open          14    │ │  89- $user->save();      │                              │ │   ▾ Services                  │
│    │   BillingService      │ │                          │  89+ dispatch_sync(...)       │ │       ○ BillingService.php  3│
│ ⌕  │   Event schema        │ │  90  return $user;       │  90  return $user;           │ │   ▸ tests                      │
│    │   Crawler policy      │ └──────────────────────────┴───────────────────────────────┘ │                                 │
│    │                       │                                                              │ Selected                        │
│    │ ▸ Questioned     1    │ @@ AuthController::callback                                 │ callback() · changed            │
│    │ ▸ Reviewed      37    │                                                              │ called by routes/web.php        │
│    │                       │                                                              │ impacts BillingService          │
│    │ SOURCES               │                                                              │                                 │
│    │ ✓ Working tree        │                                                              │                                 │
│    │ ○ 5 commits           │                                                              │                                 │
│    │ ○ Plan.md             │                                                              │                                 │
│    │ ○ PR #241             │                                                              │                                 │
│    │ ○ CI #991             │                                                              │                                 │
│    │                       │                                                              │                                 │
├────┴───────────────────────┴──────────────────────────────────────────────────────────────┴─────────────────────────────────┤
│ ● Ready    8/14 reviewed · checkpoint R3 frozen 18m ago · live scan current                              Ln 89 · PHP · UTF-8 │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Notes:

- The review category list moved to the contextual sidebar.
- Frozen/live and local lenses share one toolbar instead of separate rows.
- The canonical tree is still directly right of the change.
- Counts are quiet trailing metadata.
- The only emphasized action is the current decision.

### 7.2 Intermediate width — 1180 × 760

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ◉ ◉ ◉ ◫ ‹ › │ [ SampleApp · main ●2 ] [ Workdeck ]                                +  ⋯  ◧ │
├────┬─────────────────────┬──────────────────────────────────────────────────────────────────────┤
│ ▣  │ REVIEW              │ SampleApp / main · R3 → live · 2 incoming   Diff ▾   Review ▾   ◧ │
│ ◎  │ Incoming         2  ├──────────────────────────────────────────────────────────────────────┤
│ ◫  │ Open            14  │ app/Http/Controllers/AuthController.php                             │
│ ⎇  │ Questioned       1  │                                                                      │
│ ⌕  │ Reviewed        37  │    88  return redirect(...);                                         │
│    │                    │ -  89  $user->save();                                                  │
│    │ SOURCES            │ +  89  dispatch_sync(...);                                            │
│    │ Working tree       │    90  return $user;                                                   │
│    │ 5 commits          │                                                                      │
│    │ Plan.md            │                                                                      │
│    │ PR #241            │                                                                      │
│    │ CI #991            │                                                                      │
├────┴─────────────────────┴──────────────────────────────────────────────────────────────────────┤
│ ● Ready · 8/14 reviewed · R3 frozen 18m ago                                         Ln 89 · PHP │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

The right pane is closed, but one-click/shortcut access remains visible.

### 7.3 Narrow/minimum width — 900 × 600

```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│ ◉ ◉ ◉ ◫ ‹ │ [ SampleApp · main ●2 ]                                  ⋯  ◧ │
├────┬─────────────────────────────────────────────────────────────────────────────┤
│ ▣  │ SampleApp · R3 → live · 2 incoming      Diff ▾       Review ▾       ◧ │
│ ◎  ├─────────────────────────────────────────────────────────────────────────────┤
│ ◫  │ AuthController.php                                                          │
│ ⎇  │                                                                              │
│ ⌕  │  88  return redirect(...);                                                   │
│    │- 89  $user->save();                                                          │
│    │+ 89  dispatch_sync(...);                                                     │
│    │  90  return $user;                                                           │
│    │                                                                              │
│    │                                                                              │
├────┴─────────────────────────────────────────────────────────────────────────────┤
│ ● Ready · item 3 of 14                                                Ln 89 · PHP │
└──────────────────────────────────────────────────────────────────────────────────┘
```

The navigator and right pane are overlays. The center never shrinks below a useful review width.

## 8. Surface specifications and mockups

### 8.1 Inbox — decisions, not inventory

Purpose: answer “what requires my decision now?” within five seconds.

Default groups:

1. **Continue reviewing** — open reviews with changed or questioned items.
2. **Changed since review** — semantic changes after a frozen checkpoint.
3. **Waiting on evidence** — CI/provider/agent states that block a decision.
4. **Suggested reviews** — bounded uncaptured work, ranked by explainable signals.

Uncaptured work is capped at five rows initially. A `Show all 91 candidates` action opens a searchable table; it does not expand 91 cards in place.

Priority must use bounded, explainable signals:

- pinned project/review;
- prior active review;
- semantic change since reviewed checkpoint;
- explicit question/failed CI/PR review request;
- recency bucket;
- provider failure;
- user-defined saved view.

Raw line/file counts may affect a secondary effort estimate but never dominate rank.

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│ ◉ ◉ ◉ ◫ ‹ › │ [ Inbox ]                                                         Refresh  ⋯ │
├────┬──────────────────────┬───────────────────────────────────────────────────────────────────┤
│ ▣  │ INBOX                │ Review inbox                                           2 decisions│
│ ◎  │ ● Today          2   │───────────────────────────────────────────────────────────────────│
│ ◫  │   Updated        2   │ CONTINUE REVIEWING                                                │
│ ⎇  │   Waiting        1   │ ▸ SampleApp · main       2 items changed after R3       Continue │
│ ⌕  │   Suggested      5   │   Auth callback changed · Pricing schema reopened                 │
│    │   Snoozed        3   │   8/14 reviewed · checkpoint 18m ago · CI passing                 │
│    │                      │                                                                   │
│    │ PINNED PROJECTS      │ ▸ Workdeck · main         1 incoming item                 Continue │
│    │ SampleApp       2   │   Sidebar tree changed · 41/42 reviewed                            │
│    │ Workdeck        1   │                                                                   │
│    │ Nomad               │ WAITING ON EVIDENCE                                                │
│    │                      │ ▸ PR #241 · SampleApp     CI job 3/8 running             View run │
│    │ SAVED VIEWS          │                                                                   │
│    │ Failed CI           │ SUGGESTED REVIEWS                                                  │
│    │ Plans changed       │ ▸ Nomad · main             New plan + 4 code areas        Start…   │
│    │                      │ ▸ Artheon · feature/x      3 commits since last visit     Start…   │
│    │                      │                                         Show all 91 candidates →  │
├────┴──────────────────────┴───────────────────────────────────────────────────────────────────┤
│ ● Current · 105 available scanned · 4 unavailable retained · completed 7.2s ago               │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

Row interaction:

- Single click selects and opens a lightweight preview in the right pane.
- Enter/primary button continues or starts a review.
- Space opens a Quick Look-style evidence preview without changing tabs.
- Context menu: Pin, Snooze, Explain priority, Start review at…, Open Git graph, Reveal in Workspaces.
- `J/K` or arrows move; `Enter` acts; `S` snoozes; `P` pins.
- Each priority row includes an `Explain` popover showing the exact ranking reasons.

### 8.2 Workspaces — portfolio inventory

Purpose: browse all projects/repositories/worktrees without polluting Inbox.

The sidebar presents Projects and repository summaries. The main pane presents the selected project as a dense table/list with search, attention, availability, and review status. A third-level worktree hierarchy is therefore not forced into the narrow sidebar.

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ◉ ◉ ◉ ◫ ‹ › │ [ Workspaces ]                                                  + Project  ⋯ │
├────┬──────────────────────┬─────────────────────────────────────────────────────────────────────┤
│ ▣  │ WORKSPACES           │ SampleApp                                             3 repositories│
│ ◎  │ ⌕ Filter projects…   │ Product growth, feeds, attribution, and crawler observability       │
│ ◫  │                      │─────────────────────────────────────────────────────────────────────│
│ ⎇  │ ▾ ACTIVE             │ REPOSITORY / WORKTREE           BRANCH       ATTENTION       REVIEW │
│ ⌕  │   SampleApp      2  │ ▾ sampleapp                    —            2 decisions     R3     │
│    │   Workdeck        1  │   ● primary                     main         +2 incoming      Open   │
│    │   Nomad              │   ● feed-first                  feat/feed    clean            —      │
│    │                      │   ◌ old-checkout                 main         unavailable      R1     │
│    │ ▸ OTHER         71   │ ▸ sampleapp-infra              —            clean            —      │
│    │                      │ ▸ sampleapp-research           —            1 plan            —      │
│    │ ▸ EMPTY          7   │                                                                     │
│    │                      │ RECENT REVIEWS                                                      │
│    │                      │ SampleApp main · R3                8/14 reviewed       Continue → │
│    │                      │ Feed-first rollout · R1             accepted             Open →     │
├────┴──────────────────────┴─────────────────────────────────────────────────────────────────────┤
│ 74 projects · 75 repositories · 109 worktrees · 4 unavailable retained                         │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Requirements:

- Correct Project → Repository → Checkout → Worktree model in data and accessibility labels.
- Disclosure state persists per project/repository.
- Metadata columns align; numbers are tabular.
- Every row has availability and last-scan state with tooltip/explanation.
- Empty projects are grouped and collapsed, not silently hidden.
- Search includes project, repository, worktree path, branch, remote, and review title.
- Add Project/Repository dialogs are native, validated, and make no Git mutations.
- Selecting a worktree does not automatically start a review; the detail pane offers `Start review`, `Open Git`, and existing reviews.

### 8.3 Git — graph as a first-class review surface

Purpose: understand history and construct a review range without mutating repositories.

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ ◉ ◉ ◉ ◫ ‹ › │ [ Git · SampleApp ] [ SampleApp · main ●2 ]                          New review  ⋯ │
├────┬─────────────────────┬───────────────────────────────────────────────────────────┬──────────────────────┤
│ ▣  │ GIT                 │ sampleapp / all refs     ⌕ Filter commits…    All refs ▾│ Inspect          ◧ │
│ ◎  │ WORKTREES           ├───────────────────────────────────────────────────────────┤                      │
│ ◫  │ ● main          WIP │ ●│ WIP · main                    +18 −4       2m          │ Working tree         │
│ ⎇  │ ● feed-first    WIP │ ●│ feat: pricing feed                Rutger   8m          │ main · ahead 2        │
│ ⌕  │ ◌ legacy           │ ●│ fix: callback timeout             Agent    19m         │ 4 files · +18 −4      │
│    │                     │ ●╮ merge pull request #241           GitHub   25m         │                      │
│    │ LOCAL BRANCHES      │ │● test: callback regression         Agent    27m         │ Range                │
│    │ main               │ │● feat: dispatch callback           Agent    31m         │ Start: a1b2c3d       │
│    │ feat/feed          │ ●╯ docs: attribution plan            Rutger   2h          │ End:   e4f5a6b       │
│    │                     │ ●  chore: release                    Rutger   1d          │ 5 commits            │
│    │ REMOTES             │                                                           │ + Review range       │
│    │ origin             │                                                           │                      │
│    │                     │                                                           │ References           │
│    │ TAGS                │                                                           │ main · origin/main   │
│    │ v1.4.0             │                                                           │ PR #241             │
│    │                     │                                                           │ CI passing           │
│    │ STASHES             │                                                           │                      │
│    │ stash@{0}          │                                                           │                      │
├────┴─────────────────────┴───────────────────────────────────────────────────────────┴──────────────────────┤
│ Read-only Git · 439 commits loaded · range 5 commits                                                        │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Requirements:

- Accurate DAG lanes and merge lines, not ASCII approximations.
- A distinct WIP row for every available dirty worktree belonging to the repository identity.
- Branch, remote, tag, stash, HEAD, PR, CI, and checkpoint badges.
- Selecting one commit opens details; shift-click or `R` creates a two-point range.
- Range action creates/opens a Workdeck review; it does not check out or mutate Git.
- Graph filters: all refs, current ancestry, first parent, branch, author, path, date, reviewed/unreviewed.
- Virtualized rows, paged history, stable selection, preserved scroll anchor.
- Graph lane width is capped; overflow collapses or horizontally scrolls without crushing the subject.
- Commit inspector includes parents, author/committer, signature, refs, files, churn, linked PR/CI, and semantic review relation.
- Context menus omit mutating actions. If future mutation is introduced, it requires a separately authorized product mode and is out of this plan.

### 8.4 Review — changes

Purpose: review the smallest coherent semantic delta with synchronized evidence.

Review sidebar sections:

- Incoming
- Open
- Questioned
- Reviewed
- Sources

Toolbar:

- Breadcrumb: project / worktree / checkpoint relationship.
- Incoming indicator opens the delta/checkpoint popover.
- Conditional lens switch: Diff, Source, Calls, Structure, Rendered, Run, Artifact.
- One decision control whose label reflects state: `Mark reviewed`, `Needs attention`, or `Accepted` depending on policy.
- Overflow: copy path, open externally, view raw identifiers, reset mark, attach evidence.

Diff requirements:

- Unified and split mode.
- Syntax highlighting on both sides.
- Stable gutters, line-number selection, copy, hunk expansion, whitespace toggle.
- Move/rename/format-only indicators when analysis supports them.
- File headers stick at scroll edge.
- Selection in Structure scrolls to the corresponding hunk/symbol and vice versa.
- Marking a semantic item updates the tree without jumping to another item unless `Auto-advance` is enabled.
- The primary action may offer `Review & next` in its menu; it is not a second permanent button.

### 8.5 Checkpoint and incoming-burst workflow

```text
┌─────────────────────────────────────────────────────────────────────────────┐
│ Review checkpoint                                                       × │
├─────────────────────────────────────────────────────────────────────────────┤
│ R3 frozen 18 minutes ago                         Live has advanced by 2      │
│                                                                             │
│  R2 ───────● R3 ────────────────○ live                                     │
│             understood                + commit 9ab…                          │
│             8/14 reviewed              + PR #241 update                      │
│                                                                             │
│ Incoming delta                                                             │
│ ● 2 review items changed              reopen                                │
│ ○ 6 items unchanged                   carry understanding                   │
│ ↗ 1 item moved                        verify mapping                        │
│                                                                             │
│ [ Inspect incoming only ]       [ Compare checkpoints ]       [ Advance… ] │
└─────────────────────────────────────────────────────────────────────────────┘
```

Rules:

- Live changes never silently replace the frozen content.
- `Inspect incoming only` opens a filtered view on reopened/new items.
- `Compare checkpoints` shows semantic transition reasons and raw source diffs.
- `Advance…` previews carried/reopened/uncertain outcomes before committing the checkpoint.
- Advancing is explicit and undoable through review history.
- New incoming changes arriving during comparison update the live marker but do not invalidate the currently previewed transition.

### 8.6 Plans and Markdown

Purpose: review intent, claims, dependencies, and implementation correspondence—not just render a document.

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│ SampleApp / feed plan · R2 → live      1 claim changed       Rendered Source Diff    Review ▾ │
├───────────────────────────────────────────────────────────────────────────┬──────────────────────┤
│ # Feed-first rollout                                                     │ Structure        ◧ │
│                                                                           │ ▾ Goal               │
│ The dashboard prioritizes actionable opportunities…                      │ ▾ Decisions           │
│                                                                           │   ✓ Feed is primary   │
│ ## Decisions                                                             │   ● OAuth retries     │
│                                                                           │ ▾ Implementation      │
│ ✓ Feed becomes the default route.                  Implemented · 8 files  │   ○ Controller        │
│ ● OAuth retries use bounded backoff.                Changed since review  │   ○ Queue job          │
│ ? Historical attribution remains immutable.        Needs evidence        │ ▾ Validation          │
│                                                                           │   ✓ 18 tests          │
│ ## Validation                                                            │                      │
│                                                                           │ Evidence             │
│ - [x] Regression tests                                                    │ PR #241 · CI passing │
│ - [ ] Production probe                                                    │                      │
├───────────────────────────────────────────────────────────────────────────┴──────────────────────┤
│ Claim 2 of 7 · linked to 3 review items · R2 frozen 1h ago                                      │
└──────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Requirements:

- Rendered, Source, and Diff lenses.
- Stable heading/claim identity across edits.
- Structure pane shows headings, checkboxes, claims, links, and attachments.
- Claims can link to code symbols, commits, PR files, CI steps, and artifacts.
- Checked Markdown boxes are evidence but not automatically Workdeck-reviewed state.
- Safe Markdown rendering: scripts, iframes, remote images, and unsafe links disabled or explicitly gated.
- Relative links resolve inside the read-only snapshot, not the live filesystem.

### 8.7 Pull requests

Purpose: combine provider state with Workdeck’s durable review model.

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│ PR #241 · Feed-first dashboard     open · checks passing · 2 incoming      Review changes ▾ │
├────────────────────────────────────────────────────────────────────────┬───────────────────────┤
│ Overview                                                               │ Evidence          ◧ │
│ Feed-first dashboard                                                   │ GitHub                │
│ rutger → main · updated 12m ago                                        │ 5 commits             │
│                                                                        │ 18 files              │
│ Decision summary                                                       │ 8 checks passing      │
│ ● 2 semantic items changed after Workdeck checkpoint R3                │ 1 unresolved comment  │
│ ○ 37 reviewed items unchanged                                          │                       │
│ ✓ CI passing                                                           │ Review history        │
│                                                                        │ R2 accepted           │
│ Files                                                                  │ R3 in progress        │
│ ▸ app/Http/Controllers/AuthController.php        changed after review  │                       │
│ ▸ tests/Feature/AuthCallbackTest.php             new                   │                       │
│ ▸ resources/js/pages/feed.vue                    carried               │                       │
├────────────────────────────────────────────────────────────────────────┴───────────────────────┤
│ Provider current · Workdeck checkpoint R3 · no repository mutation                              │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Requirements:

- Overview, commits, files, checks, conversation/annotations, and provider metadata.
- Provider state and Workdeck state are visually distinct.
- Paginated loading, rate-limit state, offline cache, partial response, retry, and stale timestamp.
- PR update maps to semantic incoming changes; it does not reset all review marks.
- Open in GitHub is available; submitting provider reviews is out of scope unless explicitly designed and authorized later.
- Attach modal supports repository selection, PR search, direct URL, validation, keyboard submit/cancel, and focus restoration.

### 8.8 CI

Purpose: move from “run list” to evidence-driven failure and validation review.

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│ CI · run #991     in progress 3/8 jobs     commit e4f5a6b      Refresh       Attach run…  ⋯ │
├──────────────────────────────┬─────────────────────────────────────────────────────────────────┤
│ JOBS                         │ tests / PHP 8.4                                                   │
│ ✓ lint                 18s   │─────────────────────────────────────────────────────────────────│
│ ✓ unit · PHP 8.3       1m12s │ ✓ Checkout                                                       │
│ ● unit · PHP 8.4       0m42s │ ✓ Install dependencies                                           │
│ ○ browser                    │ ● Run tests                                        00:42         │
│ ○ deploy-preview             │                                                                            │
│                              │   PASS AuthCallbackTest                                              │
│ ANNOTATIONS                  │   PASS FeedAttributionTest                                           │
│ 1 warning                    │   … streaming, 112/240                                               │
│                              │                                                                            │
│                              │ Linked review evidence                                             │
│                              │ This job validates 6 open items · checkpoint R3                    │
├──────────────────────────────┴─────────────────────────────────────────────────────────────────┤
│ Live provider stream · last event now · Esc stops following, not the run                         │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Requirements:

- Runs → jobs → steps → logs → annotations → artifacts hierarchy.
- Status, conclusion, duration, attempt, branch/SHA, trigger, actor.
- Log streaming/follow mode with explicit pause; virtualized lines; search; ANSI handling; bounded memory.
- Failures link to files/symbols/review items when evidence is reliable.
- Retry only retries the provider read; it never reruns workflows without separately authorized mutation support.
- Artifact download progress, cancellation, checksum/size, secure ZIP import, and cleanup.

### 8.9 Artifacts

Purpose: review visual/build evidence safely and connect it to decisions.

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────┐
│ Artifacts · checkout-preview.zip     v3 · CI #991     Browser  Files  Metadata     Back  ⋯ │
├───────────────────────┬────────────────────────────────────────────────────────────────────────┤
│ COLLECTION            │ ┌────────────────────────────────────────────────────────────────────┐ │
│ ● checkout preview    │ │  localhost sandbox preview                                        │ │
│   v3 · now            │ │                                                                    │ │
│   v2 · 1h             │ │   [ rendered artifact ]                                            │ │
│   v1 · yesterday      │ │                                                                    │ │
│                       │ │                                                                    │ │
│ OTHER                 │ └────────────────────────────────────────────────────────────────────┘ │
│ ○ coverage            │                                                                        │
│ ○ screenshots         │                                                                        │
├───────────────────────┴────────────────────────────────────────────────────────────────────────┤
│ Isolated · scripts restricted · network blocked · origin 127.0.0.1:49182 · helper running       │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
```

Requirements:

- Library supports HTML, text, images, Markdown, logs, and ZIP-contained artifacts.
- Version history and linked review/PR/CI context.
- WebKit navigation denies non-exact origin, new windows, downloads, file URLs, and external schemes; external links require explicit confirmation and default browser.
- Overlay menus never appear behind the native webview; use reviewed compositing/snapshot strategy.
- Escape/Back returns to prior tab state and stops unneeded helpers.
- Import validates traversal, symlinks, entry count, compression ratio, expanded size, duplicate paths, Unicode normalization, and content type.

### 8.10 Search and command palette

There are two related but distinct tools:

- `⌘K` Command Palette: commands and navigation with optional global entities.
- `⌘P` Quick Open/Search: content-first global search.

```text
                          ┌──────────────────────────────────────────────────────┐
                          │ ⌕ callback timeout                                  │
                          ├──────────────────────────────────────────────────────┤
                          │ REVIEW ITEMS                                         │
                          │ ● Auth callback · SampleApp          changed R3     │
                          │ ○ OAuth timeout plan claim            reviewed R2    │
                          │                                                      │
                          │ COMMITS                                              │
                          │ e4f5a6b fix: callback timeout · SampleApp            │
                          │                                                      │
                          │ FILES & SYMBOLS                                      │
                          │ AuthController::callback · app/Http/…                 │
                          │                                                      │
                          │ COMMANDS                                             │
                          │ Open SampleApp Git graph                     ⌘3     │
                          └──────────────────────────────────────────────────────┘
```

Requirements:

- Search projects, repositories, worktrees, reviews, review items, paths, symbols, Markdown headings/claims, branches, commits, PRs, CI, and artifacts.
- Grouped results with stable ranking and a visible scope.
- Fast local cached results appear first; commit/provider results stream in without replacing selection unexpectedly.
- Query generation cancels stale background work.
- Preserve previous results while pending only when query relationship makes this safe.
- Complete arrows, Home/End, Page Up/Down, Tab/Shift-Tab, Enter, Escape, and accessible active-descendant semantics.
- Restore previous focus on dismissal.
- Search result actions open the right context tab and select the relevant lens/item.

## 9. Interaction model

### 9.1 Selection and activation

- Single click selects.
- Double click or Enter opens/focuses the durable context.
- Space previews where useful.
- Right click opens a native/context menu without changing durable review state.
- Selection never triggers network or filesystem I/O on the render thread.
- Refresh preserves current area, tab, selection, focus pane, filters, expansion, and scroll anchor.
- If a selected entity disappears, select the nearest stable sibling and explain why in a transient status—not a modal.

### 9.2 Back, Escape, and close

Priority order for Escape:

1. Close topmost popover/menu/dialog.
2. Stop transient follow/search/capture mode.
3. Close temporary narrow-width overlay pane.
4. Return from artifact preview to prior content.
5. Clear local selection/filter only when doing so is reversible and obvious.

Back/forward navigates location history inside the active viewport, including selected review item and lens. It does not cycle tabs.

### 9.3 Decision controls

- One persistent decision action per review surface.
- The button label describes the exact operation.
- A menu attached to it offers alternative marks and `mark + next`.
- Keyboard shortcuts use memorable, conflict-free commands and are listed in the Review menu.
- Optimistic UI is allowed only when persistence acknowledgement is quick and failure can roll back clearly.
- Advancing a checkpoint always uses a preview/confirmation sheet because it can reopen/carry many items.

### 9.4 Context menus

Every major row type gets a focused menu:

- Review: Continue, Open New Tab, Pin, Snooze, Compare, Reveal, Copy identifier.
- Project/repository/worktree: Open, Start Review, Open Git, Copy Path, Reveal in Finder, Availability details.
- Commit: Inspect, Set Range Start/End, Review Range, Copy SHA, Open provider reference.
- Review item: Open Lens, Mark, Copy Path/Symbol, Show Evidence, Show History.
- Artifact: Open, Versions, Reveal metadata, Export safe copy, Remove from Workdeck catalog only.

No mutating Git action appears in this product mode.

### 9.5 Drag and resize

- Pane resize uses a broad invisible hit zone with a 1 px divider.
- Keyboard resize is available when divider is focused: arrows ±8 px, Shift ±20 px, Home/End min/max.
- Double-click divider resets default width.
- Dragged widths persist per area/tab and clamp to usable content.
- Viewport tabs support reorder; a clear insertion marker appears.
- Review rows do not support drag unless a meaningful operation is explicitly designed.

### 9.6 Motion

Motion communicates structure, not decoration:

| Motion | Duration | Purpose |
| --- | ---: | --- |
| Hover/focus color | 100–150 ms | Affordance |
| Menu/popover in | 120–160 ms | Layer origin |
| Pane collapse/expand | 180–220 ms | Spatial continuity |
| Tab reorder | 140–180 ms | Preserve identity |
| Row resort | 180–260 ms | Explain priority change |
| Content crossfade | 100–140 ms | Avoid flash on cached content |
| Progress pulse | bounded 30 Hz | Indicate active work |

Reduced motion removes translation, scale, resort tweening, and decorative frame requests while retaining immediate state changes.

## 10. Visual design system

### 10.1 Character

Workdeck should feel:

- native, precise, and quiet;
- warm enough to avoid “enterprise admin dashboard” sterility;
- dense where comparison matters and spacious only around long-form reading;
- unmistakably a review tool, not an editor clone;
- comfortable for all-day use on high-refresh Mac displays.

### 10.2 Layout tokens

```text
Space:       2  4  6  8  12  16  20  24  32
Radius:      4  6  8  10  12  16
Hairline:    1 physical pixel where GPUI supports scale-aware rendering
Rail:        44
Title row:   44–48 depending on platform titlebar
Toolbar:     36–40
Compact row: 28
Normal row:  34–36
Rich row:    44–56
Status bar:  24
Sidebar:     252 default / 208 min / 360 max
Inspector:   320 default / 240 min / 48vw max
Code line:   20–22
```

Every metric lives in tokens. Surface modules must not introduce unexplained local spacing constants.

### 10.3 Typography

- Default UI font: system UI (`-apple-system` equivalent through GPUI) for macOS familiarity.
- Code/data font: a deterministically bundled permissive/OFL monospace after license review; SF Mono can be a system-only option, not assumed distributable.
- Scale: 11 metadata, 12 labels, 13 body/rows, 14 emphasized rows, 16 section title, 20 empty-state title.
- Use weight and spacing before color to establish hierarchy.
- Tabular figures for counts, line numbers, durations, SHAs, and graph metadata.
- No all-caps headings larger than 11 px; small section labels use subtle tracking.

### 10.4 Color roles

The theme API exposes semantic roles only:

```text
window, sidebar, panel, canvas, raised, overlay, selection
text, text_secondary, text_tertiary, text_disabled
border, border_strong, focus_ring, accent
success, warning, danger, info
diff_add_bg, diff_add_text, diff_del_bg, diff_del_text, diff_modify
review_open, review_changed, review_questioned, review_reviewed, review_accepted
graph_lane[0..N]
syntax.*
```

Rules:

- Meet WCAG AA contrast for standard text where the rendering stack allows measurement.
- Selection remains visible in inactive windows and both appearances.
- Diff color never carries meaning alone; gutters and symbols reinforce it.
- User accent influences selection/focus, not status semantics.
- Frost/blur is optional presentation; opaque fallbacks must be equivalent.

### 10.5 Icons

- Use one icon family and consistent 14/16/18 px optical sizing.
- Prefer gpui-component’s Apache-2.0 Lucide-derived icons where they fit.
- Add custom Workdeck symbols only when a semantic concept has no familiar icon.
- Icons require accessible labels/tooltips when not accompanied by text.
- Provider marks are isolated brand assets with separate provenance.
- Avoid decorative icons before every label.

### 10.6 Surfaces and materials

- Content canvas is opaque for code clarity.
- Sidebar/titlebar may use platform vibrancy/frost when contrast remains stable.
- Raised popovers and sheets use restrained shadow and a strong boundary.
- Cards are rare; grouping usually uses section headers and dividers.
- Selected rows use a quiet fill plus focus ring when keyboard focused.
- Rounded rectangles must not wrap every control; use native-like borderless toolbar buttons.

### 10.7 Loading, empty, partial, and error language

State hierarchy:

```text
loading → partial data → current
              ↘ recoverable error
offline cached → stale timestamp → retry
empty because none exists
empty because filter excludes results
unavailable but retained
```

Every state says:

- what is known;
- what is still happening or failed;
- whether cached data is shown;
- what the user can do;
- whether the state affects review correctness.

Skeletons preserve final geometry. Spinners never replace an entire usable cached surface.

## 11. Component provenance and reuse

### 11.1 Component sourcing policy

The implementation should reuse permissively licensed, mature primitives when they fit Workdeck’s behavior. Reuse is not measured by copied line count. A component is worth adopting only if it reduces risk while preserving Workdeck’s review-first architecture.

Every adopted or independently reimplemented component gets a provenance record:

```text
component_id
workdeck_path
source_project
source_revision
source_paths
license
reuse_class
local_changes_summary
notice_location
verification_tests
```

The record should live in `docs/COMPONENT_PROVENANCE.md` and be checked by packaging gates.

### 11.2 Comet adoption map — MIT

| Comet area | Audited source | Workdeck use | Class | Action |
| --- | --- | --- | --- | --- |
| Motion primitives | `crates/ui/src/motion.rs` | Easing, pane/tab/row transitions, reduce motion, pulse cadence | A/B | Extract only generic pieces, rename product-specific symbols, retain MIT notice, add deterministic timing tests |
| Edge fades | `crates/ui/src/edge_fade.rs` | Horizontal tab overflow and scroll-edge affordance | A | Port the GPUI element after API compatibility review |
| Frost/layer compositing | `crates/ui/src/frost.rs` | Optional macOS titlebar/sidebar/popover material | B | Adapt behind an opaque fallback and contrast tests |
| Appearance state | `crates/ui/src/appearance.rs` | system/light/dark observation and stable theme application | B | Reconcile with gpui-component theme infrastructure |
| Popover scaffolding | `crates/ui/src/popover.rs` | Anchoring, keyboard menu rows, dialog cards, skeletons, bounded scrollbars | B | Split into small Workdeck-owned primitives; do not import app-specific popup state wholesale |
| Syntax cache | `crates/ui/src/syntax_cache.rs` | Byte-bounded highlighted document cache | A/B | Adopt cache policy with Workdeck keys/languages and memory metrics |
| Diff engine/view | `crates/ui/src/changes.rs` | File/hunk/line model, split pairs, sticky headers, virtualization, hunk expansion | B | Reuse isolated permissive algorithms where they beat current code; preserve Workdeck semantic item links |
| History graph | `crates/ui/src/history.rs` | Lane layout, refs, pagination, width budgeting | B | Reconcile algorithm with Workdeck’s repository/worktree model and range review |
| Unified titlebar | `crates/ui/src/shell.rs`, `shell/tabs.rs` | App-owned titlebar and viewport tabs | B | Adapt the structural pattern, not the session-specific shell |
| Theme system | `crates/theme/*` | Source-neutral semantic palette and syntax themes | B | Borrow schema ideas; audit every bundled palette separately before reuse |
| Geist fonts | `crates/ui/assets/fonts/*` | Optional UI/code font | A (OFL) | Prefer direct upstream acquisition; bundle OFL and notices if used |
| Solar icons | `crates/ui/assets/icons/*` | None by default | D/A with attribution | Avoid wholesale import; CC BY attribution and brand review required |

Mandatory rule: no Comet dependency may point to its `inspiration/` checkout. Adopted code becomes reviewed Workdeck-owned source with preserved attribution and isolated tests.

### 11.3 Waku behavior map — GPL behavior reference only

| Observed Waku behavior | Public behavior specification for Workdeck | Class | Prohibition |
| --- | --- | --- | --- |
| Stable pane-width slide | Animate the visible outer width while keeping inner layout at a stable endpoint; snap under reduce motion | C | Do not copy code, constants, comments, or tests |
| Pane render islands | Invalidate only panes whose visible state or geometry changed | C | Derive Workdeck architecture independently |
| Per-context right-pane tabs | Persist local pane surface, selection, scroll, and width per viewport tab | C | Do not copy Waku state types |
| Tab overflow fades/reveal | Show fade only toward hidden tabs and scroll active tab fully into view | C | Implement from written acceptance criteria |
| Sidebar hierarchy guides | Align children and show guides only for expanded grouped rows | C | Do not copy metrics or render source |
| Bounded reveal batches | Large collapsed groups reveal in predictable pages | C | Choose Workdeck-specific batch sizes from profiling |
| Command palette continuity | Debounce search, cancel stale generations, preserve safe prior results, restore focus | C | Implement against Workdeck search model |
| Task switcher | Show a bounded recent-context grid with keyboard cycling | C | Design Workdeck-specific review previews |
| Background-work summary | Aggregate progress/errors, bound retained output, refresh on controlled cadence | C | Use Workdeck’s scan/provider task model |
| Nested scrolling | Consume scroll in child only until its boundary, then chain to parent | C | Independent pointer/scroll implementation and tests |
| Webview compositing | Ensure native webviews cannot cover GPUI overlays or survive hidden contexts | C | Implement against gpui-wry/WebKit APIs, not Waku source |

Independent-implementation workflow:

1. The behavior and acceptance criteria in this document are the input.
2. The implementing agent must not consult Waku source while writing production code.
3. A reviewer compares the result to the behavior, not to source similarity.
4. Provenance records say `behavior reference only; no source copied`.
5. If a formal legal clean-room process is desired, use separately instructed investigation and implementation teams; do not describe this investigation as formal clean-room evidence.
6. If exact implementation similarity is unavoidable due to a platform API, seek separate permission/relicensing or omit the feature.

### 11.4 gpui-component adoption map — Apache-2.0 project, final graph still audited

Prefer existing pinned components where their behavior is complete:

| Need | Candidate | Decision |
| --- | --- | --- |
| Buttons and icon buttons | `Button`, variants, sizing | Use, wrapped by Workdeck semantic styles |
| Inputs/search | `InputState`, `Input` | Use; add Workdeck generation/cancellation coordinator |
| Menus | popup/dropdown menu | Use for ordinary menus; custom command palette remains product-owned |
| Tabs | `Tab`, `TabBar` | Evaluate for local segmented tabs; viewport tab strip likely needs a dedicated component |
| Resizable panes | `ResizableState`, `h_resizable`, `resizable_panel` | Use if it supports keyboard resize, persisted clamps, and animation; otherwise wrap/extend |
| Virtual lists | `v_virtual_list`, `VirtualListScrollHandle` | Use for all large lists with stable row identities |
| Syntax | `LanguageRegistry`, `SyntaxHighlighter`, `TextView` | Use as tokenizer/renderer infrastructure; Workdeck owns caching and diff integration |
| Markdown | `TextView::markdown` | Use only after security, selection, link, attachment, and long-document tests |
| Status bar | `StatusBar` | Use with reduced persistent content |
| Tree/table/dock | library components where available | Evaluate in a harness before integration; do not adopt a generic dock that fights Workdeck’s fixed review layout |

### 11.5 Components Workdeck must own

These encode product semantics and should not be generic-library abstractions:

- `AppRail`
- `ViewportTabStrip`
- `ReviewNavigator`
- `CheckpointControl`
- `IncomingDeltaPopover`
- `DecisionButton`
- `SemanticReviewTree`
- `EvidencePanel`
- `ReviewTransitionBadge`
- `GitGraphCanvas`
- `MultiWorktreeWipRow`
- `ReviewDiffView`
- `PlanClaimView`
- `ProviderStateView`
- `ArtifactSandboxHost`
- `InboxDecisionRow`
- `PriorityExplanationPopover`
- `GlobalSearchPalette`
- `BackgroundTaskCenter`

## 12. Proposed Rust/GPUI architecture

### 12.1 Target module structure

Replace the monolithic UI module with ownership boundaries:

```text
crates/workdeck-ui/src/
  lib.rs                         # exports, initialization, no feature rendering
  app/
    mod.rs
    model.rs                     # AppModel and pure derived selectors
    actions.rs                   # typed user intents/key actions
    reducer.rs                   # synchronous state transitions
    effects.rs                   # background requests/generation guards
    navigation.rs                # area/tab/location history
    preferences.rs               # persisted presentation state
    background_tasks.rs          # task registry/progress/cancel
  design/
    mod.rs
    tokens.rs
    theme.rs
    typography.rs
    icons.rs
    focus.rs
    motion.rs
    materials.rs
  components/
    app_rail.rs
    viewport_tabs.rs
    context_toolbar.rs
    split_pane.rs
    virtual_tree.rs
    command_palette.rs
    status_bar.rs
    empty_state.rs
    error_state.rs
    progress.rs
    tooltip.rs
    badges.rs
    menu.rs
  surfaces/
    inbox/
      mod.rs
      model.rs
      view.rs
      priority.rs
    workspaces/
      mod.rs
      navigator.rs
      portfolio.rs
      detail.rs
    git/
      mod.rs
      navigator.rs
      graph.rs
      inspector.rs
      range.rs
    search/
      mod.rs
      coordinator.rs
      results.rs
    artifacts/
      mod.rs
      library.rs
      viewer.rs
    settings/
  review/
    mod.rs
    navigator.rs
    toolbar.rs
    checkpoint.rs
    decision.rs
    tree.rs
    evidence.rs
    inspector.rs
    diff/
    source/
    markdown/
    calls/
    structure/
    pull_request/
    ci/
  shell/
    mod.rs
    root.rs
    titlebar.rs
    layout.rs
    responsive.rs
    menus.rs
  harness/
    fixtures.rs
    scenarios.rs
    screenshots.rs
```

Recommended maximums:

- Normal source file: aim below 600 lines.
- Complex renderer: may reach 1,000 lines only with an explicit internal structure and focused tests.
- `lib.rs`: below 250 lines.
- Pure state/selectors should be testable without launching GPUI.

### 12.2 State layers

```text
Domain/catalog state             Presenter snapshots                 UI state
────────────────────             ───────────────────                 ────────
projects/repositories            immutable WorkspaceSnapshot         selected area
worktrees/checkpoints     →       ReviewSnapshot              →       viewport tabs
review units/marks               GitSnapshot                         pane visibility/widths
providers/artifacts              SearchSnapshot                      selection/focus/scroll
```

Rules:

- Domain state is durable truth.
- Presenter snapshots are immutable and versioned.
- UI state is cheap, local, and independently persisted where useful.
- Rendering never obtains a database lock, invokes Git, reads a file, launches a process, or performs provider I/O.
- A missing snapshot renders loading/partial state and schedules an effect outside render.

### 12.3 Navigation model

```rust
enum GlobalArea { Inbox, Workspaces, Git, Search, Artifacts }

enum TabTarget {
    Review(DeckId),
    Git(GitScope),
    Search(SavedSearchIdOrEphemeral),
    Artifact(ArtifactId),
    AreaHome(GlobalArea),
}

struct ViewportTabState {
    id: TabId,
    target: TabTarget,
    location_history: Vec<Location>,
    current_location: Location,
    navigator: PaneState,
    inspector: PaneState,
    selection: SelectionState,
    scroll_anchors: ScrollAnchors,
    filters: TabFilters,
}
```

Opening an entity is an explicit policy:

- `Open`: reuse a matching tab.
- `OpenInNewTab`: create another viewport state.
- `Preview`: transient, replaced by next preview, upgraded to persistent on edit/decision/pin.

### 12.4 Effects and cancellation

Every background operation has:

- a stable task key;
- monotonically increasing generation;
- cancellation token;
- bounded concurrency;
- progress snapshot;
- partial-result policy;
- timeout/deadline where external work is involved;
- stale-result guard;
- user-readable failure;
- metrics for duration, queued time, work count, and cancellation latency.

Examples:

```text
InboxScan(catalog_revision)
GitHistory(repository_id, scope, page)
ReviewDetail(deck_id, checkpoint_id, item_id)
DiffBuild(snapshot_pair, filters)
SyntaxHighlight(content_hash, language, theme)
GlobalSearch(query_generation, scopes)
ProviderFetch(provider, resource_id, etag)
ArtifactStart(artifact_id, version)
```

### 12.5 Render-island strategy

The shell, navigator, workbench, inspector, status bar, and overlays should be separate GPUI entities when independent invalidation materially reduces work.

Acceptance criteria:

- Inbox scan progress does not rebuild the diff tree.
- Syntax highlight completion rebuilds only affected visible code rows.
- Tab badge changes do not rebuild inactive tab content.
- Pane animation does not reconstruct immutable review rows.
- Status-clock updates do not invalidate the workbench.
- Theme changes intentionally invalidate all visible surfaces once.

### 12.6 Persistence and migration

Persist:

- tab list/order/pins/targets;
- active tab;
- last selected global area;
- pane visibility and widths per area/tab class;
- review selection/lens/filter/scroll anchors;
- project/repository expansion;
- Git scope/filter/range;
- saved searches and Inbox views;
- appearance, density, reduce-motion override, and sidebar icon size.

Migration rules:

- Existing durable catalog/review/provider/artifact records are never rewritten merely for UI redesign.
- Map the current last destination/deck/tab/mode into one new viewport tab on first launch.
- Preserve current `UiPreferences` values where semantics match.
- Unknown/invalid presentation values fall back safely and are retained in a migration backup.
- Migration is idempotent and versioned.
- A crash during migration leaves the prior preference file/catalog usable.

## 13. Data and presenter work

### 13.1 New presenter snapshots

Introduce view-specific immutable snapshots instead of one broad workspace snapshot feeding every renderer:

```text
ShellSnapshot
InboxSnapshot
WorkspacePortfolioSnapshot
WorkspaceDetailSnapshot
GitNavigatorSnapshot
GitGraphSnapshot
ReviewNavigatorSnapshot
ReviewContentSnapshot
ReviewStructureSnapshot
ReviewEvidenceSnapshot
SearchResultsSnapshot
ArtifactLibrarySnapshot
BackgroundTaskSnapshot
```

Each snapshot has:

- revision/generation;
- loading/current/stale/partial/error state;
- stable row IDs;
- enough derived text/metrics to render without expensive recomputation;
- explicit source timestamps;
- no heavyweight unused payload.

### 13.2 Inbox model improvements

Add explainable priority facts rather than a single opaque score:

```text
AttentionReason::SemanticChange { reopened, added, uncertain }
AttentionReason::QuestionedItems { count }
AttentionReason::FailedCi { run, jobs }
AttentionReason::ReviewRequested { provider, pr }
AttentionReason::Pinned
AttentionReason::PreviouslyActive
AttentionReason::Uncaptured { commits, dirty_summary, documents }
AttentionReason::UnavailableHistoryRetained
```

Priority functions are pure and tested. UI shows the dominant reason and can reveal all reasons.

### 13.3 Git graph model improvements

The presenter supplies geometry-independent graph edges:

```text
GraphNode { commit_id, parents, refs, kind, worktree_ids, metadata }
GraphLaneAssignment { node_id, lane, incoming_edges, outgoing_edges }
GraphPage { rows, lane_count, continuation, range_membership }
```

The GPUI canvas owns pixels, colors, hover, and selection. Background CPU code owns lane assignment.

### 13.4 Semantic synchronization

Every review surface consumes one selected semantic review item and synchronized evidence links. Changing lenses must not select a different item merely because that lens groups content differently.

Mappings include:

- file ↔ symbols ↔ AST nodes ↔ hunks;
- Markdown heading ↔ claims ↔ linked implementation;
- commit ↔ files ↔ semantic transitions;
- PR patch ↔ checkpoint delta;
- CI step/annotation ↔ files/symbols/review items;
- artifact region/version ↔ linked plan/code/CI evidence.

Confidence is visible when mapping is inferred.

## 14. Accessibility and keyboard specification

### 14.1 Keyboard map

| Command | Shortcut | Scope |
| --- | --- | --- |
| Command palette | `⌘K` | Global |
| Quick open/search | `⌘P` | Global |
| Inbox | `⌘1` | Global |
| Workspaces | `⌘2` | Global |
| Git | `⌘3` | Global |
| Search | `⌘4` | Global |
| Artifacts | `⌘5` | Global |
| Show/hide navigator | `⌘0` | Global/tab |
| Show/hide inspector | `⌘⌥0` | Global/tab |
| New viewport tab | `⌘T` | Global |
| Close viewport tab | `⌘W` | Global |
| Next/previous tab | `⌃Tab` / `⌃⇧Tab` | Global |
| Back/forward | `⌘[` / `⌘]` | Active tab |
| Refresh current context | `⌘R` | Active tab; read only |
| Next/previous item | `J/K` and arrows where appropriate | Review/list |
| Open/activate | `Enter` | Focused control |
| Quick preview | `Space` | Supported lists |
| Local filter | `/` | Current pane |
| Mark reviewed | configurable; default `R` only in Review context | Review |
| Needs attention | configurable; default `Q` | Review |
| Toggle unified/split | `⌥D` | Diff |
| Focus navigator/content/inspector | `⌃1/2/3` | Active tab |
| Escape | context stack | Global |

All shortcuts must exist in native menus and Command Palette. User customization can follow after the stable command IDs exist.

### 14.2 Focus model

- Focus order follows rail → navigator → toolbar → workbench → inspector → status only when status has interactive controls.
- Pane roots are focus groups; arrows navigate rows within a group.
- Tab restores the last focused child when re-entering a pane.
- Focus ring appears only for keyboard focus, remains visible on selections, and has theme-tested contrast.
- Opening a dialog focuses its first invalid field or primary field.
- Closing a dialog restores the invoking control if it still exists.
- Disabled controls explain why through accessible description and tooltip where appropriate.

### 14.3 Semantic labels

Examples:

```text
"Inbox, 2 decisions"
"SampleApp main review, 2 incoming changes, 8 of 14 reviewed"
"AuthController.php, changed since checkpoint R3, one open item"
"Commit e4f5a6b, fix callback timeout, by Agent, 19 minutes ago"
"Structure pane, level 2, Controllers folder, expanded"
"Mark AuthController callback as reviewed"
```

Do not read decorative icons, separator dots, or duplicated count labels.

### 14.4 Visual accessibility

- Test normal and increased contrast.
- Honor system reduce motion.
- Support 100–160% UI scale if GPUI/platform permits without clipping.
- Preserve text selection and copy in code/Markdown/log views.
- Never use only red/green for diff or pass/fail.
- Minimum pointer target 28 px in dense lists, 36 px for primary toolbar controls; extend invisible hit regions where visual glyphs are smaller.

## 15. Responsive layout specification

### 15.1 Breakpoints by available content, not device label

| Class | Width | Layout |
| --- | ---: | --- |
| Wide | ≥ 1320 px | Rail + navigator + workbench + inspector |
| Intermediate | 1040–1319 px | Rail + navigator + workbench; inspector toggle/overlay |
| Compact | 900–1039 px | Rail + workbench; navigator/inspector overlays |
| Below minimum | < 900 px | Window constrains to minimum or offers a single-pane fallback only if fully usable |

These numbers are starting hypotheses and must be validated with actual font metrics and native QA.

### 15.2 Adaptive rules

- Never let center diff code width fall below approximately 620 px in split mode; automatically switch to unified below the measured threshold and show a nonintrusive explanation.
- Navigator hides before inspector only when the current task is reading content; in Workspaces the reverse may be preferable.
- Toolbar labels progressively shorten, then move to overflow; icon-only controls always retain tooltips/labels.
- Breadcrumb collapses middle segments first.
- Status bar removes low-priority segments from trailing to leading.
- Dialog width clamps to viewport with scrollable body and fixed actions.
- Popovers flip/shift to remain on-screen.

## 16. Performance plan and budgets

### 16.1 Product budgets at 109-worktree scale

| Operation | Target |
| --- | ---: |
| Shell first paint from process start | ≤ 350 ms on a warm machine |
| Cached catalog visible | ≤ 650 ms |
| Cold launch usable without full scan | ≤ 1.2 s |
| Area/tab switch with cached snapshot | p95 ≤ 50 ms |
| Row selection response | next frame; p95 ≤ 16 ms |
| Command palette open | ≤ 50 ms |
| Cached search first results | ≤ 80 ms |
| Inbox first actionable partial result | ≤ 300 ms |
| Full 105-available-worktree scan | p95 ≤ 7 s; progressive/cancellable |
| Scan cancellation acknowledgement | ≤ 150 ms |
| Git graph first page | ≤ 250 ms cached / ≤ 750 ms cold |
| Diff initial visible rows | ≤ 250 ms after data arrival |
| Scroll/render on 120 Hz display | p95 frame work ≤ 8 ms; no sustained jank |
| Idle CPU | approximately 0%, excluding explicit providers/watchers |
| Baseline resident memory | target ≤ 350 MB |

Budgets are measured, not “fixed” by adding delays or hiding work.

### 16.2 Required techniques

- Virtualize every unbounded list/tree/log/diff/history.
- Use stable row IDs and uniform heights where possible.
- Hoist derived row data out of builders.
- Time-slice or background syntax highlighting and AST/call analysis.
- Bound syntax, diff, image, provider, and search caches by bytes and item count.
- Cancel stale generations on query, tab, checkpoint, and repository changes.
- Debounce only noisy input; direct navigation should not feel delayed.
- Use fingerprint/cache keys based on stable snapshot identities.
- Load provider and deep evidence on demand.
- First paint never waits for inbox scan, Git history, providers, or syntax completion.
- Make progress monotonic and retain useful cached/unavailable history.

### 16.3 Profiling scenarios

Profile with signposts/counters for:

- cold and warm launch;
- restart with 12 restored tabs;
- full catalog and cached catalog;
- inbox progressive scan/cancel/restart;
- SampleApp and Workdeck active-review switching;
- Git graph 100/1,000/10,000 rows;
- 5,000-file tree expand/filter;
- 100k-line unified and split diff scrolling;
- 50k-line CI log follow/search;
- 5 MB Markdown plan;
- global search with commit/provider streaming;
- light/dark switch;
- pane collapse animation during background updates;
- artifact open/back/quit lifecycle.

## 17. Security and trust boundaries

### 17.1 Read-only repository contract

Create a central command policy used by every Git/filesystem operation:

- permit status, show, diff, log, rev-parse, for-each-ref, worktree list, cat-file, ls-files, and other reviewed read-only commands;
- reject add, commit, checkout/switch, reset, clean, stash, rebase, merge, cherry-pick, fetch, pull, push, worktree add/remove/prune, branch mutation, tag mutation, and config writes;
- never set repository working directory as a Workdeck state path;
- tests run mutation attempts only against isolated temporary repositories and assert rejection.

### 17.2 Provider contract

- Read-only GitHub token scopes where possible.
- Redact tokens and sensitive headers from logs/errors.
- Cache with ETag/timestamp and clearly mark stale data.
- Bound pagination, body size, log size, retries, and artifact size.
- Use exponential backoff with cancellation and provider rate-limit awareness.
- External browser opening is explicit.

### 17.3 Artifact contract

- Exact loopback origin allowlist.
- Random high port and unguessable per-preview token/path.
- Content Security Policy appropriate to the artifact mode.
- No direct repository filesystem mapping.
- Download/new-window/navigation interception.
- ZIP bomb/traversal/symlink defenses.
- Helper inherits app-owned lifetime and exits on stdin close/app shutdown.
- Crash recovery cleans only verified Workdeck temporary roots.

## 18. Implementation strategy

The redesign is clean-sheet at the UI layer but must ship incrementally behind a reversible preference/feature flag until parity and quality gates pass. Do not maintain two product architectures long-term.

### Phase 0 — freeze evidence and license gate

Deliverables:

- Capture current light/dark screenshots and native interaction recordings for every destination.
- Export deterministic fixtures for Inbox, Workspaces, Git, Review, Plans, PR, CI, Artifacts, Search, loading, offline, empty, and errors.
- Record current performance baselines and catalog invariants.
- Add `docs/COMPONENT_PROVENANCE.md` and third-party notice generation.
- Resolve or isolate the GPUI GPL transitive dependency before commercial release work proceeds.
- Pin exact GPUI revision in the manifest.
- Add a packaging assertion that `inspiration/` cannot enter Cargo metadata or the bundle.

Exit gate:

- Existing app is fully reproducible and fixtures cover all durable states.
- License graph has a written disposition for every non-permissive/unknown edge.

### Phase 1 — UI state architecture

Deliverables:

- Introduce `AppModel`, typed actions, reducer, effects, `GlobalArea`, `ViewportTabState`, and pane state.
- Split immutable presenter snapshots by surface.
- Implement generation/cancellation registry.
- Migrate existing UI preferences idempotently.
- Add pure navigation, tab restoration, back/forward, and selection tests.

Exit gate:

- No visual redesign required yet, but old UI can be driven from new state boundaries.
- Render code performs no direct I/O.

### Phase 2 — design-system harness

Deliverables:

- Tokens, themes, typography, icon rules, focus, motion, materials.
- Component gallery/harness for every interactive state in light/dark/system/reduced-motion.
- Buttons, row styles, badges, tab, tooltip, popover, menu, dialog, progress, empty/error, split handle.
- Adopt approved Comet primitives with provenance and notices.
- Visual golden baseline at 1× and 2× scale.

Exit gate:

- Every primitive is keyboard operable and has an accessibility contract.
- No product surface defines ad hoc colors/radii/spacing outside approved tokens.

### Phase 3 — new shell

Deliverables:

- Unified native titlebar/tab strip.
- Siderail and contextual navigator.
- Workbench/right pane/status bar.
- Wide/intermediate/compact responsive rules.
- Per-tab state restoration, overflow, reorder, pin, close, preview, and incoming badges.
- Native View/Window/Navigation menus and shortcuts.

Exit gate:

- Maximum two persistent horizontal chrome rows.
- No clipping or unusable pane at minimum width.
- 12-tab restart and restoration pass.

### Phase 4 — Inbox and Workspaces

Deliverables:

- Decision-row Inbox with bounded groups and priority explanations.
- Saved views, pin, snooze, waiting, suggested review table.
- Active SampleApp/Workdeck visibility.
- Portfolio Workspaces with correct hierarchy, aligned metadata, collapsible groups, and unavailable-history state.
- Progressive scan center with cancel/retry and no interface block.

Exit gate:

- A reviewer can identify and open the next decision within five seconds in a moderated test/harness scenario.
- Full invariant remains 74/75/109/7/4 after repeated refresh/restart.

### Phase 5 — Git

Deliverables:

- GPU-rendered graph canvas using CPU-assigned lanes.
- Worktree-aware WIP rows.
- Ref navigator, filters, pagination, selection, inspector.
- Two-point range and Create Review action.
- Review-only menus and command policy tests.

Exit gate:

- Graph topology matches isolated Git fixtures including octopus/merge edges, tags, remotes, stashes, detached HEAD, and linked worktrees.
- No reviewed repository mutation is possible through UI or command handlers.

### Phase 6 — Review workbench

Deliverables:

- Review navigator, checkpoint control, incoming delta workflow, decision control.
- Canonical Structure/Evidence/Inspect pane.
- Unified/split diff, Source, Calls, Structure/AST, Rendered Markdown.
- Selection synchronization and scroll restoration.
- Semantic transition display and mark carry-forward.

Exit gate:

- Agent-burst scenario proves unchanged understanding carries, changed units reopen, incoming can be reviewed without moving the checkpoint, and explicit advance produces expected transitions.

### Phase 7 — Plans, PR, CI, Artifacts

Deliverables:

- Claim-aware Plan surface.
- Provider-aware PR surface.
- run/job/step/log/artifact CI surface.
- secure artifact library/viewer and version model.
- complete attachment/import dialogs.

Exit gate:

- Loading, cached, offline, partial, failure, retry, success, and empty states pass for every source type.
- Artifact helper and WebKit security/lifecycle matrix passes.

### Phase 8 — Search, palette, and task center

Deliverables:

- `⌘K` command palette and `⌘P` global quick open.
- Grouped streaming results and saved searches.
- Recent review/tab switcher.
- Background task center with progress/cancel/errors.

Exit gate:

- Search reaches every promised entity type and stale queries cannot overwrite current results.
- Focus restoration and keyboard-only flows pass.

### Phase 9 — obsessive polish and removal

Deliverables:

- Native visual iteration in light/dark/system at all widths.
- Hover/focus/pressed/disabled/loading/error state review for every control.
- Remove old shell and obsolete presentation code.
- Break remaining oversized files/modules.
- Performance profiling and optimization.
- Copy, terminology, tooltips, menu organization, and empty-state rewrite.
- Full release gates and exact packaged-app QA.

Exit gate:

- No old navigation architecture remains.
- Zero known P0/P1, no unexplained P2, no inert/placeholder controls, no obvious visual debt.

## 19. A–Z implementation matrix

| Key | Scope | Required proof |
| --- | --- | --- |
| A | App model, actions, reducer, effects | Pure transition tests; no I/O in render |
| B | Background task center | Cancellation, progress, stale generation, bounded retention |
| C | Checkpoints and incoming bursts | Frozen/live/advance end-to-end semantic tests |
| D | Design tokens and themes | Gallery + light/dark/system/contrast goldens |
| E | Evidence pane | Commits/PR/CI/artifact mappings and partial states |
| F | Focus, keyboard, menus | Full keyboard matrix and focus restoration |
| G | Git graph | Topology fixtures, WIP per worktree, range review |
| H | HTML artifact security | Navigation, CSP, ZIP, lifecycle, helper shutdown |
| I | Inbox | Explainable priority, bounded suggestions, pin/snooze |
| J | Jobs/CI | Run/job/step/log/annotation/artifact workflow |
| K | Keyboard resizing and responsive panes | Divider keyboard tests and width matrices |
| L | Licensing/provenance | SBOM, notices, no GPL/unknown release graph |
| M | Markdown/plans | Stable headings/claims, safe rendering, evidence links |
| N | Navigator hierarchy | Indentation, disclosure, persistence, unavailable history |
| O | Offline/loading/error/empty | Deterministic state harness for every surface |
| P | Pull requests | Pagination, cached/partial provider, semantic incoming mapping |
| Q | Quick open/command palette | Grouping, streaming, cancellation, keyboard/focus |
| R | Review workbench | Synchronized lenses, marks, auto-advance preference |
| S | Shell, siderail, viewport tabs | Restoration, reorder, overflow, preview, close semantics |
| T | Trees and virtualization | 5,000-file and semantic-tree scrolling/selection |
| U | Unified/split diff | Syntax, gutters, selection, hunk expansion, huge diff |
| V | Visual polish | Native screenshot matrix and manual pixel audit |
| W | Workspaces | 74/75/109/7/4 invariant and hierarchy correctness |
| X | Cross-source search | Every entity kind and result navigation |
| Y | Yield-safe performance | Signposts, budgets, no frame-thread blocking |
| Z | Zero-defect release | Complete quality/release/native QA gates |

## 20. Deterministic test and QA matrix

### 20.1 Fixture catalog

Create isolated fixtures for:

- clean repository;
- dirty tracked/untracked/renamed/binary/submodule/LFS-like files;
- nested linked worktrees and unavailable/prunable worktrees;
- linear, branched, merged, octopus, tagged, stashed, detached Git graphs;
- 5-commit/2-PR agent burst while checkpoint remains frozen;
- semantic unchanged, changed, moved, format-only, ambiguous, removed, new items;
- large polyglot syntax corpus;
- huge unified/split diff;
- Markdown with headings, tasks, links, images, unsafe HTML, and attachments;
- PR pagination and provider partial/rate-limit/offline states;
- CI success/failure/cancel/in-progress with long logs and artifacts;
- safe/unsafe ZIP and HTML artifacts;
- empty project, project with multiple repository identities, multiple checkouts/worktrees;
- 74/75/109-scale generated catalog preserving the seven/four exceptions.

All state-changing tests use temporary catalogs and repositories. Production repositories remain read only.

### 20.2 Visual golden matrix

Capture each primary surface at:

- 1512 × 982 wide;
- 1280 × 800 intermediate;
- 1024 × 700 compact;
- 900 × 600 minimum;
- light, dark, system-light, system-dark;
- normal/reduced motion final frames;
- standard/increased contrast where available;
- navigator open/closed;
- inspector open/closed/overlay;
- loading, partial, empty, error, offline, populated.

Goldens are regression aids, not automatic proof of quality. Every changed golden requires a human-readable reason.

### 20.3 Native Computer Use matrix

Run against the exact packaged `dist/Workdeck.app`:

1. Cold launch and shell-first paint.
2. Restore multiple viewport tabs and selections.
3. Open every rail area.
4. Toggle/collapse/resize navigator and inspector by pointer and keyboard.
5. Open, reorder, pin, cycle, close, and restore tabs.
6. Navigate Inbox entirely by keyboard; pin/snooze/explain/start/continue.
7. Find SampleApp and Workdeck from Inbox and Workspaces.
8. Expand/collapse project/repository/worktree hierarchy.
9. Search and filter Workspaces/Git/Review.
10. Scroll/select Git graph, inspect commit, create range review.
11. Switch every applicable review lens and diff mode.
12. Mark/question/review items; inspect incoming; preview and advance checkpoint.
13. Open/cancel/validate/submit every modal in permitted test catalogs.
14. Exercise PR/CI loading/offline/retry/partial states.
15. Open HTML artifact; test denied navigation; Back/Escape/window close/app quit.
16. Open menus/context menus/tooltips and verify focus restoration.
17. Repeat light/dark and minimum-width critical flows.
18. Verify no repository file, Git state, ref, index, or config changed.

### 20.4 Performance gates

- Instrument every budget in section 16.
- Store baseline and current distributions, not only single timings.
- Fail CI on statistically meaningful regression beyond agreed tolerance.
- Track render count per island, visible row build count, cache bytes/hit rate, task cancellation latency, and UI-thread blocking spans.
- A retry, arbitrary sleep, larger cap, or hidden loading delay is not a valid performance fix.

### 20.5 Release commands

At minimum:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked
cargo deny check advisories bans licenses sources
shellcheck scripts/*.sh
scripts/feature-matrix.sh
scripts/quality-gates.sh --release --live
codesign --verify --deep --strict --verbose=4 dist/Workdeck.app
```

Add gates for:

- exact Cargo Git revision pinning;
- no unapproved GPL/copyleft packages in release graph;
- `inspiration/` exclusion from workspace/package/bundle;
- component provenance completeness;
- SBOM and third-party notice accuracy;
- icon/font/theme asset provenance;
- visual golden harness;
- accessibility label uniqueness/completeness;
- no direct I/O reachable from render functions;
- packaged helper shutdown;
- production SQLite integrity/foreign keys and catalog invariants.

## 21. Definition of done by surface

### Shell

- One siderail, one viewport tab row, one context toolbar.
- No redundant identity or action rows.
- Tabs mean open contexts and restore completely.
- Every pane collapses/resizes/adapts correctly.

### Inbox

- Only decision-relevant rows appear by default.
- SampleApp, Workdeck, and pinned active work are immediately discoverable.
- Suggested uncaptured work is bounded and explainable.
- Pin, snooze, continue, start, waiting, retry, and saved views work.

### Workspaces

- Full catalog remains accessible without overwhelming Inbox.
- Hierarchy and indentation are unambiguous.
- Counts/metadata align.
- Empty/unavailable retained states are honest.

### Git

- Graph is accurate, readable, virtualized, and worktree aware.
- WIP/ref/stash/tag/range interactions work.
- No mutating operation exists.

### Review

- Frozen/live distinction is always understandable.
- Incoming changes are inspectable without losing place.
- Canonical structure stays immediately right of content.
- Lenses synchronize around one review item.
- Durable marks survive appropriate source churn.

### Plans/PR/CI/Artifacts

- Each is a complete workflow, not a passive attachment list.
- All network/offline/partial/security/lifecycle states work.
- Evidence links back to semantic review items.

### Search

- All promised entity types appear.
- Results are fast, stable, grouped, and keyboard complete.
- Opening a result lands in the correct tab/item/lens.

## 22. Risks and mitigations

| Risk | Impact | Mitigation |
| --- | --- | --- |
| GPL source or transitive dependency contaminates commercial binary | Blocks proprietary distribution | Provenance ledger, no Waku code, resolve GPUI tracing path, strict deny/SBOM gate, legal review |
| Big-bang UI rewrite regresses durable semantics | Loss of core product advantage | Keep domain/catalog stable, add fixtures first, drive old/new from typed state during transition |
| Tabs become another source of clutter | Repeats current hierarchy problem | Tabs represent open contexts only; preview/reuse policy; restore and close semantics |
| Inbox again becomes inventory | Overwhelming and low value | Bounded groups, reason-first rows, suggested table, Workspaces owns inventory |
| Generic component library dictates product structure | Awkward UX | Wrap primitives; Workdeck owns semantic components and fixed shell |
| GPUI accessibility limitations | Keyboard/screen-reader gap | Use roles/labels/focus where supported, test platform tree, document upstream limitation, no mouse-only controls |
| Webview overlays break native layering | Menus hidden, helper leaks | Central visibility authority, reviewed compositing fallback, lifecycle tests |
| Large repository blocks UI | Daily-driver failure | Snapshot/effect boundary, virtualization, cancellation, profiling budgets |
| Semantic mapping overclaims certainty | Reviewer distrust | Confidence/provenance visible; uncertain mapping never silently carries marks |
| Responsive collapse hides critical context | Confusion in small windows | Measured thresholds, visible pane toggles, overlays, menu equivalents, native QA |

## 23. Explicit non-goals for this redesign

- No Git staging, commit, checkout, fetch, push, rebase, reset, stash, branch, tag, or worktree mutation.
- No code editing beyond text selection/copy and existing explicitly safe annotation/review state.
- No cloud account or multi-user collaboration redesign.
- No provider write actions such as submitting PR reviews or rerunning CI.
- No Waku source-code incorporation.
- No wholesale Comet fork or replacement of Workdeck’s domain model.
- No repository-local Workdeck metadata.
- No “AI summary everywhere” substitute for deterministic evidence.
- No redesign implementation before fixtures, provenance, and architecture boundaries exist.

## 24. Decision log

Decisions made by this plan:

1. Keep Rust + GPUI + gpui-component, subject to a permissive final dependency graph.
2. Keep Workdeck’s domain/checkpoint/review identity model.
3. Replace static review feature tabs with contextual sources/views and viewport tabs.
4. Remove Review as a global siderail destination.
5. Separate Inbox decisions from Workspaces inventory.
6. Keep Structure/Atlas immediately right of reviewed content.
7. Use a maximum of two persistent horizontal chrome rows.
8. Adopt Comet selectively under MIT/OFL/asset-specific obligations.
9. Treat Waku as clean-room behavioral reference only.
10. Default to native system UI typography and restrained material; do not clone another product’s brand.
11. Preserve read-only repository policy throughout.
12. Make performance, accessibility, and licensing release gates, not polish follow-ups.
13. Keep global destinations in the persistent 44 px siderail and reserve the titlebar tab strip for open contexts. Adapt the interaction hierarchy from the pinned MIT T3 Code and Orca ADE sources without reusing either product's brand, logo, or unverified assets.
14. Adapt T3 Code's pinned MIT pull-request list and review information architecture as closely as Workdeck's Rust/GPUI and read-only provider model permit. Preserve Workdeck checkpoint semantics and present unsupported provider mutations honestly rather than exposing inert copies of T3 controls.

Post-blueprint source pins for decisions 13–14:

- T3 Code: `https://github.com/pingdotgg/t3code`, revision `a3a8cbd60539b4af4de8f96c892dbd07a2b6c041`, MIT, copyright 2026 T3 Tools Inc.
- Orca ADE: `https://github.com/stablyai/orca`, revision `5e900b10b31f12db885e4448c3e1f6300e066efb`, MIT, copyright 2026 Lovecast Inc.

Defaults the implementation may tune from evidence:

- exact pane widths/breakpoints;
- exact row heights within defined density bands;
- exact motion durations within defined ranges;
- icon choice within the approved family;
- whether Structure or Evidence is the initial right-pane tab for a source type;
- shortcut details where macOS or GPUI conflicts exist.

Any change to the 14 decisions above requires updating this plan with a rationale before implementation diverges.

## 25. Required implementation artifacts

The implementation is incomplete until the repository contains:

- this blueprint;
- `docs/COMPONENT_PROVENANCE.md`;
- generated `THIRD_PARTY_NOTICES.md` or equivalent app-bundle notices;
- SBOM for the release bundle;
- design token/theme documentation;
- navigation/state schema and migration notes;
- deterministic fixture catalog;
- visual golden manifest;
- keyboard/accessibility matrix;
- performance baseline/report;
- security review for read-only Git and artifacts;
- packaged native QA evidence;
- external distribution checklist.

## 26. Completion criteria

The redesign is complete only when:

- the new shell fully replaces the old one;
- every surface in this document is implemented and connected to real Workdeck data;
- every visible control works or has an honest actionable unavailable state;
- no static feature-tab hierarchy or four-row review chrome remains;
- the catalog and review invariants are unchanged;
- repeated discovery/restart/refresh are idempotent;
- all state-changing tests use isolated repositories/catalogs;
- no reviewed repository is modified;
- all deterministic, performance, security, accessibility, visual, packaging, and native gates pass;
- the release graph contains no unapproved GPL/copyleft/unknown code or assets;
- zero known P0/P1 defects and no unexplained P2 defects remain;
- the exact signed `dist/Workdeck.app` is runnable and has passed the native matrix;
- external distribution blockers are limited to genuine external requirements such as Developer ID credentials, notarization/stapling, and final legal review.

## 27. Execution goal prompt

Copy and start this prompt from the Workdeck repository:

```text
/goal Execute the complete Workdeck clean-sheet UI/UX redesign in docs/WORKDECK_UI_UX_REDESIGN_PLAN.md. Treat that Markdown file as the binding product, interaction, architecture, licensing, security, performance, accessibility, migration, test, visual-QA, and completion specification. Read it completely before changing code, keep it open as the implementation checklist, and do not silently weaken or skip any requirement.

Work fully autonomously through evidence capture, fixtures, license remediation, architecture refactoring, implementation, native visual iteration, regression repair, profiling, packaging, and final verification. Do not stop at an audit, plan, scaffold, partial shell, static mockup, or passing build. Continue until every phase, A-Z matrix item, per-surface definition of done, required artifact, quality gate, and completion criterion in the plan passes.

Preserve every existing dirty change. Treat repositories selected as review input as read-only, and use isolated temporary catalogs and repositories for every state-changing test. Preserve the deterministic 74-project/75-repository/109-worktree scale fixture; discovery, migration, restart, and refresh must remain idempotent and retain durable review history and preferences.

Preserve Workdeck’s checkpoint and semantic review model while replacing the current visual/navigation architecture. The final app must use one global siderail, context navigator, viewport tabs for real open contexts, no more than two persistent horizontal chrome rows, a content-first workbench, and a collapsible Structure/Evidence/Inspect pane immediately right of the reviewed content. Inbox must show bounded, explainable decisions rather than repository inventory. Workspaces must expose the complete portfolio hierarchy. Git must provide an accurate worktree-aware graph and read-only range review. Review, Plans, Pull Requests, CI, Artifacts, Search, every synchronized lens, every modal, and every loading/partial/offline/error/empty state must work end to end.

Enforce the plan’s license policy before reusing anything. inspiration/comet at audited commit 2ebe6ed06f8e7e1ed911c0adf31ec91ad2e94274 is MIT and may be selectively adopted only with file-level provenance, preserved notices, asset-specific review, and tests. inspiration/waku at audited commit cc8b2cb0ffe9074c7a622e0b2caee22d708ed307 is GPL-3.0-only and is behavior-reference-only: do not copy or derive from its code, constants, comments, tests, strings, themes, icons, fonts, layouts, or assets. Keep inspiration/ outside Cargo metadata and every bundle. Resolve the current GPUI zlog/ztracing/ztracing_macro GPL release blocker or use a reviewed permissive dependency path; remove release exceptions, pin exact revisions, generate an SBOM and third-party notices, and fail packaging on unapproved copyleft, unknown licenses, unpinned Git sources, or untracked assets. Treat this as engineering policy pending final legal counsel, not as permission to ignore commercial-distribution risk.

Use the strict defect loop for every issue: reproduce with deterministic evidence, identify the underlying cause, implement the complete fix, add a regression test or harness, rebuild and repackage, restart the exact dist/Workdeck.app, and rerun the affected native flow in light and dark mode plus the relevant width. Never hide defects behind sleeps, retries, larger limits, disabled checks, or cosmetic patches. Keep render paths free of filesystem, Git, database, provider, subprocess, blocking-lock, and heavy analysis work. Make background work incremental, cancellable, bounded, generation-safe, visibly progressive, and nonblocking. Virtualize every unbounded tree, list, graph, diff, log, and result set. Meet or improve every performance budget in the plan at the full 109-worktree scale.

Iterate visually using the exact packaged macOS application, not only source inspection or debug builds. Verify wide, intermediate, compact, and minimum layouts; light, dark, and system appearance; reduced motion; every hover/focus/pressed/selected/disabled/loading/error state; pointer and keyboard parity; native menus/context menus/tooltips/dialogs; focus restoration; scroll/selection/tab restoration; WebKit overlay and helper lifecycle; and every surface in the native Computer Use matrix. Every visible control must work, be removed, or present an honest actionable unavailable state.

Before exit, pass cargo fmt --all --check; locked workspace checks and tests for all targets/features; Clippy with warnings denied; rustdoc with warnings denied; cargo-deny advisories/bans/licenses/sources with no unapproved release exceptions; shellcheck; metadata, icon, provenance, SBOM, notice, and inspiration-exclusion gates; CLI smoke tests; deterministic A-Z feature matrix; visual golden matrix; keyboard/accessibility matrix; performance gates; live read-only GitHub provider probes; SQLite integrity/foreign-key/catalog invariant checks; secure ZIP/WebKit/artifact-helper lifecycle tests; release packaging; strict deep codesign verification; and the complete native Computer Use matrix against the exact packaged app.

Exit only when the old shell is removed, the new architecture is coherent, every specified workflow is production-grade and visually excellent, the read-only and licensing boundaries are proven, all gates pass, there are zero known P0/P1 defects and no unexplained P2 defects, no obvious UX debt remains, and the final signed dist/Workdeck.app is left runnable. Provide a concise evidence-backed coverage report, exact measured performance and catalog results, adopted-component provenance, final dependency/license disposition, and only genuine remaining external distribution requirements such as Developer ID notarization or final legal review.
```

## 28. Research references

Primary references used for this blueprint:

- Comet repository and audited local source: <https://github.com/zeronsh/comet>
- Waku repository and audited local source: <https://github.com/egoist/waku>
- Apple Human Interface Guidelines — Sidebars: <https://developer.apple.com/design/human-interface-guidelines/sidebars>
- Apple Human Interface Guidelines — Split views: <https://developer.apple.com/design/human-interface-guidelines/split-views>
- Apple Human Interface Guidelines — Toolbars: <https://developer.apple.com/design/human-interface-guidelines/toolbars>
- Apple Human Interface Guidelines — Layout: <https://developer.apple.com/design/human-interface-guidelines/layout>
- GitKraken Desktop interface/commit graph: <https://help.gitkraken.com/gitkraken-desktop/interface/>
- GitKraken worktrees: <https://help.gitkraken.com/gitkraken-desktop/worktrees/>
- GitKraken Launchpad: <https://help.gitkraken.com/gitkraken-desktop/gitkraken-launchpad/>
- gpui-component: <https://github.com/longbridge/gpui-component>
- Zed GPUI manifest: <https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml>
- Upstream GPUI transitive-license report: <https://github.com/zed-industries/zed/issues/55470>

The repositories in `inspiration/` are pinned investigation inputs, not Workdeck build dependencies.
