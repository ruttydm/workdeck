# Bundled TextMate theme notices

Workdeck embeds 67 JSON TextMate themes for native syntax rendering. They are data assets and do
not introduce a JavaScript runtime.

- The 65 Shiki catalog themes are byte-retained from `tm-themes` 1.12.0. The Rust vendoring task
  proves every parsed JSON value is identical to the corresponding `@shikijs/themes` 3.23.0
  module used by Hunk's pinned dependency graph.
- `pierre-light` and `pierre-dark` are byte-retained from `@pierre/theme` 2.0.0, the defaults used
  by `@pierre/diffs` 1.3.5.
- `tm-themes-NOTICE` contains the original source and license attribution for every Shiki catalog
  theme. The adjacent license and Pierre notice files are retained from their published archives.

Run the authenticated, runtime-free generator with the three pinned package archives:

```text
cargo xtask themes vendor \
  --shiki-archive themes-3.23.0.tgz \
  --tm-themes-archive tm-themes-1.12.0.tgz \
  --pierre-archive theme-2.0.0.tgz
```

The task verifies fixed SHA-256 digests before reading an archive, compares the 65 Shiki and
`tm-themes` payloads, writes the Rust-embedded assets, and records individual file hashes in
`crates/workdeck-diff/assets/themes/manifest.json`.
