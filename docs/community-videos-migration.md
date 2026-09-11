# Community video cards

Hunk's `CommunityVideos.astro` deliberately rendered paused YouTube-style
cards instead of embedding a third-party player. Workdeck keeps that behavior:
the two pinned thumbnails are retained under the Hunk MIT notice, each card
opens the original YouTube walkthrough in a new tab with `noopener`, and the
locally painted avatar, title, play glyph, duration, and caption remain
available in the static HTML.

The cards use native `loading="lazy"` images and contain no-player markup; no
application JavaScript or third-party player is loaded. `xtask::site_assets::verify_community_videos`
checks the exact pinned component, source thumbnail blobs, destination hashes,
asset inventory, and complete card/style surface.

The native community video cards use no application JavaScript.
