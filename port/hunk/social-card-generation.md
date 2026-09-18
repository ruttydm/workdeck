# Native social-card generation port

`cargo xtask social-cards-plan <cards.json> [slug ...]` reads the generated card
manifest and returns selected targets without writing images or site state.
The canvas is 1200×630. Changelog cards retain manifest order and target
`site/static/changelog/og/<slug>.png`; the standalone Extensions card follows them
and targets `site/static/extensions/og.png`. Its copy has no changing catalog count.

No requested slugs selects everything and records full changelog-directory
ownership. A targeted request filters the existing target order, deduplicates
requested names, ignores unmatched names when another requested card matches,
and fails with known slugs when none match. These rules follow the pinned MIT
`website/scripts/generate-og.ts`, with Workdeck branding and paths. Unsafe slugs
are rejected before they can become staging or publication paths.

The plan JSON explicitly reports `rendered: false`. Staged image publication, stale-image removal,
frozen browser oracles and visual verification remain incomplete. This planner
does not authorize deleting an existing image directory. The complete source
interval remains unmapped; two model tests and a read-only CLI test cover only
this implemented selection boundary.

Source copyright Modem Labs Inc., MIT; see `THIRD_PARTY_NOTICES`.

## HTML generation

`cargo xtask social-cards-html <cards.json> <font.woff2> [slug ...]` returns a
JSON mapping of intended PNG paths to HTML documents on stdout; it writes no PNG
or site file. It embeds the supplied font bytes as a WOFF2 data URI and uses the
complete extracted source CSS with the 1200×630 canvas, palette, spacing and
typography. Title size changes at more than 12 UTF-16 code units, preserving the
source's handling of non-BMP characters. Optional tagline, chips and Latest pill
follow the source conditions; interpolated text escapes ampersands, angle brackets
and double quotes. Workdeck branding replaces the upstream wordmark and URLs.

Three model/HTML tests cover selection, path validation, escaping, conditional
markup and the title threshold. The CLI test uses synthetic font bytes to verify
embedding and read-only output, not valid font decoding or rendered geometry.
No browser screenshot, actual-font validation or visual parity is claimed yet.

## Native capture to staging

`cargo xtask social-cards-capture <cards.json> <font.woff2> <webdriver> <chromium> [slug ...]`
uses explicit external browser/driver paths and the existing Rust WebDriver
renderer. It selects cards before launching the browser, captures a 1200×630
viewport after font readiness and two paint frames, decodes every PNG and checks
its dimensions. Temporary HTML files are removed after successful capture. A
failed capture or invalid image drops the entire temporary staging directory.

Only after every capture and browser close succeeds does the command retain the
staging directory and print its path plus indexed PNG-to-target mappings, with
`rendered: true` and `published: false`. The caller owns that retained directory.
It does not replace existing images or remove orphaned public images. There is
no claim that staging alone verifies visual parity or font fidelity.

The injected-renderer test verifies successful PNG staging, wrong-width rejection
and cleanup of a partially completed batch. `cargo check -p xtask` passed with a
must-use warning that was subsequently fixed. Neither `chromedriver` nor
`chromium` was found on the host PATH during this checkpoint; no native browser
capture was executed. These missing binaries do not block further port work.
Strict xtask all-target Clippy subsequently passed. The CLI regression verifies
that an unavailable explicit driver fails without a success report or site
directory creation, while preserving the card and font inputs.

The shared browser renderer now requires the font-and-paint callback to return
the boolean `true` before requesting a screenshot. Previously, a rejected font
promise returned an error string that was ignored. Missing, null, false and text
responses now fail capture rather than producing a success report. All 15
compositor tests pass, including explicit readiness-response validation. This
checks protocol handling, not a live browser font-failure scenario.

Successful capture now saves the same report as `capture.json` beside the staged
PNGs before retaining the directory or printing success. The manifest uses
exclusive creation, a trailing newline and a file sync; an existing manifest is
never overwritten. If saving fails, the still-owned temporary directory is
cleaned up instead of reporting a retained capture. Four social-card unit tests
pass, including exact saved bytes and collision preservation. File sync alone
does not establish directory-entry crash durability, and the manifest is not
signed provenance or authorization to publish its listed paths.

