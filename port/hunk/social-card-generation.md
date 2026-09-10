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

The JSON explicitly reports `rendered: false`. HTML rendering, font embedding,
Chromium/WebDriver capture, staged image publication, stale-image removal,
frozen browser oracles and visual verification remain incomplete. This planner
does not authorize deleting an existing image directory. The complete source
interval remains unmapped; two model tests and a read-only CLI test cover only
this implemented selection boundary.

Source copyright Modem Labs Inc., MIT; see `THIRD_PARTY_NOTICES`.
