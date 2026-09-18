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
output comparison, alternate routing/shortcode behavior, and optional-link parity
are not complete. Generated output
has not been deployed. A valid local site build is not website parity.

Validation: nine `site_markdown` unit tests pass, covering source preservation,
compact/full exclusions, ordering, duplicate routes, missing HTML and output
collisions, dotted release routes, draft descendants, symlink parents, and
bundle and explicit-slug routing. `site check` additionally builds a disposable
five-entry site with native Zola and requires an HTML counterpart for every
export, checking that superseded routes are absent. This fixture was initially
verified with Zola 0.23.4.
The updated `site check` passed with all 21 current pages and seven sections,
as did `cargo clippy -p xtask --all-targets -- -D warnings`. Zola must be on PATH;
the first local invocation without that prerequisite failed before rendering.

## Development preview

`cargo xtask site serve` captures authored site files plus the installed review
skill into disposable staging. Every generation passes the asset/skill checks,
a native Zola build, and Markdown export preflight before it reaches Zola's
live server. A 500 ms polling loop detects byte changes, including removals.
Invalid source edits leave the last valid staged generation in place and report
the error. A subsequent edit retries validation. Staging I/O failures terminate
the preview instead of claiming a partially copied generation is valid.

Generated Markdown and `llms*.txt` are injected into the staged static directory,
not `site/static` in the repository. Zola retains its own browser reload support.
The native wrapper does not add application JavaScript or a JavaScript runtime.
Ctrl-C/termination stops the owned child and releases temporary staging. The
wrapper uses the already-locked `ctrlc` crate; no new dependency version was added.

Zola 0.23.4 needs `--force` to rebuild its custom output directory. That flag is
restricted here to a fresh owned temporary directory, never authored `site/public`.
Its static watcher also leaves some deleted assets behind; Workdeck retires only
former static copies whose bytes still match the prior generation. Authored and
staged symlinks are rejected. These checks are not a concurrent-filesystem sandbox
or crash-atomic generation swap; publication still needs its separate release gates.

`cargo xtask site preview-check` (also run by `site check`) starts native Zola on
an ephemeral loopback port. HTTP assertions cover initial Markdown/full-corpus
exports, a renamed and edited page, HTML refresh, obsolete export 404, and keeping
valid output after an invalid candidate. It verifies the authored snapshot is
unchanged. Five unit tests cover synchronization, external-edit rejection,
static retirement ownership and symlink refusal. Two CLI tests retain the
asset/skill fail-before-render gates. The HTTP check initially exposed both the
missing force flag and stale deleted exports; both cases passed after correction.
Native Windows/Linux execution and full upstream website oracle parity remain
unverified.

The integrated `site check`, five preview unit tests, two asset/skill CLI tests,
and scoped xtask Clippy passed on the macOS host. A direct PTY `site serve` smoke
test returned the 52,132-byte full corpus and installation Markdown over HTTP;
Ctrl-C returned exit zero, removed its staging directory, and left no preview
child running. The staged-generation HTTP test exercises refresh helpers; the
PTY smoke covers startup/serving/shutdown, not automatic browser reload visuals.
The broader `cargo test -p xtask --all-targets -- --quiet` run passed 379 unit
tests and 39 integration/PTY tests. Seven unit tests and one integration test
remain ignored for their separate oracle/environment gates.

At `e719b9c2`, the broader `cargo test -p xtask --all-targets -- --quiet`
completed with 372 unit tests and 39 integration/PTY tests passing. Seven unit
tests and one integration test were ignored; their oracle/environment gates
remain separate. Normal `site build` also generated the expected files beneath
`site/public`, which is ignored build output and is not committed or deployed.
