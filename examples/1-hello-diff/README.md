# 1-hello-diff

A tiny first-run demo with one clean Rust diff.

## Run

```bash
workdeck difftool examples/1-hello-diff/before.rs examples/1-hello-diff/after.rs
```

## What to look for

- a renamed type and function parameter
- a small helper extraction
- obvious intra-line changes in strings and copy

The pair is compiled and exercised by `cargo test -p workdeck-examples`.
