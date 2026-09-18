# Main CI migration

The pinned `.github/workflows/ci.yml` had separate Bun/Node validation,
terminal-smoke, npm-package, prebuilt-package, and binary jobs. The native
workflow keeps the `changes` job and detector, native validation, Rust validation, terminal smoke, package smoke,
five-host matrix, artifacts, and fail-closed source checks while
calling `cargo xtask` and Cargo directly.

The Bun and npm jobs are represented by the native workspace tests, release
build, architecture check, dependency-license inventory, and sole-executable
smoke. Their JavaScript runtime and npm package behavior is not retained or
executed. The Rust workflow is the source of truth for CI; the pinned workflow
is read through Git only by the semantic-port verifier.
