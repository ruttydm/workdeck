# Terminal-width benchmark migration

The pinned Hunk `benchmarks/terminal-width.ts` workload is now an executable
native benchmark at `cargo xtask benchmark terminal-width`. The baseline and
stable-v0.20.1 blobs are identical: 3,267 bytes / 72 lines, SHA-256
`948a538a51c09018f85953ebe33184ba893ee5e3286b420610a50df55bd65fd8`.

The Rust benchmark retains the 2,000 measured iterations, 50 warmup
iterations, CJK scalar lines, emoji scalar lines, and combining/complex emoji
clusters. It emits deterministic `METRIC` measurements and checks that warmup
and measured checksums agree with the frozen dual-pin width oracle. The Rust
Cargo benchmark command is the native execution boundary.
cell-width implementation is the production path; the pinned
`string-width` comparison is retained only as oracle evidence. Workdeck ships
no JavaScript runtime or package dependency for this benchmark.

`xtask::benchmark::verify_terminal_width` reads both protected blobs through
`git show`, checks every workload marker and exact hash, and requires the
native CLI plus executable corpus test before the ledger interval is mapped.
