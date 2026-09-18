# Native file-view gallery

Three opt-in presentations exercise Workdeck's declarative native-row contract against checked-in,
realistic file pairs. Build and stage the extension, open one review, then press **F8** or choose
**Extensions -> Toggle native demo for current file**. Press F8 again to restore the raw diff.

```console
cargo xtask extension stage-example file-view-gallery
```

## 1. Change atlas

Nested rows, responsive meters, semantic color, and selected-hunk styling summarize a multi-hunk
Rust refactor. The view needs no source parser and works from public hunk/change metadata alone.

```console
workdeck --extension target/workdeck-extension-examples/file-view-gallery difftool \
  --mode stack \
  examples/extensions/file-view-gallery/fixtures/change-atlas/before.rs \
  examples/extensions/file-view-gallery/fixtures/change-atlas/after.rs
```

The compatibility matcher also accepts Hunk's JavaScript and TypeScript file families.

## 2. CSS palette delta

The extension lazily receives both immutable documents, associates changed opaque three- or
six-digit hexadecimal custom properties with each real diff hunk, and paints old/new terminal color
swatches inside deterministic two-row rectangles.

```console
workdeck --extension target/workdeck-extension-examples/file-view-gallery difftool \
  --mode stack \
  examples/extensions/file-view-gallery/fixtures/css-palette/before.css \
  examples/extensions/file-view-gallery/fixtures/css-palette/after.css
```

## 3. Dependency delta

A conservative dependency-file parser supports both `Cargo.toml` and Hunk-compatible
`package.json` input. Patch-only changes emphasize the patch number, minor upgrades emphasize the
minor number, and major upgrades emphasize the complete old/new strings. Invalid syntax or
unavailable source falls back to raw diff.

```console
workdeck --extension target/workdeck-extension-examples/file-view-gallery difftool \
  --mode stack \
  examples/extensions/file-view-gallery/fixtures/package-dependencies/before/Cargo.toml \
  examples/extensions/file-view-gallery/fixtures/package-dependencies/after/Cargo.toml
```

## Mixed five-file review

The Rust launcher shows all three preview types retained together between ordinary raw diffs and
enough content to exercise continuous-stream scrolling:

```console
cargo run -p workdeck-examples --bin workdeck-example-file-view-gallery-mixed-review
```

See [the activation sequence](./mixed-review/README.md).

## Contract illustrated

- Every component stays inside a declared fixed-height row; geometry, scrolling, windowing, hunk
  navigation, selection state, theme resolution, and input remain host-owned.
- Every row keeps useful symbolic spans for local error fallback.
- Layout captures semantic data from immutable source snapshots. The subprocess returns only bounded
  declarative nodes with symbolic or literal colors; it never owns Ratatui or terminal output.
- The demos intentionally expose no pointer actions. The registered F8 command is the interaction
  path.
- Layouts return no presentation when exact source is unavailable or no supported semantic row can
  be attributed. Neutral summary rows retain positional navigation for non-semantic hunks inside a
  mixed file.
- Exact-source ranges remain conservatively bound. If a saved note cannot resolve uniquely inside an
  alternate view, Workdeck falls back to the complete raw file diff.

The gallery is experimental and never loads unless explicitly passed or installed through the
native-extension trust flow. See [native file-view components](../../../docs/file-view-native-components.md)
for the full host contract.
