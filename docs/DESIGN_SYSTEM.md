# Workdeck Design System

Workdeck is dense, quiet, and review-first. It borrows interaction discipline from Comet/Zeron, T3 Code, and Orca without copying their branding.

## Foundations

- Geist for UI, Geist Mono for code and identifiers.
- Four-point spacing rhythm.
- Graphite neutral surfaces with a restrained sage/teal selection accent.
- One-pixel separators; 6, 8, and 10px radii.
- No gradients, glass decoration, oversized cards, or dashboard ornament.
- Motion is 100–180ms and disabled under reduced motion.
- Light and dark modes preserve hierarchy, focus, syntax, and diff meaning.

### Code

Code is a first-class reading surface, not secondary metadata. Diffs and source use the bundled variable Geist Mono at 12px/21px with contextual/common ligatures, a slashed zero, four-space tabs, tabular line numbers, and no synthetic faces. Unified and split diffs share the same line model, semantic spans, selection treatment, sticky gutters, and two-pixel addition/removal edge marker. Source text is never rewritten for presentation.

The semantic palette distinguishes keyword, type, function, string, number, comment, attribute, variable, parameter, property, constant, built-in, macro, tag, namespace, label, escape, embedded content, operator, and punctuation. Light and dark values are authored independently for their code backgrounds; diff fills remain quiet enough that token meaning is not lost. File headers always expose a compact language label.

All tokens live in `crates/workdeck-ui/tailwind.css`; `assets/workdeck.css` is deterministic output from the pinned standalone Tailwind 4.3.3 binary.

## Geometry

| Element | Value |
| --- | ---: |
| Rail | 44px |
| Workspace header and contextual tabs | 52px |
| Status bar | 24px |
| Navigator default | 256px |
| Inspector default | 320px |
| Minimum useful window | 900 × 600 |

Workdeck has one persistent horizontal header. Surface actions belong in a compact content-owned toolbar or an overflow menu; a second global context-toolbar row is prohibited.
On macOS, one unified titlebar spans the complete window. The vertical rail begins below that 52px row, and titlebar content starts after an 80px native-control gutter. No vertical pane boundary passes through the traffic-light cluster; the product name in the titlebar is the only persistent shell brand mark.

## Hierarchy

The rail changes global scope. It owns Updates, Workspaces, Git, CI, Search, and Artifacts. Git's titlebar contains the only nested workspace switcher: Commits and Pull requests. Both Git modes share one task grammar: a resizable activity list on the left, the selected diff in the center, and a resizable directory tree of changed files on the right. Selection opens content immediately and never navigates through a summary or review-set screen. Git references live in a compact popover so they do not consume a permanent fourth column. Workspaces may still expose its project index and evidence inspector; task surfaces do not also render generic navigator or inspector chrome.

## Components

Source-owned styled primitives include buttons, icon buttons, badges, status dots, progress, empty/error/loading states, segmented tabs, search fields, dialogs, tooltips, separators, scroll areas, listboxes, and treegrids. Workdeck-specific composites include rail, navigator, task sidebars, splitters, update rows, changed-files tree, canonical tree, syntax source, diffs, Git graph, PR/commit three-column workspace, CI explorer, artifact frame, and command palette.

Every visible action must have a working callback, disabled state with a reason, or be removed. Icon actions have authored accessible names and native focus rings.

## Responsive rules

- Wide: content owns the available width; Workspaces may show its navigator and Workspaces/Git may show an explicitly requested inspector.
- Intermediate: global inspector becomes an overlay; review’s canonical pane hides before content becomes cramped.
- Minimum: the Workspaces navigator becomes a drawer and any requested inspector is mutually exclusive; the selected work remains usable.
- Below product minimum is not supported by the native window.

Pane widths, global collapsed state, selected destination/tab, and search history persist. Task-owned panes expose visible collapse/reveal controls and preserve their width when reopened. Data snapshots and obsolete revisions do not.

## Review colors

Green means added/passed/reviewed, red means removed/failed, amber means waiting/attention, violet means incoming semantic change, and blue is focus. Color is always paired with copy, iconography, structure, or accessible state.
