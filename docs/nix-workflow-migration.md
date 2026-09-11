# Nix workflow migration

Hunk's pinned `.github/workflows/nix.yml` had two jobs: a code-only change
detector and a conditional Nix package job. The detector resolved shallow
revisions, classified Markdown/docs/assets/LICENSE-only changes, exposed
`code_changed` through `GITHUB_OUTPUT`, and the package job evaluated all
systems, built the flake, and smoke-tested the installed `hunk` binary and
skill path.

Workdeck preserves that boundary in the native `.github/workflows/nix.yml`:

- the pinned `.github/scripts/detect-code-changes.sh` is owned by the Rust
  `xtask::ci_changes` implementation;
- `cargo xtask ci-changes "$BASE_SHA" "$HEAD_SHA"` owns revision fetching,
  zero-tree handling, no-rename diff classification, and the output contract;
- `cargo xtask nix check`/the Nix commands own flake evaluation and the native
  `workdeck --help` plus `skill path` smoke test;
- the package job runs the native Nix flake checks across supported systems;
- the workflow keeps the pull-request/main triggers, concurrency, conditional
  package job, and Nix substituter configuration.

`xtask::ci_changes::verify_workflow` reads both pinned blobs from the protected
baseline and checks every branch against the native implementation. The
upstream shell and Bun workflow are not copied or executed, and no Hunk runtime
is shipped.
