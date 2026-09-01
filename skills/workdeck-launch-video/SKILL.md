---
name: workdeck-launch-video
description: Capture reproducible Workdeck terminal media using declared PTY, Chromium, and FFmpeg tools.
---

# Workdeck terminal media

Use deterministic repositories, terminal dimensions, themes, fonts, and input timings. Record the
real `workdeck` binary through the Rust terminal-media tooling; Chromium/WebDriver and FFmpeg are
declared external executables. Preserve the raw VT capture beside the composed output so cell
geometry can be audited independently from video encoding.
