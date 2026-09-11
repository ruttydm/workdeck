# Historical document migrations

Historical Markdown that is part of the pinned Hunk tree is translated in
place, not retained as a runtime source mirror. The verifier
`xtask::historical_docs` reads the exact baseline blob through `git show`, checks
the SHA-256/byte count and every heading, and requires the native destination
and this migration entry.

| Pinned file | Native destination | Verification |
| --- | --- | --- |
| `docs/extension-api-evaluation.md` | `docs/extension-api-evaluation.md` | `cargo xtask verify` / `historical_docs::verify` |

The translated evaluation preserves the original findings while recording the
native Rust/Ratatui resolutions and their executable extension tests. Hunk's
MIT copyright remains attributed in the destination document and
`THIRD_PARTY_NOTICES`.
