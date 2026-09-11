# Extension directory migration

The pinned `website/src/pages/extensions.astro` directory is represented by
`site/templates/extensions.html` and the checked `site/data/legacy-extensions.json`
catalog. All 16 listings, category facets, repository links, versions, API
versions, summaries, and capability labels remain in the rendered page.

The directory is deliberately a static migration surface. Hunk's client-side
search/sort/paging and clipboard installer are not executed: Workdeck does not
run TypeScript or ship an `extension install` path. Every listing is marked as
requiring a Rust rewrite, and the native extension host's trust/full-permissions
boundary is the authoritative replacement; native extensions run with full permissions. The page is no-JavaScript and the
catalog is compared to the pinned source literal by `cargo xtask verify`.
