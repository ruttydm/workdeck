# Landing page migration

The pinned `website/src/pages/index.astro` page is replaced by the native
`site/templates/index.html` Zola template. The complete component graph is
preserved in the same order: installation choices, release metadata, theme
previews, community videos, six feature chapters, quotes, feature links, and
the final quick-start call to action.

The page is branded Workdeck and uses the generated
`site/data/latest-release.json` artifact rather than a network request for a
GitHub star count. The artifact is checked against the pinned release metadata
and the page is covered by `xtask` source-shape and asset checks. Ratatui and
Rust ownership are documented in the page itself; no application JavaScript,
Hunk distribution alias, npm package, or browser runtime is shipped.
