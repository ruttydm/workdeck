# Optimized viewport-painting profile

- Production code: `d2de3d88c6ef5bd9555a8357528c755e8a894e0e`.
- Host: macOS arm64; optimized `xtask benchmark interaction-diagnostic`.
- Native `sample`, requested one second at one-millisecond intervals, process 49430.
- An earlier attachment missed already-exited process 49349; a new process was sampled only after that exit was verified.
- Local raw sample: `/tmp/workdeck-interaction-49430.sample.txt`. This partial, perturbed process is not included in benchmark timing evidence.

Selected collapsed top-of-stack counts:

| Symbol | Observations |
| --- | ---: |
| `_xzm_free` | 58 |
| `_platform_memmove` | 49 |
| `forward_search` | 45 |
| `review_content_digest` | 43 |
| `sha256::compress256` | 40 |
| `ParseState::parse_line` | 40 |
| `highlighted_content_fingerprint` | 35 |
| `match_at` | 31 |
| `_xzm_xzone_malloc` | 30 |
| `normalized_review_source_lines` | 20 |

These counts are sampled observations, not wall-time percentages or exclusive stage costs.
Highlight identity and syntax work remain visible after viewport painting. Gap planning also
normalizes both complete source snapshots into owned line strings even when only hunk positions
and line counts are used. The next change separates count-only gap geometry from expanded source
text without weakening content identity. No performance gate or ledger record is satisfied by
this diagnostic profile.
