# Native site component migrations

`website/src/components/docs/DocsFooter.astro` only delegates to Starlight's
footer component. The Rust/Zola site renders the Workdeck footer from
`site/templates/base.html`; documentation routes now select
`data-context="docs"`, while marketing routes keep `data-context="marketing"`.
`cargo xtask site check` builds the site and asserts the documentation footer
context in the generated installation page.

Source: Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, MIT, copyright Modem
Labs Inc.; see `THIRD_PARTY_NOTICES`.

The two identical Hunk favicon records are represented by the single generated
`site/static/favicon.svg` Workdeck mark. The Zola shell links and emits that
asset, and `cargo xtask site check` verifies both the link and the built file.

The pinned `website/public/robots.txt` is adapted at `site/static/robots.txt`:
all crawler allowances and Markdown-feed guidance remain, while the Workdeck
domain and sitemap replace Hunk's domain. The same site check verifies the
emitted policy and rejects a stale `hunk.dev` reference.

`website/src/components/docs/LightThemeProvider.astro` is represented by the
static `data-theme="light"` declaration on the native Zola `<html>` element and
the existing light-first CSS. The site check asserts the declaration and the
no-application-JavaScript boundary.
