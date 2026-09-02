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

## Native PTY capture

`cargo xtask media capture` replaces Hunk's Bun/tuistory capture helpers with a native PTY driver,
a Ghostty-derived Rust terminal model, and a direct RGB PNG renderer:

```console
cargo xtask media capture --script media/launch/capture.json
cargo xtask media capture --script media/launch/capture.json --scenes review,pager
```

`SCENES=review,pager` remains the environment equivalent when `--scenes` is absent. A missing scene
filter runs every scene; comma-separated names retain Hunk's exact trimming and empty-name
semantics. The script fixes one `cols`/`rows` geometry and one font for the run, then declares app
or clean interactive-shell launches and ordered actions:

```json
{
  "cols": 140,
  "rows": 32,
  "framesDir": "target/media/launch/frames",
  "font": "third_party/fonts/jetbrains-mono-nerd.ttf",
  "manifest": "target/media/launch/manifest.json",
  "scenes": [{
    "name": "review",
    "launch": {
      "kind": "app",
      "command": "target/debug/workdeck",
      "args": ["diff", "--mode", "stack"],
      "cwd": ".",
      "env": { "WORKDECK_DISABLE_UPDATE_NOTICE": "1" }
    },
    "actions": [
      { "kind": "waitText", "pattern": "src/", "timeoutMs": 60000 },
      { "kind": "probe", "key": "?", "pattern": "Controls help", "dismissKey": "escape" },
      { "kind": "press", "key": "j" },
      { "kind": "snap", "name": "review-cursor" }
    ]
  }]
}
```

Shell scenes may add `pathPrepend` entries and executable `wrappers`; the wrapper text and Unix mode
match the pinned Hunk helper. Actions also support `sleep`, `write`, delayed Unicode `type`, and
`typeCommand` with zero-based `snapAt` character indexes. Frame names are path-safe, manifests are
pretty JSON, and a partial scene run leaves prior PNGs intact while describing only the current
run—matching the original capture-side inventory behavior.

The reader thread owns only the blocking PTY read. The terminal model stays on its creating thread
and receives complete byte chunks through a channel, preserving UTF-8 sequences across reads
without unsafe cross-thread terminal pointers. Snapshots retain combining characters, wide-cell
spacers, dynamic 256-color palettes, inverse/faint/bold/italic/invisible text, all underline styles,
strike/overline decoration, and block/bar/underline/hollow cursors.

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
embedded JavaScript engine is part of the repository tooling or shipped product. FFmpeg encoding
consumes the generated `concat.txt`.
