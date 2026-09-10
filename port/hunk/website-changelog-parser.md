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

## Publication CLI integration

The temporary-repository CLI test verifies absent publication dates produce no
default install target, prerelease-only dates still produce no stable target,
and a dated older stable version wins over newer unpublished and prerelease
entries. Stale date keys are ignored and release order comes from parsed versions,
not input order. Missing/extra arguments, missing files, malformed JSON and invalid
date value types fail without stdout or input rewrites. The directory inventory
remains exactly the temporary Git metadata and the two supplied input files.
All six changelog CLI integration tests pass. Generated-page integration remains
open, and no ledger mapping changed.

## Release body rendering

`cargo xtask changelog release-notes <markdown-file> <dates.json>` emits JSON
containing each minor series' release-body Markdown, using Workdeck PR links.
The renderer preserves version anchors, dated/unreleased metadata, section and
entry ordering, PR suffixes, and the no-user-facing-changes fallback.

`website-changelog-release-body-oracle.json` freezes the release-body portion
of actual `renderSeriesPage` output from both pins: 48 main and 47 stable
releases. Tests render with the upstream repository URL for exact comparison;
production uses Workdeck's URL. All twelve website unit tests pass. This is not
full-page parity: frontmatter, videos, cards, installers and adjacent navigation
remain to be integrated. Date formatting outside the baseline date corpus and
release-notes CLI integration still require tests. No ledger mapping changed.

## Date formatting edge cases

Executed fixtures from both pins cover 18 inputs each: ordinary and malformed
dates, missing components, whitespace, hexadecimal/binary/octal and exponential
fields, NaN, Infinity, and trailing components. The Rust formatter now handles
these source coercions and uses the existing ECMAScript-compatible number
formatter. All thirteen website tests pass, including the 95 release bodies.
These vectors are not exhaustive numeric-conversion proof; extreme radix values
and broader malformed-input differential testing remain open. No ledger mapping
changed.

## Release-notes CLI integration

The command now has an exact-output temporary-repository test for Workdeck PR
links, descending patch releases, version anchors, unreleased/no-change text,
dated entries, and preserved inline Markdown. Missing files/arguments, extra
arguments, malformed JSON and invalid date types fail without stdout. Input
bytes and the root directory inventory are checked after success and failures.
All seven changelog CLI integration tests pass. This does not establish full-page
or website parity, and no ledger mapping changed.

## Tooling gate refresh after release-body integration

At `ab271a84`, `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test -p xtask
--all-targets` passed: 258 unit tests, one existing ignored oracle-capture test,
seven changelog CLI tests, seven extension-catalog CLI tests, one workspace-test
CLI test, and one terminal-theme PTY test. This verifies the xtask package only,
not a refreshed full-workspace or cross-platform release gate.

The strict `cargo xtask port audit` exited 1: 1,257 baseline files, 1,440 records,
277 unmapped records and 92 pending upstream commits. Upstream was not fetched
for this refresh. The baseline generator remains unmapped; its passing partial
tests do not establish full-file implementation or release readiness.

## Factual summary fallback

`cargo xtask changelog resolved-summaries <markdown-file> <dates.json> [notes.json]`
adds the factual fallback used by release-page descriptions. Existing `summaries`
behavior is unchanged. Main counts all releases and includes published prereleases
in date spans; stable counts stable releases and excludes prereleases from spans.
Both pins' executed summary fixtures retain those differences. Stable comparisons
explicitly filter prereleases as its generator does; production follows main.

The 32 fixture cases exercise single/multiple releases, missing and empty dates,
date spans, prereleases, written highlights and editorial overrides. All fourteen
website unit tests pass. This remains partial generator work: resolved-summary
CLI integration and complete page rendering are not yet verified. No ledger
mapping changed.

## Resolved-summary CLI integration

An executable temporary-repository test now checks Workdeck-branded singular and
plural factual summaries, ordered date spans including a published prerelease,
and verbatim editorial overrides. Missing and extra inputs, missing files and
malformed/invalid date JSON fail without stdout. Supplied files remain unchanged
by the command and the directory inventory contains no new repository state.
All eight changelog CLI tests pass. Complete page rendering remains open, and
no ledger mapping changed.

## Large radix date rounding

A further executed two-pin fixture exposed a real per-digit accumulation error:
the binary integer ending in `11` above the 53-bit precision boundary rounded
down in Rust while both pins rounded up. The radix conversion now retains 53
significant bits, a rounding bit and a sticky remainder, then rounds once to even.
It continues validating digits beyond floating-point overflow. Sixteen frozen
comparisons cover binary, octal, hexadecimal, tie cases and overflow. All fifteen
website unit tests pass, including the previously failing case and all historical
release bodies. This is partial generator verification, not a ledger completion.
