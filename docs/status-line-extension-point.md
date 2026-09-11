# Status line as an extension point, and `/` as content search

Status: proposal. Nothing here is implemented.

## Summary

Make the bottom status row a real, host-owned surface that Hunk's own filter, `hunk log`
search, and extensions all drive through one primitive; then make `/` search diff content by
default by bringing `elucid/hunk-less-search` into the repo as a bundled extension built on that
primitive.

Two stacked PRs:

| PR  | Scope                                                                                                                                                                     |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Status-line primitive (items + inline prompt), host-internal first, then exposed to extensions. Filter and `hunk log` search migrate onto it.                             |
| 2   | `/` stops focusing the file filter (Tab keeps it). Bundled `search` extension binds `/`, `n`, `N`. Bundled composition grows to cover commands, highlighters, and events. |

## Why not "a meta footer pane"

The `hunk show` / `hunk log` commit header is not a new extension point. It is
`packages/hunk/src/extensions/default/ui/reviewInfo/index.tsx`, a bundled extension calling the
public `hunk.registerPane({ placement: "top" })`. The mirror image, `placement: "bottom"`, already
exists and hunk-less-search already uses it. A footer _pane_ adds nothing.

What hunk-less-search actually works around is the absence of a **status line** surface. Today:

- `StatusBar` (`packages/hunk/src/ui/components/chrome/StatusBar.tsx`) is a one-row, transient
  surface. It appears only when the filter is focused or non-empty, a notice is showing, or a
  keyboard mode is active (`statusBarVisible`, `App.tsx`). It hosts a real OpenTUI `<input>` for
  the filter, a notice slot, and the keyboard-mode badge. None of it is extension-reachable.
- `hunk log` has a second, hand-rolled footer with its own hand-rolled search editor
  (`LogApp.tsx` `appendSearch` / `backspaceSearch`, `controller.ts`).
- Pane `available()` is only re-probed when host state changes (review, files, selection, current
  line, open keys — `useExtensionPaneController.ts`), never on extension state. A pane cannot
  appear and disappear with an extension's own prompt.
- `ExtensionKeyboardModeContext` exposes no navigation; `ExtensionCommandContext` does.
- `ctx.dialogs.input()` is a centered modal — the wrong shape for `/`.

So hunk-less-search reserves one row for the whole session, re-implements a line editor in a
keyboard mode with a `█` standing in for a cursor, and defers the jump through a store token to
the pane's `useEffect` because only the pane holds `actions`. `main` even carries a PTY test
guarding that pattern (`test/pty/extensions-integration.test.ts`, "REVEAL_LINE_MOUNT_ACTIONS").
Those are honest trade-offs for a third-party extension and the wrong thing to ship as the
default product experience.

## PR 1 — the status line

### Product behavior first

The status row stays what it is today: one transient row at the bottom, below the review, above
nothing. It is visible only when something is on it. Users see:

- a prompt (`/readConfig▏`, `filter: ▏`) with a real cursor while typing,
- persistent items (`filter=foo`, `[2/7] src/session/agent/surface.ts:214 — …`, `3 files viewed`),
- the keyboard-mode badge on the right, unchanged.

No new chrome. The row is the same height it has always been; the change is who may write to it.

### Model

The status line is a small host-owned store, rendered by one component that both `App` and
`LogApp` mount. It has three concerns:

```text
items    ordered, persistent, text-only contributions   (left-aligned, trailing right-aligned)
prompt   at most one focused inline text input           (takes the left region while active)
badge    host-owned keyboard-mode / focus badge          (right edge; unchanged)
```

**Items** are declarative text, not components. An item is `{ id, spans, alignment?, priority? }`
where `spans` reuses the existing `ExtensionFileViewSpan` shape (`text`, symbolic `tone`,
`attributes`) so the host measures and truncates without a theme and paints with the active one.
Push model: the owner _sets_ an item and the host re-renders. There is no `render()` callback to
re-probe — that is the exact failure mode panes have with `available()`.

**Prompt** is imperative and promise-shaped: `line(options) → Promise<string | null>`. Options:
`prefix` (painted before the input, e.g. `/`), `placeholder`, `initial`, and an optional live
`onChange(value)` for consumers that react while typing (the file filter needs this; less-style
search does not). Enter resolves the text; Escape clears a non-empty buffer first, then cancels
with `null` — the same two-step Escape the filter has now. One prompt at a time; a second request
queues FIFO behind the first, like `ctx.dialogs`. A review reload cancels open and queued prompts,
like dialogs. App teardown resolves `null` immediately.

