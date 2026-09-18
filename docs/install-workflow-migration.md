# Installation workflow migration

The pinned `install-sh-e2e.yml` exercised release discovery, archive download,
checksum validation, PATH updates, idempotency, custom directories, and
unknown-version failures for the shell installer. The native workflow keeps
those contracts in the native archive Rust installation transaction and oracle tests, runs on
Linux, macOS, and Windows, and smoke-tests the sole `workdeck` executable.

`cargo xtask install-plan --no-modify-path` provides the read-only installation plan
used before any destination write. Archive authentication, atomic
replacement, recovery, and custom directory behavior are tested in
`workdeck-cli`'s native install modules. No shell runtime or legacy installer
is retained; no shell runtime is shipped.
