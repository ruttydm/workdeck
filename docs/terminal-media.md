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
`totalSeconds`).

## Static compositor

The native compositor validates all named keyframes before launching an external process, generates
one static HTML state per unique frame, drives a declared WebDriver/Chromium pair, writes
`out/f0000.png`-style screenshots, and produces the ffmpeg concat-demuxer input with the final frame
repeated:

```console
cargo xtask media compose \
  --storyboard media/launch/storyboard.json \
  --work-dir target/media/launch \
  --font /path/to/jetbrains-mono-nerd.ttf \
  --webdriver /path/to/chromedriver \
  --chromium /path/to/chromium
```

`--frames-dir` defaults to `<work-dir>/frames`. `--root-dir` is used only for packaged font
discovery; Workdeck checks `share/workdeck/fonts/jetbrains-mono-nerd.ttf` before the source-tree
asset location. `WEBDRIVER_PATH` and `CHROMIUM_PATH` provide explicit environment equivalents, and
Chromium may otherwise use the WebDriver's default browser discovery.

The checked-in stage is HTML and CSS only. Rust owns title/caption escaping, trusted storyboard HTML,
quartic easing, terminal-image URL injection, bottom-left PNG background sampling, browser lifecycle,
paint synchronization, screenshot decoding, progress reporting, and ffconcat serialization. The
external browser is used only as a renderer. No Node, Bun, Playwright, JavaScript application, or
embedded JavaScript engine is part of the repository tooling or shipped product. FFmpeg encoding is
the next declared pipeline stage and consumes the generated `concat.txt`.
