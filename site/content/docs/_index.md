+++
title = "Workdeck documentation"
template = "docs.html"
page_template = "docs.html"
+++

Workdeck's full Rust semantic port is in progress. The pages here distinguish
available source-build workflows from release channels that still require
qualification. Passing individual tests does not establish complete Hunk parity.

- [Install and verify a source build](/docs/start/install/)
- [Review quick start and input controls](/docs/start/)
- [Keybindings, layout and display](/docs/configure/)
- [Git integration workflows](/docs/workflows/)
- [Compatibility and troubleshooting](/docs/help/)
- [Agent-assisted review guidance](/docs/agents/)
- [Legacy extension migration inventory](/extensions/)

The repository's `port/hunk/ledger.jsonl` and strict `cargo xtask port audit`
remain the source-coverage gate. These initial pages do not replace the full
upstream documentation corpus, whose migration is unfinished.
