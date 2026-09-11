+++
title = "Install"
description = "Install Workdeck with its native installer, Cargo, Homebrew, mise, or Nix and verify the CLI."
template = "docs.html"
+++

Workdeck runs on macOS, Linux, and Windows as one native Rust executable named
`workdeck`. The install script is the default method on macOS and Linux; Cargo,
Homebrew, mise, Nix, and the signed release archives cover the other workflows.
Workdeck does not ship an npm package, Bun runtime, Node.js launcher, or `hunk`
alias. Git is recommended for the most common review workflows.

## Install script (default)

On macOS and Linux, the default install script downloads and verifies the
prebuilt binary for the detected machine:

```bash
curl -fsSL https://workdeck.dev/install.sh | sh
workdeck --version
```

Release archives contain `SHA256SUMS`, and the script requires `shasum` or
`sha256sum` before it installs. A missing verifier is an error rather than a
silent integrity downgrade. It installs into `~/.workdeck` (binary at
`~/.workdeck/bin/workdeck`, bundled skills beside it) and can add that directory
to the shell startup file. Restart the shell afterwards.

The script accepts these settings:

| Setting | Effect |
| --- | --- |
| `WORKDECK_VERSION` | Install an exact release instead of the newest one; also accepted as a positional argument. |
| `WORKDECK_INSTALL_DIR` | Install the binary into this directory instead of `~/.workdeck/bin`. |
| `--no-modify-path` (or `WORKDECK_NO_MODIFY_PATH=1`) | Leave shell startup files alone. |
| `--force` (or `WORKDECK_ALLOW_CONFLICTING_INSTALLS=1`) | Install despite another Workdeck on PATH or in a known version-manager directory. |

By default the installer refuses to create a second Workdeck installation. It
lists every competing path it finds, its version and PATH precedence, and the
command that removes it. Remove those installs first; use `--force` only when
you deliberately manage multiple copies.

```bash
curl -fsSL https://workdeck.dev/install.sh | sh -s -- 0.20.0
curl -fsSL https://workdeck.dev/install.sh | sh -s -- --no-modify-path
curl -fsSL https://workdeck.dev/install.sh | sh -s -- --force
curl -fsSL https://workdeck.dev/install.sh | WORKDECK_VERSION=0.20.0 sh
```

`workdeck update` refreshes a default install in place. An install redirected
with `WORKDECK_INSTALL_DIR` cannot be auto-detected later (the variable is gone
once the shell exits), so update one of those by re-running the script with the
same directory; the installer prints a reminder at the end of a custom-directory
install. Windows is covered by the signed archive, PowerShell installer, Cargo,
Homebrew-on-Linux, or mise rather than the POSIX script.

## npm

There is deliberately no published `workdeck` npm package and no JavaScript
runtime dependency. A legacy Hunk npm installation must be removed or kept
separate while installing the native Workdeck binary. Verify that the command
resolves to the native executable:

```bash
workdeck --version
```

Do not use `npm`, `bun`, or `pnpm` to install or update Workdeck. The absence of
those package-manager commands is a release invariant, not a missing feature.

## Homebrew

The release process publishes a signed Homebrew formula once the native archive
and checksums have passed the platform gate:

```bash
brew install workdeck
workdeck --version
```

If an older tap formula is present, remove it before installing the canonical
formula:

```bash
brew uninstall modem-dev/tap/workdeck
brew install workdeck
```

Homebrew owns updates for Homebrew installs; `workdeck update` reports the
detected method and does not overwrite a different package manager's files.

## mise

mise can install a released Workdeck binary on macOS, Linux, and Windows after a
Workdeck tool definition is available:

```bash
mise use -g workdeck
workdeck --version
```

Use a current mise release so platform/architecture selection and checksum
verification are available. Workdeck does not register a `hunk` or `hunkdiff`
alias. Omarchy or another distribution may package Workdeck through its own
native tool definition; the distribution remains responsible for that package.

## Nix

The repository exports a native `workdeck` package from `flake.nix`. From a
clone of Workdeck:

```bash
nix build
./result/bin/workdeck --version
```

See `nix/README.md` for Home Manager and development-shell details. Nix owns
updates for Nix-managed installations.

## Verify the install

```bash
workdeck --help
```

You should see `Usage: workdeck <command> [options]` and no JavaScript runtime
path. If the shell cannot find Workdeck, ensure the selected Cargo, Homebrew,
mise, Nix, or `~/.workdeck/bin` directory is on `PATH`, then open a new shell.
`workdeck verify-install` can additionally print the resolved binary, release
digest, and provenance file without creating repository state.

## Update Workdeck

`workdeck update` is the canonical update command for default install-script,
Cargo, Homebrew, Nix, mise, curl/PowerShell, and direct GitHub archive installs.
It selects the recorded installation method and verifies the new archive before
the atomic replacement:

```bash
workdeck update          # install the newest release
workdeck update --check  # check without installing
workdeck update 0.20.0   # install an exact release
```

On an older Workdeck release, update once with the installer or package manager
that installed it; after that, use `workdeck update`. mise, Nix, and local source
builds remain owned by their own tooling, so use `mise upgrade workdeck`, your
Nix configuration, or a deliberate Cargo rebuild. A custom
`WORKDECK_INSTALL_DIR` also requires re-running the installer with the same
directory. Pass `--method cargo`, `--method brew`, `--method nix`,
`--method mise`, `--method curl`, or `--method github` if Workdeck detects the
wrong method. Every update keeps the previous binary until checksum, signature,
SBOM, and provenance checks succeed.

Next, [review your first working tree](/docs/start/quick-start/).

Adapted from Hunk's MIT-licensed installation documentation, Copyright Modem
Labs Inc. The package-manager and runtime portions are intentionally translated
to Workdeck's native release model; no TypeScript source or npm package is
retained.
