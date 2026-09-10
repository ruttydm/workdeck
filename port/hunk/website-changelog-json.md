# Generated JSON formatter continuation

The native formatter in `xtask/src/changelog/website/json.rs` translates Hunk's
100-column JSON artifact layout. Primitive arrays remain inline only if the key,
indent, UTF-16 content width and any following comma fit; arrays containing
containers expand. Empty containers stay compact. Object order follows canonical
JavaScript array-index keys before remaining insertion-ordered keys, and number
text uses the existing JavaScript-compatible Ryu implementation.

The latest-release command now uses this formatter. No new executable or runtime
is introduced. Frozen fixtures execute `formatJson` from both pinned generators:
eight cases per pin cover short/long arrays, key width, final-key comma budgeting,
nested objects, empty containers, numeric keys/numbers and astral character width.

This implementation accepts Rust JSON values; JavaScript-only `undefined` values
must be removed by typed artifact construction, not represented by a fake JSON
sentinel. The original undefined-member source test is not claimed translated.
Arbitrary caller-supplied indent/column parameters and the complete source test
block remain unmapped. Complete artifact orchestration is also still open.
MIT Modem Labs Inc. attribution is retained in the Rust module and notices.

Verification: all 16 frozen formatter outputs match, all 16 changelog CLI tests
pass after integration, and strict xtask Clippy, workspace formatting and diff
whitespace checks pass. No ledger interval is mapped by this partial port.
