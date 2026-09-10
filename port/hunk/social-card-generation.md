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

The plan JSON explicitly reports `rendered: false`. Chromium/WebDriver capture,
staged image publication, stale-image removal,
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
