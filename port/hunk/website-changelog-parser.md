# Website changelog parser continuation

`cargo xtask changelog parse <markdown-file>` reads a changelog and prints parsed
release JSON without writing repository state. The new parser translates the
release-ordering, Changesets bullet cleanup, fenced heading/bullet handling,
legacy dates, highlights and empty-section filtering from pinned Hunk's
MIT-licensed `scripts/generate-changelog.ts`. Existing fragment commands remain
unchanged.

An executed differential comparison imported `parseChangelog` from disposable
checkouts of both pinned baselines using Bun 1.3.14. It compared the parsed JSON
values against the native command reading each original `CHANGELOG.md`:
all 48 main releases and all 47 stable releases matched. This compares complete
release values, not only counts. These live comparisons are not yet committed
frozen oracle fixtures and do not claim complete input-domain parity.

Native tests cover legacy dates, linked and bare-SHA entries, prerelease order,
highlights, empty sections, fenced headings and delimiter matching, and exact
version headings. Version digits are explicitly ASCII, matching JavaScript's
source regex rather than Rust regex's Unicode digit class.

This is only the parser portion. Full comparator edge cases, the complete source
test corpus, release grouping and overlays, date resolution, generated pages,
feeds, cards, cleanup safety and all output/check orchestration remain open.
No source interval or source file is marked complete by this change.

## Frozen full-history parser oracles

`website-changelog-parse-oracle.json` now records the executed `parseChangelog`
results from both pinned Hunk trees under disposable Bun 1.3.14. The native
`complete_pinned_changelogs_match_frozen_source_oracles` test reads each input
from its exact Git commit and compares every parsed release value with the
frozen output, including sections, entries, PR references, dates and highlights.
The test checks both baseline identities and the 48/47 release counts explicitly.
No TypeScript mirror or runtime is needed to execute this Rust test.

All four parser unit tests passed in 0.07 seconds. These frozen outputs cover the
historical changelog corpus only; they do not waive the outstanding source tests
or the remaining generator behavior listed above.
All three changelog CLI integration tests passed in 2.25 seconds. The parser
case checks exact JSON output, successful empty stderr, rejection of missing,
extra and nonexistent-file arguments, unchanged input content, and no new
repository-root entries after either successful or failed calls.
