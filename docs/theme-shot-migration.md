# Theme picker migration

Hunk's `ThemeShot.astro` presents six review screenshots from the bundled
theme catalog. Its client component uses `aria-pressed` buttons, `aria-controls`,
pointer/focus warming, idle decoding, and hidden image swaps; the source also
links to the remaining bundled themes.

Workdeck keeps all six byte-identical, MIT-licensed screenshots in the static
asset inventory and renders the same six labels, accessible group, image
dimensions, and `and 61 more` link. The interaction is a CSS radio control:
keyboard focus, label activation, and browser history-free selection work with
no application JavaScript. Native `loading="lazy"` replaces the source's
`requestIdleCallback`/`decode()` warming while retaining the source's first-paint
and deferred-image intent.

`xtask::site_assets::verify_theme_shot_component` reads the complete pinned
component through Git, checks every theme and behavior marker, verifies the
native HTML/CSS controls, and rejects a script runtime in the landing page.
