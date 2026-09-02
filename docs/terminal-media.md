# Terminal media tooling

Workdeck's terminal-media pipeline is Rust-owned and deterministic. The first completed layer is
the pure storyboard planner:

```console
cargo xtask media plan --storyboard media/launch/storyboard.json --output target/media/plan.json
```

The storyboard may be a JSON array of shots or an object with a `shots` array. A `card` shot carries
HTML; a `term` shot carries an image key, terminal title, caption, and optional caption identity.
`enter` animates the full surface. Frames inside animation windows are emitted individually, while
the rest of each shot becomes one timed hold, so deterministic videos do not require thousands of
duplicate PNGs.

The output preserves the Hunk planner's camelCase frame-state contract (`shotT`, `capT`, and
`totalSeconds`). The Rust capture and compositor stages consume this plan and invoke explicitly
declared Chromium/WebDriver and FFmpeg binaries; no JavaScript runtime is part of the repository or
shipped product.
