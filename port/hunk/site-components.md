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
