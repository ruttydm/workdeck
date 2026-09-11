# Prebuilt release workflow migration

This is the single native release workflow for the product.

The pinned `.github/workflows/release-prebuilt-npm.yml` coordinated release
channels, benchmark comparison, five platform binaries, staged npm packages,
trusted publishing, checksums, attestations, and a GitHub release. Workdeck
uses one native release workflow for the same platform builds, benchmark and
strict-port gates, signed archives, SBOMs, provenance, checksums, and GitHub
publication.

The npm/Bun/Node package branch is deliberately not published: Workdeck ships
one MIT-licensed `workdeck` executable and updates through Cargo, Homebrew,
Nix, curl/PowerShell, or direct GitHub archives. The complete pinned workflow
is inspected by the Rust verifier, while no legacy package runtime is kept in
the repository.
