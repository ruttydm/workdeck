# Feature showcase migration

Hunk's `FeatureShowcase.astro` contains six feature chapters (media-led), two
quotes, one extension-code sample, and responsive framed media. Workdeck keeps
the complete chapter/quote order, links, labels, dimensions, and all retained
feature assets in a static Zola template. The extension sample is translated
from the removed TypeScript API to the native Rust/JSON-RPC API.

The migration retains two quotes exactly in their chapter positions.

The pinned component autoplayed clips through an `IntersectionObserver` and
paused them for `prefers-reduced-motion`. Workdeck's no-application-JavaScript
replacement uses muted, inline, metadata-preloaded videos with visible native
controls: users can start/stop playback explicitly, reduced-motion users are
never surprised by autoplay, and the first frame remains available on narrow
terminals. The source clips and still are retained under the Hunk MIT notice,
inventoried in the website SBOM, and hash-checked against the baseline.

`xtask::site_assets::verify_feature_showcase` checks the complete source marker
surface, all media bytes, native HTML/CSS structure, inventory entries, and
the no-script boundary.

The static replacement deliberately uses no application JavaScript.
