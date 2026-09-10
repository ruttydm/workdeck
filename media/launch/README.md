# Workdeck launch-video pipeline

This directory contains Workdeck's canonical product-video storyboard. The Rust `xtask` owns the
capture scenes and generic PTY, planning, compositing, and encoding machinery; this directory owns
the current cards, captions, shot order, and timing.

```console
cargo xtask media launch capture --font /path/to/mono-font.ttf
cargo xtask media launch compose --font /path/to/mono-font.ttf --webdriver /path/to/chromedriver --chromium /path/to/chromium
cargo xtask media launch encode --ffmpeg /path/to/ffmpeg
```

All stages default to `.video-work/`. Capture drives the real `workdeck` binary in isolated
temporary repositories and configuration roots. The `manifest.json` describes only the latest
capture run, while `frames/` remains cumulative so `--scenes` can update one section. Compose
preflights all 38 storyboard keyframes and emits `out/` plus `concat.txt`; encode produces MP4 and
WebM using libx264 and libvpx-vp9.

The complete authoring, environment, verification, and delivery procedure is in
[`skills/workdeck-launch-video/SKILL.md`](../../skills/workdeck-launch-video/SKILL.md).

## Native review capture checkpoint

The review scene's keyboard probe now expects the current `Controls help` dialog
title. A real PTY run exposed the stale `Workdeck help` expectation: the keypress
worked, but the probe reported failure. After correction the complete review
scene captured 17 keyframes, including scrolling, draft creation, typed text and
saved note state, using the freshly built native executable.

For the documentation image, capture ran with `NO_COLOR` unset for that process
only, a 140×32 terminal and the host's `/System/Library/Fonts/Menlo.ttc`. The font
file is not redistributed. `site/static/docs/images/review-stream-native.png`
is the unedited `review-walk-00` frame, SHA-256
`602a383272c92dec03031b256fa4807b0d3c08afe74b871a1db33d340efa4e96`.
The demo repository and configuration were temporary. This is native capture
evidence, not a dual-baseline terminal-golden or performance-parity result.