Capture reports now identify schema 1 and record each staged image's exact byte
length and SHA-256 alongside its indexed filename and target. Report generation
rejects missing or nonregular staged entries before retaining the directory.
The test uses synthetic bytes to verify ordered destination mapping and that
changed content produces a changed digest; PNG decoding is tested separately
at capture time. All five social-card unit tests pass. These hashes support
future publication validation but do not authenticate the manifest or prevent
an actor from modifying both the manifest and image.

## Saved capture validation

`cargo xtask social-cards-check <staging-directory> <cards.json> [slug ...]`
reselects targets from the current card input, rehashes the indexed staged files
and compares the complete regenerated report with `capture.json`. Changed image
bytes, card metadata, destination mappings, full/targeted scope or staging root
fail validation. Reports record the canonical staging path to handle filesystem
aliases consistently. Final-component symlinks for the directory, manifest and
images are rejected. Success returns JSON with `valid: true, published: false`.

The six unit tests include unchanged acceptance and image/target/scope drift
rejection. This read-only check is not a reservation against later changes, does
not authenticate a jointly modified manifest/image pair or prove visual fidelity.
Publication must validate its actual inputs at
write time; that integration remains incomplete.

Both social-card CLI integration tests pass. The saved-capture case derives its
target through the actual planner, builds a manifest over synthetic bytes,
accepts the matching capture and rejects a changed image. It verifies unchanged
manifest/card input bytes, preservation of the changed image, empty failure
stdout, and no site or Workdeck state creation. Synthetic bytes deliberately
exercise manifest/hash validation only, not PNG validity or browser capture.

The checker now additionally uses the capture renderer's shared PNG decoder to
require decodable 1200×630 pixels after manifest/hash comparison. Its unit and
CLI acceptance fixtures have been upgraded from the earlier synthetic bytes to
real encoded PNGs. A matching digest for malformed image bytes is rejected by
the decoder. Six unit tests and both CLI tests pass; this proves structural image
validation, not visual parity with either upstream baseline.

## Direct pinned HTML oracle

The explicitly ignored `html_matches_both_pinned_source_renderers` test passed
against main `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`. It reads the original renderer
through `git show`, executes it with Bun only in a disposable temporary
directory, and compares the complete HTML string with the Rust renderer.
The expanded 108 comparisons pass for ordinary titles and both sides of the
12-UTF-16-unit title-size boundary, crossed with absent, empty and populated
taglines and chip lists, and both latest-badge states. Metadata and footer
escaping are also compared directly against the pinned source.
Only the visible product mark and the Rust CSS attribution comment are
normalized; whitespace, CSS and geometry are compared exactly.

Run this optional source-oracle check with:

```sh
cargo test -p xtask html_matches_both_pinned_source_renderers -- --ignored
```

This is a live source-function comparison, not a browser screenshot comparison
or proof of complete generator parity. Bun is
not used by the Rust renderer or shipped product. The source interval remains
unmapped while publication and remaining generator behavior are incomplete.

The 108 raw upstream HTML results and their explicit inputs are now frozen in
`port/hunk/fixtures/social-card-html.json`. Each case carries its source commit;
the original Hunk mark is retained in the fixture and normalized only at replay.
These MIT-derived HTML/CSS outputs retain Modem Labs attribution through this
document and `THIRD_PARTY_NOTICES`. No TypeScript implementation is retained.
The ordinary `frozen_html_oracles_match_rust_without_upstream_runtime` test
replays them using Rust alone. To deliberately regenerate from the two pinned
sources, run the ignored oracle with `WORKDECK_CAPTURE_SOCIAL_HTML_ORACLE=1`.
Capture is written only after every live comparison passes. The frozen corpus
covers HTML rendering only, not browser cells, font fidelity or publication.

## Publication planning

