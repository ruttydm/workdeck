# Original version-ordering test translation

Pinned main `scripts/generate-changelog.test.ts` bytes 1549–2294 (end exclusive),
lines 75–101, contain four tests translated in `xtask/src/changelog/website.rs`:

- `source_versions_order_releases_newest_first`
- `source_versions_sort_stable_above_own_prereleases`
- `source_versions_derive_minor_series`
- `source_versions_build_readable_anchor`

All original arrays, expected orderings and string assertions are retained. The
production grouping and release-body renderer now share named minor-series and
anchor helpers; the minor helper also preserves the source's missing-component
defaults. Both pins passed four source tests and five assertions under Bun 1.3.14.
All 52 native website tests passed, including these four translations and the
full pinned release-body oracles. Only this complete 745-byte test block is mapped.
The runtime generator remains incomplete and unmapped.
