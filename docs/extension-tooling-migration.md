# Native extension tooling migration

Workdeck replaces Hunk's JavaScript extension checks with a native Rust contract. The pinned
helpers are inspected from the protected Hunk refs by `cargo xtask verify`; they are never copied
into the tree and no JavaScript runtime is invoked.

The migration covers the consumer declaration check, all documentation examples, the translated
example test, and the package-surface check:

| Hunk helper | Native contract |
| --- | --- |
| `scripts/extension-consumer-check.ts` | `xtask/src/extension_catalog.rs` verifies the native API and extension host surface. |
| `scripts/extension-doc-examples.ts` | `xtask/src/extension_catalog.rs` checks the static guide and compiled native examples. |
| `scripts/extension-doc-examples.test.ts` | `native_extension_examples_and_docs_are_checked_by_rust` is an executable parity test. |
| `scripts/check-pack.ts` | native release-artifact validation and the extension catalog verifier enforce the one-binary package boundary. |

Both baseline `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2` and stable
`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd` source blobs are checked for byte count, line count,
and SHA-256. The Rust extension API is JSON-RPC v2 over newline-delimited stdio, with declarative
views owned by Ratatui and trusted native subprocesses for user extensions.