**Focus tier.** The prompt is a host focused input. It sits with the filter input: after dialogs
and menus, before session keyboard modes and the command table. That is what removes the
keyboard-mode requirement for anything prompt-shaped.

**Visibility.** `statusBarVisible` becomes "any item non-empty, or a prompt active, or a badge".
An item that is set but empty contributes nothing. This is a deliberate product choice: an
extension that sets a persistent item keeps the row on screen, exactly like a non-empty filter
does today. Items that should not cost a row should be cleared when idle.

**Width policy.** Left items in `priority` order, then the prompt if active, then right items,
then the badge. When the row overflows, lowest-priority items are dropped whole, then the leading
item is truncated with an ellipsis. The badge is never dropped. This is the same "measure with
symbolic colors, paint with the theme" split the STML notes and file-view spans use.

### Host-internal first

Land the primitive as host code before exposing it:

1. `packages/hunk/src/ui/statusLine/` — store (`items`, `prompt`, `badge`), a pure width/priority
   layout (`(items, prompt, badge, width) → row spans`, deterministic, tested without a renderer),
   and one `StatusLine` component that owns the `<input>` and the badge click-to-exit.
2. `App` mounts it in place of `StatusBar`. The file filter becomes the first consumer: focusing
   the filter is `prompt.line({ prefix: "filter:", initial: review.filter, onChange: review.setFilter })`,
   and the residual `filter=foo` display is a host item. `focusArea === "filter"` collapses into
   "the host filter prompt is active".
3. `LogApp` mounts the same component. `hunk.history.search` opens a prompt with `prefix: "/"`;
   `appendSearch` / `backspaceSearch` and the inline `searchEditing` branch are deleted. The
   `provider · N commits` text and the selection count become host items. This is the proof that
   the primitive is right: two hand-rolled footers and two line editors become one.

Existing behavior to preserve while doing this, with PTY coverage:

- Escape in a non-empty filter clears it; Escape in an empty filter leaves the filter.
- Tab (`hunk.app.toggleFocusArea`) still toggles between files and filter.
- The keyboard-mode badge is still clickable and still exits the mode.
- `hunk log` search still selects the next match on Enter and leaves the row showing the query.

### Then the extension surface

Expose the same store through the public API. Keep `extension-api/types.ts` import-free.

```ts
/** One symbolic run in a host-painted status item. */
export type ExtensionStatusSpan = ExtensionFileViewSpan; // text, tone?, attributes?

export interface ExtensionStatusItem {
  /** Identifies the item within its extension; `<extensionId>:<id>` globally. */
  id: string;
  spans: readonly ExtensionStatusSpan[];
  /** Defaults to "left". Right items sit beside the host badge. */
  alignment?: "left" | "right";
  /** Higher survives longer when the row overflows. Defaults to 0. */
  priority?: number;
}

/** Write to, or clear, this extension's items on the status line. */
export interface ExtensionStatusLineControls {
  /** Set or replace one item. Empty `spans` hides it without forgetting its slot. */
  set(item: ExtensionStatusItem): void;
  clear(id: string): void;
}

export interface ExtensionPromptLineOptions {
  /** Painted before the input, e.g. "/" or "filter:". Not part of the value. */
  prefix?: string;
  placeholder?: string;
  initial?: string;
  /** Called on every edit, for consumers that react while the user types. */
  onChange?(value: string): void;
}

export interface ExtensionPromptControls {
  /** Resolves the submitted text, or null on Escape / reload / teardown. */
  line(options: ExtensionPromptLineOptions): Promise<string | null>;
}
```

Where they live:

- `ExtensionCommandContext` gains `statusLine` and `prompts`. A command already has `navigation`,
  `highlights`, `review`, and `panes`, so a search command is one async handler with no store
  seam.
- `ExtensionEventContext` gains `statusLine` (an extension updating a count on `file_viewed`
  should not need a command).
- `ExtensionKeyboardModeContext` gains `statusLine` only. A mode never needs a prompt — a
  prompt-shaped interaction should now _be_ a command plus `prompts.line`.
