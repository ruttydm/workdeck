# 4-ui-polish

A screenshot-friendly Ratatui diff with copy edits, prop renames, and a small layout cleanup.

## Run

```bash
workdeck difftool examples/4-ui-polish/before.rs examples/4-ui-polish/after.rs
```

## What to look for

- renamed props and extracted button-label helper
- clear intra-line emphasis in strings and labels
- a compact UI-focused diff that works in split and stacked layouts

Both widgets render into Ratatui cell buffers and are exercised by `cargo test -p workdeck-examples`.
