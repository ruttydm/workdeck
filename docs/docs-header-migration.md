# Documentation header migration

Hunk's `DocsHeader.astro` composes `BrandHeader` with route-aware Docs versus
Changelog state and a Starlight Search slot. Workdeck keeps that composition in
`site/templates/base.html`: documentation and changelog routes expose the
Workdeck brand controls, `aria`-labelled navigation, and a plain GET form that
submits a query to the repository's documentation search surface. The form is
usable without application JavaScript and remains visible in static Zola
deployments; the native site deliberately ships no client runtime or
application JavaScript. The native site uses no application JavaScript.

The native `.docs-header-tools` styles retain the source's flex alignment,
zero-width shrink behavior, and 14-pixel gap while adding Workdeck focus and
contrast tokens. `xtask::site_links::verify_docs_header` reads the exact pinned
source blob through Git and checks every component marker, native route branch,
control, and style. The Astro/Starlight component is not retained or executed.
