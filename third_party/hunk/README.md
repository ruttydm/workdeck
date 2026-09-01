# Retained Hunk media

This directory contains byte-exact media from the pinned Hunk source tree. The files are retained
solely as licensed third-party port evidence and visual-oracle inputs. Workdeck does not ship or
load them in its executable or website.

`assets.jsonl` records the upstream path, Git blob, byte count, destination, and SHA-256 digest for
every retained file. Run `cargo xtask port materialize-assets` to reproduce the directory from the
protected `hunk-port/main-2c00f435` source anchor.

Hunk is MIT licensed. Its attribution is preserved in `THIRD_PARTY_NOTICES`.
