# Pull-request CI migration

The pinned `pr-ci.yml` combined release-note checks, Windows compatibility,
compiled-headless portability, terminal smoke, and a broad Bun/Node/npm
validation job. The native workflow keeps the `changes` gate, release-note
verification, Windows Rust matrix, cross-target `workdeck` build, static-site
check, workspace tests, Clippy, and executable smoke.

The native validation workflow replaces the former JavaScript runtime and npm/prebuilt package jobs; they are not retained.
Their runtime-boundary assertions are covered by the native CLI tests and
architecture verifier; this workflow is the review-time validation source of
truth and remains pull-request scoped.