`cargo xtask social-cards-publication-plan <staging-directory> <cards.json> [slug ...]`
validates the saved capture and produces exact original/replacement byte maps.
A full run includes stale files under `site/static/changelog/og` as deletions;
a targeted run only considers selected destinations. Unchanged bytes are omitted.
Standalone page images are never swept. Planning does not modify site files.
Existing symlinks/nonregular entries and duplicate destination writes are
rejected. The full-set inventory includes nested files, but empty-directory
removal, application/recovery, and protection against changes after planning
remain unfinished. Eight social-card unit tests pass, including the full versus
targeted stale-image regression; this does not establish publication parity.

## Local publication application

`cargo xtask social-cards-publish <plan.json> <new-external-backup> <staging-directory> <cards.json> [slug ...]`
holds the shared repository release lock, regenerates the publication plan and
requires exact equality with the supplied plan. Changed files are written with
sibling temporary files; absent destinations use exclusive persistence.
Original bytes and permissions are saved before the first write in a new backup
directory outside the repository. A failure rolls back already written files
only if they still contain the operation's replacement, preserving detected
concurrent edits and reporting recovery conflicts. Staging captures are retained.
An unchanged plan succeeds without creating a backup. This command changes local
site files only; it does not upload or release them.

Nine social-card unit tests pass, including binary replacement, deletion and
creation, successful application, and injected failure after each of the three
writes with exact original-byte recovery. Filesystem races, directory-entry
crash durability, empty-directory
cleanup and changes to the full-set inventory during application remain open;
the implementation is not a crash-atomic publication transaction or full parity.

All three social-card CLI integration tests now pass. The publication case
generates a real encoded PNG and capture manifest in temporary directories,
obtains the plan through the actual CLI, publishes exact image bytes, checks the
recovery plan, and verifies unrelated changelog images remain untouched during a
targeted run. Reusing the stale plan fails before creating another backup;
regenerating an unchanged plan succeeds with zero edits and no second backup.
The original capture and card input remain intact and no `.agents` state is
created. This is local filesystem/CLI evidence, not live browser evidence.

Capture validation now returns the exact in-memory PNG bytes used to regenerate
the manifest hashes and decode pixels. Publication planning consumes those bytes
instead of reopening staged paths after validation. A changed or missing file
between validation and a second read can therefore no longer become an unchecked
replacement or accidental deletion. This does not remove races in initial path
resolution or concurrent edits to publication destinations.

## Broader verification checkpoint

At `cc1f68ee`, `cargo test -p xtask --all-targets` passed: 354 unit tests,
37 integration/PTY tests, and six explicitly ignored unit tests. This includes
the shared media compositor and release tooling, not just social-card tests.
The strict `cargo xtask port audit` still exits 1 with 1,257 baseline files,
1,459 ledger intervals, 280 unmapped records and 92 cached upstream delta
commits. No upstream fetch was performed for this checkpoint. Neither result
establishes full workspace verification, native-platform parity or release
readiness; no ledger dispositions were changed.

The three CLI tests also pass with cross-process lock contention and full-run
cleanup assertions. Holding `.git/workdeck-release.lock` in the test process
prevents the publication child from creating either the backup or destination.
After releasing the lock, targeted publication succeeds. A subsequent full run
removes the stale changelog image, retains the unchanged standalone card and
records the removed image's exact original bytes in recovery data. Empty
directory removal and inventory changes during application are still open.

Full publication plans now also record directories that no selected card needs.
Application removes these deepest-first using nonrecursive `remove_dir`, so an
unexpected new entry causes failure instead of being swept. Recovery records
their permissions, and rollback recreates successfully removed directories
parent-first before restoring files. Empty-directory-only plans count as changes.
Nine unit tests pass with failure injection after all five operations (three
file writes/deletions and two nested directory removals). This supersedes the
earlier empty-directory limitation; concurrent filesystem races and crash-atomic
directory replacement remain unresolved.

Two focused publication-application tests pass with additional adversarial
coverage: inserting an unexpected image immediately before directory removal
fails cleanup, preserves that image and restores the earlier deleted image.
The five-step rollback matrix additionally verifies restoration of Unix 0750
and 0700 permissions on nested directories. This covers the injected event
sequence, not arbitrary concurrent filesystem interleavings.

