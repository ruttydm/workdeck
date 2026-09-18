# Competitor benchmark migration

The pinned Hunk workload `benchmarks/competitors.ts` is retained as a source
oracle at both the baseline (`2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`) and
stable (`4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd`) pins. It is 3,432 bytes,
112 lines, and has SHA-256
`5b09707e6ee9916d3c44ce6f559ddb2e16973fb7fe0e1eac8e20ca0302ce74e0` at both
pins.

Workdeck replaces its Bun process probes with the native Rust command
`cargo xtask benchmark competitors`. The same deterministic 96-file patch and
changed-repository fixture is used, followed by comparisons against Git,
`delta`, `difftastic` (or `difft`), and `diff-so-fancy`. Git is always measured;
the other tools are optional and report an explicit `*_available=0` metric when
not installed. A non-zero optional tool exit is reported as unavailable with
its stderr, matching the source workload's informational semantics.

The Rust benchmark owns temporary fixtures and removes them on every exit path,
sets `NO_COLOR=1` and `TERM=xterm-256color`, discards tool stdout, and emits the
same metric names. It never invokes a JavaScript runtime or executes the pinned
TypeScript source; there is **no JavaScript runtime** in this path. The verifier reads both pinned blobs through Git, checks
their byte count, line count, hash, and markers, and requires the native
runner, docs, and evidence surfaces to remain present.
