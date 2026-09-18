# Inline edit extension

A miniature line editor for the file under review. Press `Ctrl-E`, type into the diff, press
`Ctrl-S`, and Workdeck asks before writing the file back to the working tree.

The example is an independent native extension executable. It composes Workdeck's immutable
workspace snapshots, interactive file-view mode, scoped layout refresh, host-owned confirmation,
and guarded working-tree write capabilities. It is not bundled or loaded automatically.

Build it with `cargo build -p workdeck-examples --bin workdeck-example-inline-edit-extension`, copy
the executable to this directory as `bin/workdeck-example-inline-edit-extension`, and install the
directory through `workdeck extension install`.

Keys retain Hunk's behavior: arrows move by Unicode grapheme, printable characters insert,
Backspace deletes or joins, Enter splits, `Ctrl-S` requests a consented write, and Escape discards
unsaved edits. `]`, `?`, and `q` pass through to review navigation, help, and quit. The layout
preserves CRLF, LF, and CR terminators, marks modified buffers, truncates by terminal cell width,
and retains exact source-line provenance across splits and joins so review notes remain anchored.

The host owns terminal rendering, confirmation, path validation, filesystem mutation, and review
reload. The trusted extension owns only its edit buffer and declarative rows.

See [Native interactive file views](../../../docs/native-interactive-file-views.md) for the full
protocol lifecycle and write-safety contract.
