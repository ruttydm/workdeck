# Workdeck Component Provenance

This register is the release authority for externally influenced Workdeck UI code and assets. Workdeck ships an independently styled Rust/Dioxus implementation. `A` is direct permissive reuse, `B` is modified permissive adaptation, `C` is behavior-only reference, and `D` is prohibited.

## Source-owned Dioxus primitives

- Workdeck paths: `crates/workdeck-ui/src/components/primitives.rs`, `components/shell.rs`, `components/markdown.rs`, `components/gallery.rs`
- Upstream: Dioxus Components, revision `bf007c15d0cf4d04d3181cc46cf12325aa773955`
- Reviewed upstream files: `primitives/src/progress.rs`, `separator.rs`, `collapsible.rs`, `tabs.rs`, `tooltip.rs`, `dialog.rs`, `popover.rs`, `dropdown_menu.rs`, `toolbar.rs`, `virtual_list.rs`
- License choice: MIT; exact text is in `third_party/dioxus-components/LICENSE-MIT`
- Class: B
- Changes: reduced to Workdeck-owned props, CSS tokens, focus-visible styles, native HTML semantics, stable labels, controlled state, and Rust-only rendering. Workdeck does not copy or link the upstream focus-trap JavaScript, TypeScript source, preview application, CSS, or branding.
- Tests: SSR landmarks, Markdown injection tests, UI interaction suite, accessibility suite, component-gallery snapshots.

## Workdeck shell and design language

- Workdeck paths: `crates/workdeck-ui/src/components/shell.rs`, `app.rs`, `tailwind.css`
- Comet/Zeron: `zeronsh/comet` at `2ebe6ed06f8e7e1ed911c0adf31ec91ad2e94274`, MIT
- T3 Code: `pingdotgg/t3code` at `348367dcc6e1ac8b11baf94a31c47f33d46313cd`, MIT
- Orca ADE: `stablyai/orca` at `5e900b10b31f12db885e4448c3e1f6300e066efb`, MIT
- Reviewed behaviors: 44 px icon rail, one-row workspace header, contextual tabs, compact hierarchy rows, titlebar drag boundaries, content-owned master/detail density, progressive side panes, quiet selected washes, command palette, and responsive overlay behavior.
- Class: B for interaction/layout grammar, with independent Workdeck Rust/RSX/CSS implementation.
- Changes: original graphite/sage token system, independent component structure, accessibility contracts, renderer-independent protocol, review-first navigation, no upstream logos or strings.
- Distributed notices: `third_party/comet/LICENSE`, `third_party/t3code/LICENSE`, `third_party/orca/LICENSE`.

## Pull-request master/detail review

- Workdeck paths: `crates/workdeck-ui/src/surfaces/pull_requests.rs`, `crates/workdeck-presenter/src/client.rs`
- Upstream behavior reference: T3 Code revision `348367dcc6e1ac8b11baf94a31c47f33d46313cd`, particularly `apps/web/src/routes/_chat.pull-requests.tsx`, `components/pullRequest/PullRequestRow.tsx`, `PullRequestDetailPanel.tsx`, `PullRequestSummaryTab.tsx`, and `PullRequestCodeTab.tsx`.
- License: MIT
- Class: B
- Changes: independent Rust/RSX/CSS implementation with a dense filterable master, 54px rows, content-owned detail, Summary/Timeline/Code contexts, and progressive review readiness. Provider data is immutable; mutations, comments, reactions, merges, checkout, and review submission are excluded. Workdeck adds frozen checkpoints, incoming semantic units, read-only URL validation, and durable local review creation.

## Git graph and range review

- Workdeck paths: `crates/workdeck-git/src/history.rs`, `crates/workdeck-ui/src/surfaces/git.rs`
- Source: Workdeck-owned Git topology and SVG rendering; Comet/Zeron is a density/geometry reference under MIT.
- Class: Workdeck-owned/B
- Changes: Rust computes lanes and visible SVG rows, linked refs, HEAD, detached state, WIP, filtering, incremental pages, and two-point immutable review ranges. No Git mutation control exists.

## Geist typography

- Workdeck paths: `crates/workdeck-ui/assets/fonts`
- Source: Geist Project font files originally obtained through the audited Comet checkout.
- License: SIL Open Font License 1.1; exact text `crates/workdeck-ui/assets/fonts/Geist-OFL.txt`
- Class: A
- Changes: none to font programs or reserved names.

## Syntax highlighting

- Workdeck paths: `crates/workdeck-analysis/src/lib.rs`, `crates/workdeck-presenter/src/client.rs`, `crates/workdeck-ui/src/components/code.rs`, `crates/workdeck-ui/src/surfaces/review.rs`, `crates/workdeck-ui/tailwind.css`
- Sources: upstream tree-sitter grammar packages and their distributed highlight queries, resolved and licensed through Cargo; no editor theme, CSS, or proprietary token palette is copied.
- Class: Workdeck-owned integration over permissively licensed parser/query data.
- Changes: query caching, overlap normalization, UTF-8-safe line splitting, renderer-independent semantic tokens, original graphite light/dark colors, Geist Mono code typography, sticky gutters, and unified/split/source accessibility semantics.
- Tests: every grammar query compiles and every visual capture maps; polyglot, multiline, presenter, fixture, renderer, Playwright typography, Axe, and responsive light/dark snapshots.

## Workdeck product artwork

- Workdeck paths: `assets/Workdeck.svg`, `assets/Workdeck.icns`, `favicon.png`
- Source: original Workdeck artwork created in this repository.
- License: Workdeck MIT
- Class: Workdeck-owned
- Notes: no logo, glyph, illustration, or screenshot from an inspiration repository is reused.

## Behavior-only and prohibited sources

### Waku

- Repository: `egoist/waku` at `cc8b2cb0ffe9074c7a622e0b2caee22d708ed307`
- License: GPL
- Disposition: C, behavior-only. No Waku source, CSS, constants, comments, tests, icons, assets, font files, or generated output may enter Workdeck.

### GitKraken and Codex

- Disposition: C, visual and behavioral reference only. No proprietary source or asset is available or copied.

### GPUI, Zed, and Longbridge

- Disposition: D in the shipping graph after Dioxus cutover. The temporary source under `legacy/` is non-shipping parity history and is deleted at final cutover. No GPUI/Zed/Longbridge package may resolve in `cargo tree` or enter `Workdeck.app`.

## Release assertions

`scripts/license-gates.sh` checks the pinned revisions, exact Dioxus Components MIT text, Cargo license expressions, MPL allowlist, inspiration exclusion, absence of unapproved GPL material, absence of GPUI/Zed/Longbridge dependencies, Workdeck SBOM identity, and the Rust-only shipped-frontend policy.
