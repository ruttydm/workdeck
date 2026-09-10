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

## Minor-release grouping

`cargo xtask changelog series <markdown-file>` now projects releases into minor
series, sorting both series and their releases newest-first. The frozen
`website-changelog-series-oracle.json` records source results for reversed parsed
input from both pins: 21 main groups and 20 stable groups. The Rust regression
checks exact group membership/order, unchanged complete release values, and empty
input. All five parser/grouping tests passed in 0.07 seconds. Both read-only
commands are listed in xtask help. No ledger interval is newly mapped.

## Release-date resolution

`cargo xtask changelog dates <markdown-file> <recorded-dates.json>` now emits a
resolved date map without writing inputs. It preserves recorded values, prefers
legacy heading dates over Git lookup, and omits unresolved versions and stale
recorded versions no longer present in the changelog. Lookup tries annotated
tagger dates and then tagged commit dates. Missing date files mean an empty map;
malformed JSON and other read errors fail rather than silently clearing history.

The native regression checks lookup invocation boundaries, recorded and heading
precedence, unresolved and stale entries, key ordering, and tag-date string
selection. All six parser/grouping/date tests passed in 0.06 seconds. Real Git-tag
fixtures, date-command integration checks and frozen date oracles remain to be
added before claiming complete date-resolution parity. No ledger mapping changed.

## Executed date oracles and real tags

`website-changelog-dates-oracle.json` now freezes both pins' resolved date maps,
key order and lookup call sequence for recorded, legacy, missing and stale entries.
The main pin also provides exported `resolveTagDate` vectors; stable does not
export that helper, which is explicitly recorded rather than treated as a pass.
All seven parser/grouping/date unit tests passed in 0.06 seconds.

The CLI regression creates a real temporary Git repository with an annotated
tag dated one day after its commit and a lightweight tag pointing at that commit.
It checks the distinct publication dates, unresolved versions, heading dates,
recorded overrides, stale-entry omission, and unchanged inputs. A missing date
file is not created; malformed JSON produces an error and no output or rewrite.
This remains partial generator evidence, not a new ledger mapping.

## Heading whitespace parity

Both pinned runtimes accept BOM-trimmed version headings and reject NEL-wrapped
versions. Rust's default trim did the opposite. Version-heading trimming now uses
the ECMAScript whitespace set. The executed two-pin results are frozen in
`website-changelog-whitespace-oracle.json`; all eight website unit tests pass.
This is a heading-specific correction; other parser whitespace operations and
the remaining page generator still require parity work. No ledger mapping changed.

## Body whitespace follow-up

Parser trimming and regex whitespace now share ECMAScript's whitespace semantics
for fences, entries, link preambles, references, nested bullets, legacy headings,
section titles and Highlights. A second executed oracle fixture covers six
characters (BOM, NEL, file separator, NBSP, line separator and ideographic space)
across four input shapes on both pins: 48 comparisons. All nine website unit tests
pass, including the complete baseline changelog fixtures. The generator remains
partially ported and its ledger record remains unmapped.

## Editorial summaries

`cargo xtask changelog summaries <markdown-file> [notes.json]` emits read-only
JSON summaries by minor series. Nonempty editorial summaries are preserved
verbatim; otherwise it finds the newest Highlights lead paragraph and unwraps
the source-supported Markdown constructs. Bullet-only highlights fall through
to older releases. Missing summaries are represented as JSON null.

The Rust Highlights splitter, plain-text conversion and summary selection are
checked against executed frozen fixtures from both pins, including empty leads,
bullet-only content, wrapped prose, BOM/NEL and editorial overrides. All ten
website unit tests pass. This does not yet generate pages, feeds or social cards;
CLI integration and the full overlay schema remain to be completed. No ledger
mapping changed.

## Summary command integration

The summaries CLI now has an executable temporary-repository regression for
descending series order, bullet-only fallback to older prose, null summaries,
verbatim editorial overrides, empty overrides and irrelevant overlay keys.
It also checks missing arguments/files, extra arguments, malformed JSON and
non-string summaries fail without stdout or rewriting inputs. Directory contents
are checked to ensure no repository state or generated files appear. All five
changelog CLI integration tests pass. This does not close page generation or the
remaining full overlay schema work.

## Publication selection

`cargo xtask changelog publication <markdown-file> <dates.json>` reports published
versions, published stable versions, and the latest stable version without writes.
It follows main's distinct publication and stable-publication predicates. The
stable pin has no `isStablePublished` export: its `isPublished` already excludes
prereleases. Frozen executed fixtures record that difference explicitly, testing
no dates, prerelease-only dates, mixed publication, empty recorded dates and stale
date keys. Empty strings count as present in these source predicates (not date
resolution). Eleven website unit tests pass; CLI and generated-page integration
remain open. No ledger mapping changed.
