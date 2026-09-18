+++
title = "Themes"
description = "Choose native review themes, detect terminal background and override colors."
template = "docs.html"
+++

Press `t` in Workdeck or choose **View → Themes…** to preview and select a theme. Persist it in TOML:

```toml
theme = "github-dark-default"
```

Use `theme = "auto"` to query the terminal background at startup. Workdeck chooses `github-light-default` for light terminals, `github-dark-default` for dark terminals, and falls back to dark if the terminal does not answer.

## Create a custom theme

```toml
theme = "custom"

[custom_theme]
base = "catppuccin-mocha"
label = "My Theme"
accent = "#7fd1ff"
panel = "#10161d"
noteBorder = "#c49bff"

[custom_theme.syntax_scopes]
"comment" = "#6e85a7"
"punctuation.definition.comment" = "#6e85a7"
"keyword.operator" = "#7fd1ff"
```

A custom theme inherits from a built-in base, then overlays semantic Workdeck colors and exact TextMate scopes. Colors must be six-digit hex values.

Quote scope selectors containing dots. Later equal-specificity declarations win, but a more-specific base selector can beat a broad override, so add grammar-specific selectors when needed.

The old `[custom_theme.syntax]` role table is deprecated and temporarily translated. Prefer `syntax_scopes` for new themes.

## Name several themes

`[custom_theme]` is the single theme named `custom`. To keep more than one of your own themes around, declare `[themes.<id>]` tables instead and select one by id:

```toml
theme = "midnight-ink"

[themes.midnight-ink]
base = "catppuccin-mocha"
label = "Midnight Ink"
accent = "#7fd1ff"

[themes.paper-trail]
base = "github-light-default"
label = "Paper Trail"
```

Every `[themes.<id>]` table accepts exactly the keys `[custom_theme]` accepts, and each theme appears in the theme picker. Ids are lowercase words separated by `-` or `_`; ids already taken by a built-in theme, by `auto`, or by `[custom_theme]` are skipped with a startup notice. A repository config can refine a user theme with the same id field by field.

The full website configuration reference is still being migrated. Native theme
parsing and registration live in `crates/workdeck-cli/src/config.rs`; built-in
theme identities and colors live in `crates/workdeck-core/src/theme.rs`.

Syntax scopes are interpreted by the native highlighter; no Shiki JavaScript
runtime is shipped. Adapted from Hunk's pinned MIT guide, Copyright Modem Labs
Inc. Complete theme/tokenization parity is still a release gate, and this source
interval remains unmapped pending migration verification.

