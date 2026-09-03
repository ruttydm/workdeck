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