Full plans now retain the entire original changelog-file inventory, including
unchanged images. Application compares it before creating recovery data and
compares the expected final inventory after writes. A new file in a retained
directory is detected rather than silently accepted as a complete full run.
The regression covers additions before application (no backup or writes) and
during application (rollback with the new file preserved). Eleven social-card
unit tests pass. These checks do not provide isolation against changes after the
final check, ABA edits, or parent replacement between filesystem operations.

## Live-capture prerequisite inspection

The host has `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`;
its `--version` reports `Google Chrome 152.0.7977.83`. No `chromedriver` or
JetBrains Mono WOFF2 was found in the checked project, `/Applications`,
Homebrew executable directory, Selenium cache, or user cache locations.
This is a scoped filesystem inspection, not proof that neither exists elsewhere.
The pinned website declares `@fontsource-variable/jetbrains-mono` with range
`^5.2.8`; the exact resolved asset still needs acquisition and licensing evidence.
Before a real capture, provide a compatible declared ChromeDriver binary and
the verified font asset to `social-cards-capture`. No live browser capture or
font-fidelity result is claimed by this checkpoint. Existing Chrome was queried
for its version only; no user browser profile was opened or modified.

## Native browser smoke capture

A subsequent live capture succeeded using Google Chrome 152.0.7977.83 and
ChromeDriver 152.0.7977.82 (mac-arm64), selected from Google's Chrome for Testing
build metadata. Driver ZIP source:
`https://storage.googleapis.com/chrome-for-testing-public/152.0.7977.82/mac-arm64/chromedriver-mac-arm64.zip`;
SHA-256 `f5d378ce382494416bb3243491ef30a5d22b12cfd2ab7cb5fdf6c448bff8abbb`.
The driver and its bundled notices remain in temporary storage, not the repository.

The font came from `@fontsource-variable/jetbrains-mono` 5.3.0, the exact pinned
website lockfile resolution. The downloaded archive's SHA-512 matched
`F32xpS2NsGYoQi2ADSkKTgpJj7ozajsGgDJ8woTnqjmIB+dxDIqImjl4pXZVEExu8UFZ2ndhmX18EBS/hdz3Lw==`.
Only its license, metadata and `jetbrains-mono-latin-wght-normal.woff2` were
extracted; no package manager or JavaScript runtime was used. Font SHA-256:
`18be452724bfdc236c074ca94a249a7f41a86752c7d04ab258ce9ed5651f6a7e`.
The asset is SIL OFL 1.1, Copyright 2020 The JetBrains Mono Project Authors.
The font itself is not committed by this checkpoint.

With an empty card manifest and the `extensions` selector, native capture
produced the visually inspected `port/hunk/fixtures/social-card-extensions-native.png`:
1200×630, 39,455 bytes, SHA-256
`5c619830bf299866b8df747d7fef10c4b90640a8aaae9375cd8aa4ea8c1fdfaa`.
`social-cards-check` accepted the saved capture. This image derives from Hunk's
MIT card layout (Copyright Modem Labs Inc.) with Workdeck branding. No site file
was published. This proves one native browser smoke path, not dual-baseline pixel
parity, cross-platform rendering, or complete generator behavior.

## Dual-baseline HTML pixel comparison

The explicitly ignored `browser_pixels_match_all_frozen_baseline_html` test now
passes for all 108 frozen HTML cases: 54 from each pinned baseline. It renders
each upstream HTML result and its Rust counterpart through the same native
WebDriver session, then compares decoded dimensions, pixel format and every
pixel byte. All 216 screenshots matched pairwise on the host described above
(29.03 seconds for the test). The only upstream HTML substitutions are the
permitted product mark and the real verified font data URI replacing the
synthetic font input used during HTML-oracle capture. No geometry, whitespace,
text or pixel differences are normalized away.

Reproduction requires explicit tool/asset paths:

