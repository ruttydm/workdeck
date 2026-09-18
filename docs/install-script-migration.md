# Install script migration

The pinned Hunk installer contract is represented by Workdeck's native Rust installer.  The
source blobs are read from the protected Git pins at verification time. No TypeScript source
mirror is committed and no JavaScript runtime is shipped.

## Pinned coverage

| source | baseline bytes / lines / SHA-256 | stable-v0.20.1 bytes / lines / SHA-256 |
| --- | --- | --- |
| `install.sh` | 20,132 / 554 / `566ac5bce9f7c04ffc356aadc3e7aca14ddd5c2929d95bfe3b3bb98d6a08b4dc` | 12,810 / 372 / `f15239daaeea4179caee3d96b564cd200ce332fa5377486ca00a90e60e8283ec` |
| `scripts/install-sh.test.ts` | 13,638 / 361 / `fd0ff678cf220285c22a9ca7bd5908e8422fe927b03a960b70c002928785513e` | 6,536 / 157 / `f89381df3304faff020ef1b489d32d7e52fe437dafad9bcbe7a9b87aed08bbd3` |
| `scripts/install-bin.ts` | 2,011 / 60 / `0ed22e1a045724112cf02c8c10ff88e0ab71030fe57b7196f5961b3ff25c0c89` | 2,011 / 60 / `0ed22e1a045724112cf02c8c10ff88e0ab71030fe57b7196f5961b3ff25c0c89` |
| `scripts/stage-install-script.ts` | 1,101 / 28 / `059742cafd9d6c8bd6e17f64ef673e6849c148056176cd411954a5a134c1d081` | 1,101 / 28 / `059742cafd9d6c8bd6e17f64ef673e6849c148056176cd411954a5a134c1d081` |

The baseline installer exposes platform detection, downloader selection, release resolution,
checksum verification, archive extraction, bundled-skill placement, atomic swaps, shell startup
updates, and conflict diagnostics.  The stable pin removes the competing-install helpers; both
surfaces are checked explicitly and their byte intervals are recorded as non-overlapping ledger
records.

## Native projection and evidence

`workdeck-cli` owns the release downloader, platform/Rosetta selection, authenticated staging,
archive safety checks, atomic installation, conflict identity/manager diagnostics, and shell PATH
transactions.  `port/hunk/install-platform-oracle.json` freezes all 20 baseline/stable platform
cases, including unsupported operating systems and architectures.  The native installer and
archive/PATH tests cover every source test, including conflict aliases, inactive NVM installs,
force handling, skill resolution, checksums, and shell quoting.

The former local `install-bin.ts` build-and-copy helper is replaced by the same native first-install
and authenticated archive path; no `hunk` or `otdiff` alias is created and no Bun process is
started by Workdeck tooling.

The pinned `stage-install-script.ts` final website-build step is replaced by
`xtask::site_markdown::stage_install_script`. It stages the Workdeck-native
`site/static/install.sh` into the rendered output, verifies exact bytes, is idempotent when Zola
has already copied the static asset, and rejects conflicting or symlinked destinations. The
staging test exercises both the collision and symlink boundaries without writing outside the
owned output directory.

The verifier is `cargo xtask verify` (and runs as part of strict `cargo xtask port audit`).  It
checks both protected pins, exact hashes and line counts, function/test surfaces, required native
anchors, the dual-pin platform oracle, and this document before the ledger interval is mapped.
