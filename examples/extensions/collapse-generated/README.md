# Collapse generated files

Native Rust translation of the complete filtering example in Hunk's
`website/src/content/docs/docs/extend/extensions.md` at `2c00f435` (MIT; see the
repository's `THIRD_PARTY_NOTICES`).

Build from the workspace root:

```sh
cargo build -p workdeck-examples --bin workdeck-example-collapse-generated-extension
```

For local use, copy the built executable beside `workdeck-extension.toml` in a
separate extension directory, then pass that directory to
`workdeck diff --extension /path/to/collapse-generated`. On Windows, update the
manifest executable filename to include `.exe`. An explicitly supplied extension
runs with your user permissions; inspect it before loading it.

Defaults hide `*.lock`, `*-lock.json`, and `dist/*`. Override them in Workdeck's
user or repository configuration:

```toml
[extension.collapse-generated]
patterns = ["*.lock", "bun.lockb", "generated/*"]
```

Only `*` is special, including across directory separators; other regex/glob
characters are literals. Matching is anchored and case-sensitive. Wildcards do
not consume ECMAScript line terminators. An empty list hides nothing. Invalid
configuration is rejected during handshake. Filtering preserves remaining files,
their order, and all other changeset fields. A nonzero count emits one info
notification with singular/plural wording.

The native transform response also supports an optional `notifications` array.
Host-generated IDs prevent extensions from supplying notification identities.
Valid notifications are delivered before an invalid changeset warning, matching
the side-effect ordering of the upstream example. This example instead sends
independent `workdeck/notify` frames before the transform response, using the
host's existing notification transport. Response-only notifications cannot
represent side effects preceding a process crash or JSON-RPC error.

Validation: `cargo test -p workdeck-examples --lib collapse_generated` exercises
configuration, wildcard semantics, metadata preservation, count wording, and
newline-framed handshake/transform/error responses. Host transform tests cover
legacy responses, notification order, malformed payloads and invalid changesets.
`cargo test -p workdeck-examples --test collapse_generated` launches the compiled
binary through `LoadedExtension` and verifies retained-file identity and exactly
one delivered info notification. On the local macOS host, all four example unit
tests, that subprocess integration test, and all 210 extension-host unit tests
passed. Clippy with `--all-targets -- -D warnings` passed for the extension API,
host, and examples packages. These results do not establish native Windows/Linux
execution or whole-workspace release qualification.
These are native tests, not frozen dual-baseline oracle comparisons. The parent
upstream documentation file remains unmapped until all its content and behavior
have genuine migration evidence.
