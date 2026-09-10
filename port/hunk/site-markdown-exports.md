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
body Markdown. Draft pages are excluded. Bundled `index.md` pages use their
directory route; explicit page slugs replace the page or bundle basename, and
explicit paths override slugs. Non-string and unsafe routes are rejected, as
are section path/slug fields unsupported by Zola. Overview and onboarding pages
lead the corpus;
extension authoring, the legacy component reference and changelog pages are
excluded only from the compact corpus. Full output retains those pages.

The source policy comes from Hunk `2c00f435`'s MIT `website/astro.config.mjs`.
That interval remains unmapped: complete corpus migration, upstream plugin
output comparison, alternate routing/shortcode behavior, optional-link parity,
and development-server refresh integration are not complete. Generated output
has not been deployed. A valid local site build is not website parity.

Validation: nine `site_markdown` unit tests pass, covering source preservation,
compact/full exclusions, ordering, duplicate routes, missing HTML and output
collisions, dotted release routes, draft descendants, symlink parents, and
bundle and explicit-slug routing. `site check` additionally builds a disposable
five-entry site with native Zola and requires an HTML counterpart for every
export, checking that superseded routes are absent. This fixture was initially
verified with Zola 0.23.4. Development-server export refresh remains open.
The updated `site check` passed with all 21 current pages and seven sections,
as did `cargo clippy -p xtask --all-targets -- -D warnings`. Zola must be on PATH;
the first local invocation without that prerequisite failed before rendering.

At `e719b9c2`, the broader `cargo test -p xtask --all-targets -- --quiet`
completed with 372 unit tests and 39 integration/PTY tests passing. Seven unit
tests and one integration test were ignored; their oracle/environment gates
remain separate. Normal `site build` also generated the expected files beneath
`site/public`, which is ignored build output and is not committed or deployed.
