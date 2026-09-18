# CLI tools extension

This compiled Rust extension demonstrates a generic top-level command tree. The handler owns every token below `cli-tools`, streams headless output through Workdeck-owned stdout and stderr, receives cooperative cancellation, and can hand terminal ownership to one built-in Workdeck command.

Stage the executable and manifest from this checkout, then run it explicitly:

```bash
cargo xtask extension stage-example cli-tools
cargo run -p workdeck-cli -- --extension ./target/workdeck-extension-examples/cli-tools cli-tools status
cargo run -p workdeck-cli -- --extension ./target/workdeck-extension-examples/cli-tools cli-tools review
```

`status` writes to stdout and exits. `review` performs cancellation-aware preprocessing for 100 ms, writes progress to stderr, then delegates to `workdeck diff`. A delegating handler may not write stdout or read stdin because the built-in command or TUI takes ownership of both.

The example preserves its raw argument tokens. Native extensions may use their own parser, make network requests, spawn processes, or access the filesystem under Workdeck's ordinary extension trust model. They are trusted processes with crash isolation, not security sandboxes.

The diagnostic `write-burst` action emits 128 separate stdout frames and exits. Its
compiled integration test deliberately delays the first host write, verifies every
byte and frame order beyond the host's eight-frame serialized response queue, and
then runs another command on the same extension. Backpressure must not discard or
truncate CLI output.
