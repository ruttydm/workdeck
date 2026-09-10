# Native website Markdown exports

`cargo xtask site exports-plan` is read-only and prints a deterministic JSON map
of relative output paths to generated Markdown. `site build` and `site check`
emit that plan after Zola rendering. Each page export requires a corresponding
rendered HTML page; preflight rejects existing output files and symlink parents.
Writes use exclusive creation and verify the resulting text. This is generated
build output, not crash-atomic publication or protection against concurrent
filesystem replacement.

Current scope is TOML-frontmatter Markdown beneath `site/content/docs` and
`site/content/changelog`. Exports remove frontmatter, add the title, and preserve
body Markdown. Draft pages are excluded. Explicit slugs are rejected rather than
silently routed incorrectly. Overview and onboarding pages lead the corpus;
extension authoring, the legacy component reference and changelog pages are
excluded only from the compact corpus. Full output retains those pages.

The source policy comes from Hunk `2c00f435`'s MIT `website/astro.config.mjs`.
That interval remains unmapped: complete corpus migration, upstream plugin
output comparison, alternate routing/shortcode behavior, optional-link parity,
and development-server refresh integration are not complete. Generated output
has not been deployed. A valid local site build is not website parity.

Validation: seven `site_markdown` unit tests pass, covering source preservation,
compact/full exclusions, ordering, duplicate routes, missing HTML and output
collisions, dotted release routes, draft descendants and symlink parents;
integrated `site check` also passes on the current content.

At `e719b9c2`, the broader `cargo test -p xtask --all-targets -- --quiet`
completed with 372 unit tests and 39 integration/PTY tests passing. Seven unit
tests and one integration test were ignored; their oracle/environment gates
remain separate. Normal `site build` also generated the expected files beneath
`site/public`, which is ignored build output and is not committed or deployed.
