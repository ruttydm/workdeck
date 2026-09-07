# Native PTY extension fixture

This compiled test fixture translates the embedded Hunk MIT extension implementations in
`test/pty/extensions-integration.test.ts`. It is not included in Workdeck release archives.

The Rust PTY harness stages the binary and manifest in a disposable directory and writes a
`fixture-kind` file beside the manifest. Supported kinds are `sidebar`, `slots`, `edges`,
`dialog`, `notify`, `shutdown`, `transform`, `highlight`, `reveal`, and `held-reveal`.
Each kind registers only its corresponding contract. Rendering is declarative and host-owned;
there is no React, JSX engine, or JavaScript execution.

Run the executable tests with:

```text
cargo test -p workdeck-cli --test terminal_pager extensions::
```

The shutdown fixture takes the session working directory from the native handshake. The
held-reveal fixture retains the selected file identity from its first pane render and asks
the host to resolve the current measured line when its later command is invoked.