- `HunkExtensionAPI` (factory scope) does **not** get either. Status writes should come from a
  context whose lifetime the host can scope, so items clear with the registry on extension reload.

Attribution follows the dialog policy: the prompt prefix region for a user-installed extension
carries the same `ext` marker toasts and dialogs use; bundled extensions omit it. Items carry no
marker — they are text, and the extension is discoverable through `hunk extension list`.

Lifetimes: items persist across ordinary content reloads (the extension re-derives on
`changeset_loaded` if it wants), and clear on extension reload, registry closure, and App
teardown. Prompts cancel on any reload.

Failure isolation: a bad `spans` value (non-array, non-string text) is an extension programming
error and throws from `set` like malformed dialog options. `onChange` throwing quarantines nothing;
it warns once, attributed, and the prompt continues.

Docs and tests for this half:

- `docs/extensions.md`: a `### Status line` section between panes and file views, and move the
  vim-navigation `:` example from `ctx.dialogs.input()` to `ctx.prompts.line()` since that is the
  shape it always wanted.
- `docs/extensions.md` "Not contributable yet": remove nothing yet; menu entries remain there.
- `AppHost.status-line.test.tsx` for command-driven items/prompts, queueing, reload cancellation,
  and registry-closure clearing.
- `test/pty/`: a prompt-driven extension typing, submitting, cancelling, and an overflow case.
- Bump the extension API version; document it in the Changeset.

## PR 2 — `/` searches content

### Keybindings

Current defaults, for the record:

| Surface    | `/`                       | `n` / `N`                                       | Tab                                 |
| ---------- | ------------------------- | ----------------------------------------------- | ----------------------------------- |
| review     | `hunk.review.focusFilter` | `hunk.review.nextNote` / `previousNote` (#1044) | `hunk.app.toggleFocusArea` (filter) |
| `hunk log` | `hunk.history.search`     | `hunk.history.nextMatch` / `previousMatch`      | —                                   |

Review and history already disagree about what `/`, `n`, and `N` mean. Content search on `/`
brings them into line and matches `less`, `vim`, and the expectation voiced on #463.

Changes:

- `hunk.review.focusFilter` keeps its id and behavior and loses its default key. Tab still opens
  the filter through `hunk.app.toggleFocusArea`; the menu entry stays. A user who wants the old
  behavior writes one line, and the exclusive-binding resolver takes `/` away from search:

  ```toml
  [keybindings]
  "hunk.review.focusFilter" = "/"
  ```

- `hunk.review.nextNote` / `previousNote` give up `n` / `N` and have no default key. `}` / `{`
  (annotated hunk) stay the keyboard path to notes; note stepping remains remappable through
  `[keybindings]`. Ids do not change, so no compatibility alias is needed. Update the help
  dialog, menu, and `docs/keybindings.md` rows accordingly.

### The bundled `search` extension

Bring `elucid/hunk-less-search` into `packages/hunk/src/extensions/default/search/` as a bundled
extension, rewritten onto the PR 1 primitive. What survives from the extension is the pure part:
patch parsing, smart-case literal/regex compiling, hunk-granular targets with line-exact reveal,
match marks, wrapping repeats, status wording, and its tests. What is deleted is everything the
README lists under "How the prompt line is possible": the store seam, the keyboard mode, the
prompt grammar, the pane, and the deferred-navigation token.

Registration shape:

```ts
hunk.registerLineHighlighter({ id: "matches", highlight: ({ file }) => session.marksFor(file) });

hunk.registerCommand({ id: "find", title: "Search diff content", key: "/" }, async (ctx) => {
  const query = await ctx.prompts.line({ prefix: "/" });
  if (query === null) return;
  const outcome = session.search(query, ctx.selection);
  if (outcome.target)
    ctx.navigation.revealLine(outcome.target.fileId, outcome.target.side, outcome.target.line);
  ctx.highlights.refresh("matches");
  ctx.statusLine.set({ id: "status", spans: outcome.spans });
});

hunk.registerCommand({ id: "next", title: "Next match", key: "n" }, (ctx) => step(ctx, "forward"));
hunk.registerCommand({ id: "previous", title: "Previous match", key: "N" }, (ctx) =>
  step(ctx, "backward"),
);
hunk.on("changeset_loaded", ({ changeset }) => session.setFiles(changeset.files));
```

Under the `hunk` vendor id these become `hunk.search.find`, `hunk.search.next`,
`hunk.search.previous` — same remappable command ids as everything else in
`docs/keybindings.md`. `[extension.hunk-less-search] mode = "regex"` becomes a
`[search] mode = "regex"` config key, since bundled extensions do not read `[extension.<id>]`.

Two semantic fixes while porting, both flagged in the earlier proposal:

- Search **visible** files in review order, from `ctx.review` rather than a shadow copy of
  `changeset_loaded`, so a match in a filtered-out file is never a target that navigation then
  refuses.
- Land on the first matching line (`revealLine`) and fall back to `selectHunk` only when the
  patch never numbered the line — as today, but without the pane-held `actions` indirection.

### Bundled composition has to grow

`getBundledUIRegistry()` is composed into the app in exactly one place:
`packages/hunk/src/ui/lib/extensionPanes.ts` merges `panes` only. Commands, line highlighters,
keyboard modes, and event handlers from a bundled factory are dropped on the floor. PR 2 extends
composition so bundled `commands`, `lineHighlighters`, and `eventHandlers` join the user
registry's at the same merge points (`resolveExtensionCommands` in `App.tsx`, the highlighter
runtime, `useExtensionReviewEvents`) — bundled first, so a user extension can never shadow them,
which is already the rule for key conflicts.

