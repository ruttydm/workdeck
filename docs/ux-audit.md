# Workdeck UX Audit

## Current Direction

Workdeck should behave like a compact repo map, not a dashboard. The primary UI should keep the user oriented around:

- Where am I in the repo?
- What changed?
- What is selected?
- What can I do next?

The best use of space is the tree/list plus preview. Side metadata panes are not worth the width in narrow or medium terminal sidecars.

## Changes Made From Audit

- Removed the wide Details pane.
- Collapsed title and tabs into a single header row.
- Switched panels to top-only rules to save vertical and horizontal space.
- Added a context-rich footer with active tab, focus pane, grouping mode, selection index, selected object, stage, and churn.
- Made footer key hints contextual instead of generic.
- Reworked navigation toward a lazygit-like model: `h/l` collapse, expand, and move between tree and preview instead of switching tabs.
- Added preview focus with independent `j/k/g/G` scrolling.
- Added selectable collapsible directory rows in Changes and Files.
- Added narrow Files drill-down navigation with breadcrumb titles so 40-column panes browse one folder level at a time instead of rendering the full repo tree.
- Simplified Changes rows from noisy staged/unstaged brackets to compact scan glyphs:
  `+` untracked, `M` modified, `A` added, `D` deleted, `R` renamed, `S` staged, `S+` staged plus unstaged.
- Compressed churn in the tree to a single trailing token such as `+12`, `-4`, or `+4/-2`.
- Added `theme = "auto"` with light/dark syntax theme selection.
- Removed forced syntax backgrounds.
- Clamped syntax foregrounds so near-white code is readable on light terminals and near-black code is readable on dark terminals.

## Remaining High-Value UX Improvements

1. Add a compact command palette overlay.
   `?` is useful, but a command palette would make actions discoverable without occupying persistent space.

2. Add inline issue badges.
   Files linked to local issues should show a small `WD-12` marker in Changes and Files.

3. Add richer change intent grouping.
   Current grouping is directory or status. A useful next step is generated/test/docs/config buckets inferred from path patterns.

4. Add visual selection breadcrumbs.
   The footer now shows selection context. A future improvement is a short breadcrumb above preview, for example `Changes > crates/workdeck-cli/src/views/mod.rs`.

5. Add theme presets.
   `auto`, `light`, and `dark` are enough for now. Later presets can tune borders, selected rows, and diff colors per terminal background.

## Design Principles

- Prefer one-line context over side panels.
- Keep persistent UI chrome to two rows: header and footer.
- Use preview width for actual code, not metadata.
- Make selected state obvious but avoid large inverted blocks.
- Treat light terminals as first-class, not an afterthought.
