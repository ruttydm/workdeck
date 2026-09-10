+++
title = "Install and verify"
description = "Build Workdeck from source and understand the native installer and update boundaries."
template = "docs.html"
+++

Workdeck ships one executable named `workdeck`. It does not expose a `hunk`
alias or use npm, Bun, Node.js, or a JavaScript runtime. The semantic port and
cross-platform release qualification are still in progress.

## Build from a checkout

Use a Rust toolchain compatible with the checkout's `Cargo.toml` and lockfile.
From the repository root:

```sh
cargo build --release --locked -p workdeck-cli --bin workdeck
./target/release/workdeck --help
```

On Windows, the resulting executable is `target/release/workdeck.exe`.
To install the checkout using Cargo's normal installation directory:

```sh
cargo install --path crates/workdeck-cli --locked
workdeck --help
```

If the shell cannot find the command, check the installation directory reported
by Cargo and your PATH. Keep existing installations intact until you have
identified which executable your shell resolves.

## Native release installer

An existing Workdeck executable exposes `workdeck install [VERSION]` with
`--destination DIRECTORY`, `--no-modify-path`, and `--force`. This is a native
release installer, not a bootstrap command that can run before Workdeck exists.
It requires suitable published release artifacts and verification evidence;
the presence of the command does not prove a release is available or qualified.

The default destination is the home directory's `.workdeck` directory. A custom
destination is an installation root, not a request to overwrite an existing
binary. `--force` allows known competing installations; it does not authorize
overwriting the destination. `--no-modify-path` leaves shell startup files alone.
Do not assume Hunk's npm packages, Homebrew formula, mise aliases or installer
URLs install Workdeck.

## Updates

Inspect the implemented interface with `workdeck update --help`. It accepts an
optional version, `--check`, and a `--method` override for Cargo, Homebrew, Nix,
curl, PowerShell or direct installations. Qualification of every installer and
update path remains a release gate; no published package availability is implied.
For a source checkout, update the checkout deliberately and rebuild using the
commands above. There is no Workdeck npm update path.

## Verification and provenance

Use `workdeck --help` to verify that the expected executable starts. Release
archives are required to carry licensing, checksums, SBOM and provenance; those
requirements are not satisfied merely by a successful local build. Follow the
repository's release procedure before distributing a build.

This page adapts installation topics from Hunk's MIT-licensed documentation,
Copyright Modem Labs Inc. It is not a completed mapping of that source page;
remaining migration and release-channel documentation is tracked in the port ledger.

Next, [review a working tree or commit](/docs/start/quick-start/).
