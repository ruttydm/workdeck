+++
title = "Custom panes"
description = "Render native panes around Workdeck's review stream without taking ownership of geometry or input."
template = "docs.html"
+++

`workdeck-extension-api` lets a native extension register a pane on the left,
right, top, or bottom of the review. Pair it with a registered command so a key
can open or close it:

```rust
fn register(api: &mut ExtensionApi) {
    api.register_pane(Pane {
        id: "flat".into(),
        title: "Flat files".into(),
        placement: Placement::Right,
        size: PaneSize::columns(34).with_min(22),
        default_open: false,
        render: render_flat_files,
        ..Default::default()
    });
    api.register_command(Command::new("toggle-flat", "Toggle flat pane", Key::Ctrl('f')),
        |ctx| ctx.panes().toggle("flat"));
}
```

`placement` defaults to `left`. Left/right panes use `width`; top/bottom panes
use `height`. Both accept `{ preferred, min?, max?, fraction? }`, defaulting to
`{ preferred: 34, min: 22 }` columns or `{ preferred: 8, min: 3 }` rows. Equal
bounds make a fixed pane.

`fraction` opts into live responsive sizing until the user drags the divider. It
must be greater than `0` and at most `1`; Workdeck rounds that fraction of the
full body width or height to a terminal cell, then applies `min`, `max`, and the
space required by the review. `preferred` remains the fixed-cell target when
`fraction` is omitted. A divider drag establishes a session-local cell override:
terminal shrink may clamp it temporarily, and expanding restores it. Panes
without `fraction` retain their fixed preferred startup size.

Use `default_open` to open a pane initially, `replaces: "workdeck:files"` to
replace the built-in role (and override `default_open`), or `available(context)`
to hide it conditionally. One pane may replace each target; the first
registration owns that slot and later claims are skipped with a warning.
`replaces` can name another pane by fully-qualified `<extension-id>:<pane-id>`;
Workdeck follows replacement chains.

`workdeck:files` is a named role, not a left-edge location. The
`workdeck.view.toggleFilesPane` command and **View → Files pane** follow the
resolved owner on any edge and leave independent panes alone. The compatibility
`workdeck.view.toggleSidebar` ID remains accepted. A literal
`ctx.panes().toggle("workdeck:files")` addresses the built-in pane; use
`ctx.commands().execute("workdeck.view.toggleFilesPane")` for role-aware
behavior.

Set `current_line: true` to receive the selected-row painter plus the `{ side,
line }` source address of the current-line marker. A blame or diagnostic pane
can use that address; it is not bundled with Workdeck and does not receive file
contents beyond the declared snapshot.

## Props

The host supplies fresh immutable props as the review changes:

| Prop | Meaning |
| --- | --- |
| `files` | Visible reviewed files in review-stream order, filtered and frozen, with change type, truncation state, and hunk summaries. |
| `selected_file_id` | Selected file, or `None`. |
| `selected_hunk_index` | Selected hunk in that file, or `None`. |
| `placement` | Accepted terminal edge. |
| `width` | Exact columns in the host-owned rectangle. |
| `height` | Exact rows in the host-owned rectangle. |
| `current_line` | Selected-row painter and `{ side, line }` when opted in, otherwise `None`. |
| `theme` | Semantic colors from the active theme, updated on a theme switch. |
| `keybindings` | Current command bindings after defaults and user configuration. |
| `actions` | Guarded navigation and notification actions. |

`actions.select_file(file_id)` and `actions.select_hunk(file_id, index)` route
through the same review controller as the built-in Files pane and keyboard
shortcuts. The review stream scrolls, selection updates, and
`selection_changed` fires exactly as for a built-in row. `actions.notify` shows
a toast attributed to the extension. An action targeting a non-visible file is
refused with a warning.

The three hunk surfaces line up by design: each file's `hunks` contains public
summaries (`index`, the `@@` header, inclusive old/new spans) in render order;
`selected_hunk_index` reports that index; and `select_hunk` accepts it. Match an
annotation's old/new range against those spans without touching opaque file
metadata.

## Keys inside a component

A pane that owns a key event should ask the injected keybinding manager about a
command ID rather than hard-coding its default chord. This keeps local behavior
synchronized with user remaps and unbindings:

```rust
fn handle_key(props: &PaneProps, key: KeyEvent) -> PaneKeyResult {
    if let Some(next) = props.files.get(1)
        && props.keybindings.matches(&key, "workdeck.review.nextFile")
    {
        props.actions.select_file(&next.id);
        return PaneKeyResult::Handled;
    }
    PaneKeyResult::Pass
}
```

