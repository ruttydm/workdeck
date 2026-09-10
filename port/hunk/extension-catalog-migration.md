# Legacy extension catalog migration

The pinned `website/src/data/extensions.ts` file is a declarative directory of
16 community extensions. Workdeck retains those listings as
`site/data/legacy-extensions.json` with a required
`compatibility = "requires-rust-rewrite"` marker. The native extensions page
never offers installation or executes the historical TypeScript packages.

`xtask/src/extension_catalog.rs::verify_pinned_source` reads the exact
13,852-byte source blob through `git show`, decodes only its known literal
syntax (strings, fields, arrays, objects, punctuation, and integers), and
compares every listing, category, version, API version, and repository field
with the checked-in native catalog. Expressions and executable syntax are
rejected. `cargo xtask verify` and strict port auditing run this check.

This is a data-preserving native replacement: the source module is not copied
or run, and the public page explicitly tells users that existing TypeScript
extensions require a Rust rewrite. Hunk's MIT attribution is retained in
`THIRD_PARTY_NOTICES`.
