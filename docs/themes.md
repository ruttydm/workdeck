# Themes

Choose a built-in theme, let Workdeck select one from your terminal background,
or define custom themes in `~/.config/workdeck/config.toml` or
`.agents/workdeck/config.toml`. Workdeck keeps the selected theme in its existing
`[ui]` table:

```toml
[ui]
theme = "github-dark-default"
```

While reviewing, press `t` or choose `View -> Themes…`.

## Automatic theme selection

Set `[ui] theme = "auto"` or pass `workdeck diff --theme auto` to query the
terminal background at startup. Light backgrounds select `github-light-default`;
dark backgrounds select `github-dark-default`. If the terminal does not answer,
the fallback is `github-dark-default`.

Older identifiers, including `graphite` and `paper`, remain compatibility aliases.

## Custom themes

Inherit from a built-in theme and override only the colors you need:

```toml
[ui]
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
"entity.name.function" = "#8ed4ff"
```

Use a separate `[themes.<id>]` table for each additional theme, with the same
keys. Select its table identifier through `[ui] theme = "<id>"` or
`workdeck diff --theme <id>`:

```toml
[ui]
theme = "ocean"

[themes.ocean]
base = "nord"
label = "Ocean"
accent = "#7fd1ff"

[themes.ocean.syntax_scopes]
"keyword.operator" = "#7fd1ff"

[themes.paper-review]
base = "github-light-default"
accent = "#0969da"
```

Identifiers must be lowercase words separated by `-` or `_` and cannot reuse a
built-in theme identifier. Invalid identifiers are skipped with a startup notice,
not a failed review session. `[custom_theme]` defines the identifier `custom` and
takes precedence over `[themes.custom]`.

Custom themes follow built-in themes in the selector, in declaration order.
Repository configuration overrides user configuration table by table for a shared
identifier; omitted values continue to inherit from the user definition.

## Syntax scopes

`syntax_scopes` accepts TextMate scope selectors directly. Quote selectors
containing dots. Preserve declaration order: the compatibility contract is that
later equally specific matching rules win, but a more-specific base-theme rule
beats a broader override. If that happens, add the grammar-specific selector.
Every custom theme color must be a `#rrggbb` hexadecimal value.

The pinned upstream guide describes these as Shiki theme rules with no
application-specific translation layer. Workdeck consumes the selectors through
its native highlighting implementation, without executing Shiki or JavaScript.
The configuration contract and ordered overrides are tested; this migrated guide
does not certify complete tokenization or selector-precedence parity for every
grammar. That remains part of the strict source and terminal-oracle release gates.

## Migrating the legacy syntax table

The deprecated `[custom_theme.syntax]` role table is temporarily translated to
approximate scopes. It can coexist with `syntax_scopes` during migration. An
exact `syntax_scopes` entry overrides a translated entry with the same selector.

Semantic roles have no one-to-one TextMate mapping, so migrate when practical.
For example, replace legacy `comment = "#ffffff"` with both
`"comment" = "#ffffff"` and
`"punctuation.definition.comment" = "#ffffff"` under
`[custom_theme.syntax_scopes]`. Add language-specific selectors where a grammar
uses more-specific scopes.

The pinned upstream policy scheduled removal of the compatibility table in its
next major release. Workdeck retains it for this compatibility port; any removal
requires an explicit future major-version migration policy.

For importing existing configuration, see [Migration](MIGRATION.md). Imported
configuration uses Workdeck's paths; this guide does not enable ongoing reads
from legacy configuration directories.

Adapted from Hunk's MIT-licensed `docs/themes.md` at
`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. See
[third-party notices](../THIRD_PARTY_NOTICES).