`keybindings.get_keys(command_id)` returns the current chord list for a label or
hint; unknown and unbound commands return an empty list. `matches` returns
`false` for those commands. The manager includes Workdeck and extension command
IDs, and its event argument is a structural native key value. Prefer a named
command whenever a shortcut should be user-remappable. A local key parser is
available for shortcuts that intentionally are not commands.

## The pane is Workdeck's, the content is yours

Workdeck owns pane geometry, dividers, responsive omission, and the outer
review lifecycle. Render failures are contained to the pane; a failed Files-pane
replacement restores file navigation. Props carry exact `width` and `height` in
the host-owned rectangle. Extensions return declarative Ratatui nodes and never
receive a renderer, portal, or filesystem authority through the pane surface.

## Scrolling: the scrollbox ref contract

The behavior a list pane always needs is following the selection. Give rows
stable IDs, retain a host-owned pane handle, and scroll the selected row into
view from a state update:

```rust
fn render_files(props: &PaneProps, scroll: &mut PaneScroll) -> ViewNode {
    let rows = props.files.iter().flat_map(|file| {
        file.hunks.iter().map(move |hunk| {
            let selected = file.id == props.selected_file_id
                && Some(hunk.index) == props.selected_hunk_index;
            ViewNode::row(
                format!("row-{}-{}", file.id, hunk.index),
                [ViewNode::text(format!(" {}  {}", file.path, hunk.header))
                    .tone(if selected { Tone::Accent } else { Tone::Text })],
            )
            .on_mouse_up(Action::select_hunk(file.id.clone(), hunk.index))
        })
    }).collect();
    scroll.scroll_child_into_view(
        format!("row-{}-{}", props.selected_file_id.as_deref().unwrap_or(""),
            props.selected_hunk_index.unwrap_or(0)),
    );
    ViewNode::scrollbox(rows)
}
```

The handle surface mirrors the built-in Files pane:

- `scroll_child_into_view(id)` scrolls the descendant with that stable ID into view;
- `scroll_top` and `viewport.height` read the live offset and viewport rows; before
  the first layout pass, a read reports `0`, so viewport-dependent work belongs
  behind layout events;
- `on_scroll`, `on_layout_changed`, and `on_resized` report scrolling and pane
  resizes and return an unsubscribe guard.

That is enough to window a long list yourself: render rows near the viewport,
plus spacer rows sized from the same reads. Workdeck never scrolls a pane it
cannot see into; follow policy remains the extension's choice. The built-in
Files pane is the reference implementation for grouping, stat badges, and
selection behavior, but its geometry calculations remain host-owned.

## Pane state from events

Lifecycle handlers run outside the Ratatui render call. A pane rerenders when a
state snapshot changes, so connect the two with an extension-owned immutable
store. The event handler replaces the snapshot and notifies mounted panes; the
store keeps accumulating even while the pane is closed:

```rust
#[derive(Default)]
struct ViewedState {
    paths: Arc<Mutex<BTreeSet<String>>>,
    listeners: Arc<Mutex<Vec<Sender<()>>>>,
}

fn mark_viewed(state: &ViewedState, path: &str) {
    let mut paths = state.paths.lock().unwrap();
    if !paths.insert(path.to_owned()) { return; }
    for listener in state.listeners.lock().unwrap().iter() {
        let _ = listener.send(());
    }
}

fn register_progress(api: &mut ExtensionApi, state: ViewedState) {
    api.on("file_viewed", move |event, _ctx| {
        mark_viewed(&state, &event.file.path);
        Ok(())
    });
    api.register_pane(Pane::new("progress", "Viewed files", move |_props| {
        ViewNode::text(format!("{} files viewed", state.paths.lock().unwrap().len()))
    }));
}
```

Snapshots must be immutable from the host's perspective. Replace collections or
publish a new generation instead of mutating a value that a mounted pane may be
reading. Storing the only state in a component loses it whenever the pane closes
and unmounts. Workdeck owns event ordering, note snapshots, and selection
authority; the extension store is presentation state only.

Adapted from Hunk's MIT-licensed custom-pane guide, Copyright Modem Labs Inc.
The native page retains the documented pane geometry, replacement, props,
keybinding, scrolling, and event-store behavior while replacing React/OpenTUI
with declarative Rust and Ratatui.
