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
not authenticate a jointly modified manifest/image pair and does not decode PNGs
again or prove visual fidelity. Publication must validate its actual inputs at
write time; that integration remains incomplete.

Both social-card CLI integration tests pass. The saved-capture case derives its
target through the actual planner, builds a manifest over synthetic bytes,
accepts the matching capture and rejects a changed image. It verifies unchanged
manifest/card input bytes, preservation of the changed image, empty failure
stdout, and no site or Workdeck state creation. Synthetic bytes deliberately
exercise manifest/hash validation only, not PNG validity or browser capture.