```sh
WORKDECK_ORACLE_DRIVER=/absolute/path/to/chromedriver \
WORKDECK_ORACLE_BROWSER=/absolute/path/to/chrome \
WORKDECK_ORACLE_FONT=/absolute/path/to/jetbrains-mono-latin-wght-normal.woff2 \
cargo test -p xtask browser_pixels_match_all_frozen_baseline_html --bin xtask -- --ignored
```

Temporary screenshots are cleaned up by the test. It needs no Bun at replay
time. This verifies rendered equivalence for the frozen HTML corpus using one
browser transport, not the original Playwright lifecycle, cross-platform font
rasterization, every possible card input, or complete generator parity. The
source ledger interval remains unmapped.

## Connected native generation command

`cargo xtask social-cards-generate <cards.json> <font.woff2> <webdriver> <chromium> <new-external-backup> [slug ...]`
now connects target selection, HTML generation, browser capture, capture
validation and recoverable local publication. It holds the shared release lock,
renders the complete selection before publication begins, and cleans its owned
scratch directory on success or failure. The backup is required for changed
outputs and must be new and outside the repository. No package manager or
JavaScript runtime participates. Standard output lists each selected PNG and
the rendered count and Workdeck destination paths, following the upstream text
shape; progress timing is not claimed identical.

The ignored CLI test
`native_generator_captures_then_publishes_without_partial_capture_writes` passed
using the real browser/driver/font documented above. It verifies a failed launch
preserves stale site images and creates no backup or destination, then verifies
a successful full run, exact summary stdout, PNG geometry, stale-directory
removal, original-byte recovery data and absence of `.agents` state. Invoke it
with the same `WORKDECK_ORACLE_*` environment variables as the pixel test.
No actual repository site outputs were modified by the isolated test. Explicit
tool/asset paths and recovery arguments intentionally adapt the maintainer
workflow; full upstream-script lifecycle and repository-wide parity remain open.

## Retained font asset

The verified, unmodified WOFF2 now lives at
`site/static/fonts/jetbrains-mono-latin-wght-normal.woff2`, alongside the complete
supplied `jetbrains-mono-LICENSE.txt`. `THIRD_PARTY_NOTICES` identifies its
non-MIT SIL OFL 1.1 licensing. The Rust asset test checks exact font and license
SHA-256 values and the notice reference. Use this checkout path as the font
argument to native generation; no temporary package archive is needed thereafter.
This adds a third-party asset, not a TypeScript source mirror or runtime
dependency. Website typography integration and distribution-wide asset SBOM
coverage remain separate work.

`site/data/third-party-assets.json` now records the retained font's resolved
version, archive URL/integrity, OFL identifier and exact font/license hashes.
`cargo xtask site-assets-sbom` validates those files and prints a website-only
CycloneDX 1.5 file inventory without writing outputs. Missing, changed, duplicate
or symlinked asset paths fail validation. The binary Cargo-dependency SBOM is
intentionally unchanged: this font is not embedded in the shipped executable.
This is an inventory for the retained font assets, not a complete website/source
SBOM or proof of integration with every release archive.

The native site's main stylesheet now loads the retained Latin variable font
directly and uses Hunk's font fallback stack under Workdeck ownership. Code,
keyboard and preformatted elements inherit it. The face retains normal style,
100–800 weights and `swap` display from the verified Fontsource CSS, without a
package import. Three site-asset tests pass. This is Latin typography wiring,
not full `brand.css` parity: other script subsets, palette, shared header/footer
geometry and responsive behavior remain unmapped. Zola was not found on PATH
during this checkpoint, so no complete site build or browser-page validation
is claimed.

The Latin-only site declaration has now been replaced by all six normal variable
subsets from the same verified Fontsource archive: Cyrillic extended, Cyrillic,
Greek, Vietnamese, Latin extended and Latin. `fonts/jetbrains-mono.css` preserves
the source `index.css` declarations and Unicode ranges, changing only `./files/`
URLs to the colocated assets. The base template loads this stylesheet directly;
there is still no package import or JavaScript runtime. The website asset
inventory now covers six fonts, the supplied license and the relocated CSS.
This supersedes the earlier missing-subset limitation but does not establish
full website layout, browser-page or cross-platform typography parity.

