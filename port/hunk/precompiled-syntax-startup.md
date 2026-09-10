# Build-generated syntax set and cold startup

The workspace rerun after `aea27a8c` failed in the review-triage startup test's
unchanged 100 ms assertion. The failure reproduced in an isolated process.
Diagnostic timing measured about 744 ms overall: changeset and options were
ready in under 4 ms, app initialization reached highlighter construction in
2.4 ms, and the highlighter took roughly 735 ms. Cursor setup and event
publication accounted for only a few milliseconds. Temporary runtime timing
probes were removed after diagnosis.

`HighlightCache::default` synchronously assembled the bundled Syntect syntax set,
including the existing Elixir grammar. The process-wide cache helped subsequent
applications but did not eliminate cold-start assembly. The same assembly now
runs in `workdeck-diff/build.rs`, which serializes the set into Cargo's output
directory. Runtime initialization deserializes the embedded generated data and
retains the existing shared cache. This does not defer grammar assembly to the
first render, remove syntax definitions, or change highlight dispatch policy.

The build script watches the Elixir source asset; Cargo tracks the script and
build dependency. No generated binary is committed and no external runtime is
required. The runtime and build use the same Syntect dependency declaration.

`embedded_syntax_set_matches_runtime_assembly` compares the complete serialized
JSON value of the embedded set against the previous runtime construction. All
127 diff library tests passed in 0.91 seconds, including this equality test and
existing theme, grammar-state, source-highlight and worker tests. The isolated
review-triage startup test now passes its unchanged 100 ms limit.
Twenty consecutive fresh-process repetitions passed that same assertion, and
all 18 review-triage integration tests passed in 4.93 seconds. Formatting and
diff checks passed.

This is a native startup regression fix, not a same-host Hunk benchmark result.
Full workspace, cross-platform build/package and release verification remain
required, as do the latency and peak-memory parity gates. No source-ledger
interval is newly mapped by this optimization.