This is the one piece of infrastructure PR 2 needs beyond PR 1, and it is the same investment
that makes any future bundled behavior (not just panes) dogfood the public API. Constraints to
keep:

- Bundled extensions stay active under `--no-extensions`.
- Bundled registrations are trusted; they skip the repo-local trust gate.
- The bundled registry stays process-cached; its command handlers must not close over session
  state (the search session object is created per factory run, which is per process — that is
  fine because the session re-derives from `changeset_loaded`).

The alternative — port the search natively into `App` using the same PR 1 primitives — is less
work in this PR and equally correct for users. I would still do the bundled route: it is what
you asked for, `reviewInfo` already set the precedent that bundled UI goes through the public
API, and it forces the composition gap closed instead of leaving it as a trap for the next
bundled feature.

### Migration for existing installs

- Release notes: "`/` now searches diff content; `n` / `N` step through matches. The file filter
  is on Tab and in the menu. Remap `hunk.review.focusFilter` to restore `/`."
- Anyone with `hunk-less-search` installed will have two commands on `/` (the bundled one wins)
  and stale `[keybindings] "hunk-less-search.find"` lines. Detect the installed id at startup and
  emit one startup notice pointing at `hunk extension remove hunk-less-search`; the existing
  startup-notice path (`useStartupNotices`) is the place.
- Archive `elucid/hunk-less-search` with a README pointer, and update
  `website/src/data/extensions.ts` to drop it from the directory.
- Update `docs/keybindings.md`, the help dialog, the menu, and the `hunk-extensions` skill.

## Verification

- PR 1: `bun run typecheck`, `bun run test`, `bun run test:integration` (new PTY coverage for the
  prompt in both review and log), `bun run test:tty-smoke`, and a real TTY run of `hunk diff`,
  `hunk log`, and a prompt-driven extension from `examples/extensions/`.
- PR 2: the same, plus `bun run generate:skill` if the hunk-review skill text mentions `/`, and a
  real TTY run confirming `/`, `n`, `N`, Tab, and a remapped `hunk.review.focusFilter = "/"`.
- Both: a Changeset each (`minor`: new extension API; `minor`: default keybinding change).

## Decisions

1. **Note stepping keys.** `n` / `N` go to search. `hunk.review.nextNote` / `previousNote` lose
   their defaults; `}` / `{` remain the keyboard path to notes, and note stepping stays
   remappable.
2. **No factory-scope status writes.** `statusLine` is reachable only from command, event, and
   keyboard-mode contexts, never from the `hunk` factory object, so every write belongs to an
   activation the host can scope to the current registry. An extension that learns something
   outside any activation (a background poll) caches the result and applies it from the next
   event it handles. If that proves too limiting, the answer is a scoped handle or a host event,
   not a factory-scope setter.
3. **No prompt `onKey` / history recall in v1.** `initial` covers "re-open with the last query".
4. **No history extension surface in these PRs.** `LogApp` consumes the host-internal primitive
   only. The extension surface for history follows when history gets extension commands at all.
