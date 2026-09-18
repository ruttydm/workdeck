# Release social metadata integration

Native index and series-page generators now select `release.html` and emit
`extra.social_image` plus `extra.social_image_alt` in Zola TOML frontmatter.
The values come from the existing source-derived card models, with separate
index and minor-series image URLs under Workdeck's `/changelog/og/` path.

The release template overrides the base metadata block with the page description,
OpenGraph image and alt tags, and the name-based Twitter image tag. The template
keeps default escaping for metadata attributes; only Zola-rendered Markdown is
inserted as safe HTML. Existing non-release pages retain the base description.
The release page has its own title and article heading.

This replaces the source generator's Astro `head` entries through Zola's template
boundary. It does not generate card images or write the generated Markdown into
the site. Tests check frontmatter paths, template selection and series alt text.
Zola is unavailable on the current host, so real template rendering, escaping,
accessibility, links and visual verification remain unproven. No source interval
or website release gate is marked complete by this change.

Validation: both series-page tests, both index tests and all 16 changelog CLI
tests pass. Strict xtask Clippy, workspace formatting and diff whitespace checks
pass. These tests verify generator behavior, not execution of the Zola template.
