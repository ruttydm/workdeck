# Pinned changelog page migration

The 21 series pages under `website/src/content/docs/changelog/` are generated
content, not hand-authored application code. Workdeck's native Rust/Zola
generator in `xtask/src/changelog/website/pages.rs` rebuilds each page from the
pinned `CHANGELOG.md`, `website/releases/dates.json`, and `website/releases/notes.json`
inputs. `verify_pinned_editorial_inputs` reads the exact pinned blobs through
`git show`, checks every series page, and compares the complete
`## Releases in this series` section: release anchors, headings, dates, entries,
and pull-request links. The comparison applies only the documented branding and
repository substitution (`Hunk`/`hunk.dev`/`modem-dev/hunk` to Workdeck values).

The native page intentionally replaces Astro front matter with Zola metadata,
uses the native `cargo install` and `workdeck update` installation path, and
keeps the surrounding navigation and structured metadata in Rust templates.
Those adaptations are covered by the page and artifact tests; the source
generated release body is not copied into the final tree and no JavaScript
runtime is required.

Evidence:

- `xtask/src/changelog/website/artifacts.rs#verify_pinned_editorial_inputs`
- `xtask/src/changelog/website.rs#tests::all_pinned_release_bodies_match_rendered_page_oracles`
- `xtask/src/changelog/website/pages.rs#tests::page_composes_sections_and_zola_metadata_without_npm`

The source pages remain attributable to Hunk's MIT license in
`THIRD_PARTY_NOTICES`; each ledger interval is recorded as a
`rust-generated-replacement` rather than a blanket documentation waiver.
