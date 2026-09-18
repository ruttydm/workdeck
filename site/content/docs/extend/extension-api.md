+++
title = "Extension API"
description = "Declare native commands, themes, file views, VCS adapters, panes, transforms, and review events."
template = "docs.html"
+++

Workdeck extensions are trusted native executables. They communicate with the
host using JSON-RPC 2.0 messages separated by newlines on standard input and
output; stderr is reserved for extension logs. The host retains ownership of
Ratatui, terminal input, rendering, and process lifetime. An extension returns
declarative panes, rows, dialogs, notifications, and actions rather than
terminal escape sequences.

The API is versioned by the `api_version` in `workdeck-extension.toml`:

```toml
id = "review-tools"
name = "Review tools"
version = "1.0.0"
api_version = 1
executable = "bin/review-tools"

[capabilities]
commands = true
panes = true
file_views = true
```

An executable compiled for a newer API is rejected before registration. A
malformed manifest, duplicate id, failed handshake, deadline, or child crash
rolls back only that extension and becomes a startup notice.

## Server lifecycle

Use the SDK's stdio server to receive a bounded handshake, registration, and
host events:

```rust
use workdeck_extension_api::{ExtensionServer, Notification};

fn main() -> anyhow::Result<()> {
    ExtensionServer::stdio(|api| {
        api.on("startup", |_event, ctx| {
            ctx.notify(Notification::info("Review tools is ready"));
            Ok(())
        })
    })
}
```

Registration is accepted while the startup callback runs and is sealed before
the first review frame. Retained callbacks cannot mutate the registry after
that point. Requests have bounded payloads and deadlines; cancellation is
cooperative and is delivered before the host retires a child. Extensions are
fully trusted native programs, not security sandboxes.

## Commands

`register_command` adds a namespaced command and optional key binding. The
handler receives an immutable selection snapshot, navigation controls, dialog
requests, notification methods, and extension-owned capability handles. Built-in
commands are addressed by their public `workdeck.*` id and cannot be shadowed.
Relative movement counts are applied atomically; a command owned by another
extension is rejected.

Top-level command trees use `register_cli_command`. The extension owns raw
tokens below its lowercase-kebab name and may return a validated exit status or
delegate once to a built-in Workdeck command. Streaming stdin, stdout, and
stderr have explicit leases and size limits. Delegation cannot follow a stdin
read, another extension command, or a second delegation.

```rust
api.register_cli_command(
    CliCommand::new("review-tools", "Inspect a review", "<status|review>"),
    |args, context| async move {
        if args.first().map(String::as_str) == Some("review") {
            Ok(CliCommandResult::delegate(vec!["diff".into()]))
        } else {
            context.stdout().write_all(b"ready\n").await?;
            Ok(CliCommandResult::exit(0))
        }
    },
)?;
```