Both `cargo xtask site build` and `cargo xtask site check` now verify the retained
asset inventory before invoking Zola. The CLI regression supplies a modified
font and confirms both commands fail with the hash error, preserve the input and
create neither site output nor Workdeck state. This makes asset verification a
build prerequisite; it does not replace the still-required full site build and
browser validation.

## Zola build checkpoint

`cargo xtask site check` now passes locally with Zola 0.23.4. The temporary
macOS arm64 tool was downloaded from the official getzola/zola release and its
archive SHA-256 matched GitHub's release digest:
`303b8e1f3251a6250e47f811eda143316f653c22201faa66777d48ac499c0ee3`.
The check initially exposed an overly strict repository-link assertion: current
Tera emits a literal slash where the older check expected `&#x2F;`. Validation
now counts both exact encodings together, retaining duplicate-link rejection.
The regression covers each encoding, duplicates, missing links and longer URLs.
CI's Zola installation is pinned to the tested 0.23.4; no remote CI run is claimed.
This proves the current small native site builds and passes its implemented
checks, not that all upstream pages, accessibility or visual parity are complete.

## Shared brand stylesheet migration

`site/static/brand.css` now retains the complete pinned-main stylesheet, with
only the package font import removed (the local six-subset stylesheet supplies
it), `--hunk-` tokens renamed to `--workdeck-`, `brand-modem` renamed to
`brand-attribution`, and an MIT attribution header added. A Rust test verifies
the entire resulting text against `git show` and those explicit transformations.
The native template loads it and uses its shared header/footer classes; existing
page styles now consume its colors and font tokens instead of unrelated colors.
`cargo xtask site check` passes with this integration.

An attempted comparison against stable exposed a genuine version difference:
stable lacks main's star-control styling and responsive star-label/count rules.
The migration retains main's complete rules as required, rather than removing
them to match an older tree. This text check is against main, not a claim that the
two source stylesheets are identical. Complete header controls, theme switching,
all responsive page layouts and screenshot parity remain unfinished; the brand
source interval is still unmapped.

The native shell now includes the header's Install, Extensions and GitHub-star
controls, retaining the source star SVG and responsive class names. Install
points to the real homepage install anchor; Extensions carries `aria-current`
on its page; repository URLs come from Workdeck configuration. Footer GitHub and
MIT links use the same configured repository. No upstream Discord ownership,
star count, npm distribution or unbuilt Docs/Changelog route is invented.
Those remaining header/footer capabilities are not waived and their source
records remain unmapped. Site validation checks the built header controls,
active-page attribute and home install target; broader interaction and viewport
testing remain necessary.

`site serve` now shares the build/check asset gate. The CLI regression passes
for all three commands with a changed font, before any site server or build
starts. Six site-asset unit tests pass, including duplicate inventory paths and
a symlinked static-directory parent; the latter preserves the external file.
These path checks are not a sandbox against concurrent parent replacement.

## Initial native documentation routes

The site now builds `/docs/`, `/docs/start/` and `/docs/start/install/` through a
native Zola documentation template. The install page documents source builds,
the current native installer/update interface and remaining release gates. It
does not relabel Hunk npm/Homebrew/mise/Discord availability as Workdeck's.
Docs navigation marks these routes current, and site checking verifies the
rendered install page and active navigation. The Getting Started section keeps
the install page attached to the documentation hierarchy rather than orphaned.
This is an initial adaptation, not completion of the 5,438-byte upstream install
page or the documentation corpus. That source record remains unmapped.

The initial `/docs/start/quick-start/` route now covers working-tree review,
untracked exclusion, stream navigation, layouts, commit/path filtering, watch
mode and the native skill entrypoint. It distinguishes headless `changes diff`
from the reviewer and flags the missing native screenshot and full agent guide.
The targeted `workdeck-tui` hunk-navigation test passes using Ratatui's test
backend (despite its historical `pty_real_` name, this is not a fresh PTY run).
`cargo xtask site check` passes with three pages, two sections and zero orphans.
The upstream quick-start's complete content and behavioral evidence are not yet
accounted for; its source interval remains unmapped.
