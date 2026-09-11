# Historical document migrations

Historical Markdown that is part of the pinned Hunk tree is translated in
place, not retained as a runtime source mirror. The verifier
`xtask::historical_docs` reads the exact baseline blob through `git show`, checks
the SHA-256/byte count and every heading, and requires the native destination
and this migration entry.

| Pinned file | Native destination | Verification |
| --- | --- | --- |
| `docs/extension-api-evaluation.md` | `docs/extension-api-evaluation.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/watch-benchmark.md` | `docs/watch-benchmark.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/watch-benchmark-final.md` | `docs/watch-benchmark-final.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/module-boundaries.md` | `docs/module-boundaries.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/extension-system-exploration.md` | `docs/extension-system-exploration.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/browser-review-rebuild.md` | `docs/browser-review-rebuild.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/browser-review-seam-audit.md` | `docs/browser-review-seam-audit.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/session-broker-sdk.md` | `docs/session-broker-sdk.md` | `cargo xtask verify` / `historical_docs::verify` |
| `docs/changelog-on-hunk-dev.md` | `docs/changelog-on-hunk-dev.md` | `cargo xtask verify` / `historical_docs::verify` |

The translated evaluation preserves the original findings while recording the
native Rust/Ratatui resolutions and their executable extension tests. Hunk's
MIT copyright remains attributed in the destination document and
`THIRD_PARTY_NOTICES`.

The watch reports retain the complete historical campaign surface, including
its exact headings, tables, provenance, limitations, and disclosed failures.
The destination pages identify those values as upstream historical evidence;
native Workdeck measurements remain in the Rust benchmark reports and are never
silently substituted for the pinned campaign.
