# Landing release metadata continuation

`cargo xtask changelog latest <markdown-file> <recorded-dates.json> [notes.json]`
prints the latest stable landing-page metadata, or JSON `null` when no stable
release is published. Date resolution uses recorded values, legacy heading dates
and read-only Git tag lookup, matching the native dates command.

The artifact selection excludes unpublished prereleases before series grouping.
Unpublished stable preparation headings remain in the input but cannot displace
the newest published stable release. The object contains version, minor series,
date and summary. A non-null tagline wins verbatim, including an empty string;
otherwise the resolved summary is converted to plain text and truncated to 72
UTF-16 units using the existing source-compatible helper.

The frozen `website-changelog-latest-oracle.json` captures the actual latest.json
artifact from `generateChangelogArtifacts` in both pinned source runtimes. Twelve
comparisons cover no publication, beta-only publication, stable fallback, a newer
beta alongside stable, empty/raw taglines and a truncated editorial summary.

This command implements an artifact component, not the complete site pipeline.
Full artifact output orchestration, source-format JSON layout, social-card head
integration and Zola rendering remain open. No ledger
mapping changes. MIT attribution is retained in the Rust module and
`THIRD_PARTY_NOTICES`.

Validation: all twelve frozen artifact comparisons pass. Strict xtask Clippy,
workspace formatting and diff whitespace checks pass. These focused results do
not establish the full artifact-generation or release gates.

## CLI integration

The native temporary-repository test exercises all six main artifact fixtures
through the real CLI with Workdeck branding, then checks that omitting notes
returns the exact captured fallback summary. It checks missing/extra arguments,
missing inputs, malformed JSON and invalid date/tagline types. Successful reads
preserve all input bytes; errors emit no stdout. The final directory inventory
contains only Git and the three inputs, with no generated site or Workdeck state.

All 15 changelog CLI tests pass. Strict xtask Clippy, formatting and diff whitespace
checks pass. No additional source interval is claimed complete by these checks.
