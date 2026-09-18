# Hunk Changesets configuration migration

These configuration artifacts are retained as pinned MIT source evidence and
validated through `git show`; no JavaScript configuration or package manager is
part of Workdeck's final tree.

| Pinned artifact | Native owner | Verification |
| --- | --- | --- |
| `.changeset/README.md` | `docs/native-release-fragments.md` | `changeset_config::verify_readme` |
| `.changeset/config.json` | `xtask/src/changelog/fragments.rs`, Cargo workspace metadata, and `release/fragments/` | `changeset_config::verify_config` |
| `.changeset/pre.json` | `xtask/src/release_notes.rs` and `release/prerelease.json` | `changeset_config::verify_prerelease_state` (native prerelease validator) |

The README's authoring, version preparation, and post-publication guidance is
adapted into the native release-fragment guide. The config's public access,
main-branch, patch dependency, and ignored-package policies are represented by
the Cargo-owned fragment workflow; the old package target is deliberately not
published. The pinned beta state is represented by the native prerelease JSON
validator, which rejects malformed or inconsistent state before release work.

Source files are from Hunk `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`, MIT,
copyright Modem Labs Inc.; see `THIRD_PARTY_NOTICES`.
