# Native changelog index continuation

`cargo xtask changelog index <markdown-file> <dates.json> [notes.json]` renders a
Zola Markdown index to stdout. It does not write site files or repository state.
Notes use minor-series keys with optional `summary` fields, as in the existing
page and feed commands.

The index preserves pinned main's newest published stable-series selection,
prerelease/unreleased labels, date spans, counts of all releases and changes,
editorial-only summary paragraphs, 240-unit truncation, and links to every series.
The introductory links target Workdeck RSS and the authoritative changelog.
Frontmatter is TOML with an explicit `changelog/` path, replacing Astro YAML.

`website-changelog-index-oracle.json` freezes complete source pages from both
pins: four main publication states and two stable states without published
prereleases. Main's four cases compare the entire body beginning with the RSS
link; only branding/repository links change. The two stable cases explicitly
record a superseded behavior: an unpublished prerelease contributes zero to the
release count in stable and one in main. Their output is intentionally different,
not normalized to manufacture parity. Frontmatter and generated notices are
migrated rather than compared byte-for-byte. A separate source SAMPLE test checks
the five original index-test behaviors and the Zola path metadata.

This is partial generator implementation. Social-card head integration, actual
Zola template rendering and artifact orchestration remain open.
The initial implementation mapped no ledger record. The subsequent
[index-test translation](changelog-index-tests.md) maps only the complete original
index test block, preserving all nine source assertions. Attribution remains with Modem
Labs Inc. under MIT, as recorded in the Rust source and `THIRD_PARTY_NOTICES`.

Validation: both index unit tests pass, including four complete main body
comparisons and the two explicit stable divergences. Strict xtask Clippy,
workspace formatting and diff whitespace checks pass. No full workspace or
website rendering gate is claimed by these focused checks.

## CLI integration continuation

The temporary-repository CLI test compares all four main fixture bodies through
the real executable, validates the Zola title/path metadata, and verifies that
omitting notes removes the editorial override. Missing/extra arguments, missing
files, malformed date JSON, non-string dates and invalid notes shapes must fail
with no stdout. Inputs remain unchanged and the directory inventory contains
only the Git directory and the three input files: no Workdeck state or generated
site files are created. These are additional native workflow checks, not new
source-ledger mappings.

All 14 changelog CLI integration tests pass after this addition. Strict xtask
Clippy also passes.
