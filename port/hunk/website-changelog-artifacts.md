# Native changelog artifact composition

`cargo xtask changelog artifacts <markdown-file> <recorded-dates.json> [notes.json]`
returns a JSON object mapping repository-relative destination paths to generated
UTF-8 file contents. The command does not write those destinations or remove
anything. Dates resolve through recorded values, legacy headings and Git tags.

The composition connects native date resolution, publication filtering, latest
stable metadata, social-card models, source-format JSON, RSS, the index and
individual Zola pages. Unpublished prereleases disappear before grouping;
unpublished stable preparation pages remain. Neighbor links and current-release
labels use that filtered series list. Notes feed summaries, taglines, video and
documentation links through their existing typed renderers.

Output destinations:

- `site/data/releases/dates.json`, `latest.json`, `cards.json`;
- `site/static/changelog/rss.xml`;
- `site/content/changelog/index.md` and one `MINOR.md` per included series.

The integration test verifies an unpublished stable heading remains, an undated
beta does not produce a page/feed entry, tagged beta discovery restores both,
the latest stable remains current, neighbor links are connected and card latest
flags agree with release metadata. This is a native integration check, not a
complete frozen source-artifact comparison.

Atomic write/check orchestration, orphan cleanup, missing-card image checks,
full artifact differential fixtures and real Zola
rendering remain open. No source interval or release gate is marked complete.
Hunk MIT attribution is retained in the composition module and notices.

Validation: the artifact-composition unit test and all 16 existing changelog CLI
tests pass. Strict xtask Clippy, formatting and diff whitespace checks pass.

## CLI integration continuation

The real-executable temporary-repository test verifies artifact counts and paths
before and after beta publication, stable preparation retention, editorial page
content, landing tagline precedence and feed membership. Every returned artifact
destination is checked absent from disk. Input bytes remain unchanged; the final
repository inventory contains only Git and the three inputs. Missing arguments,
extra arguments, missing files, malformed dates, non-string dates and malformed
video notes fail with no stdout and without writing artifacts.

All 17 changelog CLI tests pass after this addition, along with strict xtask
Clippy, formatting and whitespace checks. Source-ledger mappings are unchanged.