The host's static help lists built-in commands. `workdeck <extension-command>
--help` is passed to the extension so its summary and usage stay authoritative.

## Themes and languages

`register_theme` contributes a selectable theme with a lowercase id, label,
base theme, semantic colors, and exact syntax-scope overrides. Configuration
themes win over extension themes with the same id; extension order is stable.

`register_file_language` maps an extension, exact filename, or path/basename
glob to one of the vendored TextMate grammars. Matching is deterministic:
reserved Workdeck mappings run first, exact filenames outrank globs, and later
registrations win ties. A language registration selects an existing grammar; it
does not load code or a runtime parser.

## Version-control adapters

`register_vcs_adapter` contributes provider-neutral detection and operations for
working-tree diffs, revision shows, and stash shows. An adapter returns
`Changeset`, `DiffFile`, source snapshots, untracked files, and structured
user-fixable errors. Detection priority is explicit; the bundled Git, Jujutsu,
and Sapling providers remain available even when user extensions are disabled.
Watch plans, range endpoints, pathspecs, binary files, renames, and safe source
read limits are part of the adapter contract. See [VCS adapters](/docs/extend/vcs-adapters/).

## Panes and file views

`register_pane` declares a left, right, top, or bottom pane with bounded
preferred/minimum/maximum dimensions. The callback receives a frozen review
selection and current-line source address; the host lays it out with Ratatui.
The former sidebar API is retained as a compatibility alias, while pane ids are
namespaced as `<extension-id>:<pane-id>`.

`register_file_view` declares an alternate presentation for matching files. A
layout callback receives the public file model, typed changes, terminal width,
cancellation, and lazy exact-source reads. It returns symbolic rows, optional
fixed-height render descriptors, source bindings for notes, and hunk extents.
The raw diff remains the fallback. While an inline note is being drafted, the
host masks the alternate view and paints the raw diff so note geometry cannot
drift.

```rust
api.register_file_view(FileView::new("symbols", |input| async move {
    let source = input.read_document(ReviewSide::New).await?;
    Ok(FileViewLayout::rows(symbol_rows(source.as_deref().unwrap_or(""))))
}))?;
```

## Line highlighters

`register_line_highlighter` marks `[start, end)` character ranges addressed by
side and one-based source line. Marks survive split/stack layout, wrapping,
horizontal scrolling, and collapsed context. The six semantic tones are
`match`, `current`, `info`, `warning`, `error`, and `dim`; the host resolves
contrast against the actual row background while preserving syntax hues.

Highlighter results are pure derivations of the file and an invalidation epoch.
`ctx.highlights.refresh(id, file_id)` invalidates one view or one file. Invalid,
oversized, or failed results remove marks for that file only; they never change
text, navigation, or geometry.

## Changeset transforms

`transform_changeset` runs in registration order on initial load and every
reload. A transform may filter or reorder files, but must preserve the opaque
diff metadata for every retained file. Invalid or failing output is ignored and
the previous changeset remains active. Transform notifications are declarative
and are displayed by the host.

```rust
api.transform_changeset(|changeset| async move {
    Ok(changeset.with_files(
        changeset.files.into_iter()
            .filter(|file| !file.path.ends_with(".lock"))
            .collect(),
    ))
})?;
```

## Keyboard modes and dialogs

`register_keyboard_mode` adds an explicitly activated mode. It receives frozen
key snapshots after focused dialogs and file-view modes, but before ordinary
Workdeck commands. Return `Handled`, `Pass`, or `Exit`; `on_enter` and `on_exit`
callbacks reset extension-owned state. Only one session mode is active at a
time, and Escape, the Extensions menu, or the status badge can leave it.

Commands may open host-owned dialogs, select files and hunks, reveal exact
source lines, toggle panes, select or enter file-view modes, refresh highlights,
and send bounded notifications. Dialogs and focused inputs temporarily outrank
keyboard modes without destroying them.

## Review snapshots, notes, and workspace access

`ctx.review.snapshot()` captures immutable file identities and saved notes from
the shared review store. Selection and navigation payloads expose public hunk
summaries, source sides, and line spans; they never expose mutable renderer
internals. Notes are anchored to validated source coordinates and retain parent
identities for threaded replies.

The workspace capability can read the old or new reviewed document and can write
one file only after the host's consent check. A refusal is a typed result with a
human-readable reason. Paths are repository-relative, writes are bounded, and
the extension cannot replace the active changeset behind the host's back.

## Events and configuration

Extensions can subscribe to startup, reload, review navigation, view selection,
and note lifecycle events. Event payloads are immutable snapshots and are
delivered in sequence order. A slow listener is cancelled at the host deadline;
event failure never tears down the review.

Opaque extension settings live under `[extension.<id>]`. Workdeck merges the
user and repository tables field by field, reports repository overrides, and
trust-gates repository-declared executables. Imported legacy trust provenance
does not authorize a rewritten native executable; migration always requires a
fresh confirmation.

## Testing and distribution

Build an extension as a normal Rust executable and test the exact manifest and
directory layout users receive. `workdeck extension install` records the source
and commit, validates the manifest, and stores the installation under the
managed native-extension directory. Publish checksums, a license notice, and
an SBOM with every archive. The host enforces payload size, depth, deadlines,
cancellation, duplicate registration, trust, and crash isolation; it does not
pretend to sandbox trusted native code.

Existing TypeScript/OpenTUI extensions are never executed. Port them to the
native SDK and use the [extension authoring guide](/docs/extend/extensions/),
the [file-view examples](/docs/extend/file-previews/), and the compiled examples
under `examples/extensions/` as executable references.

Adapted from Hunk's MIT extension API guide, Copyright Modem Labs Inc. The
Workdeck API preserves the provider-neutral and review semantics while replacing
the JavaScript/OpenTUI host with Rust, Ratatui, Crossterm, and a bounded native
subprocess boundary.
