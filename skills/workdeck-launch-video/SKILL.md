---
name: workdeck-launch-video
description: Capture reproducible Workdeck terminal media using declared PTY, Chromium, and FFmpeg tools.
---

# Workdeck terminal media

This source-checkout-only workflow produces feature demos, explainers, announcements, and release
roundups from the real Workdeck TUI. It has three deterministic stages:

```text
cargo xtask media launch capture   drive Workdeck through a PTY and rasterize styled keyframes
cargo xtask media launch compose   render keyframes, cards, and captions on a 1920x1080 stage
cargo xtask media launch encode    encode the ffconcat plan to MP4 and WebM
```

Rust owns the PTY, VT state, PNG rasterization, storyboard planning, WebDriver protocol, static
HTML/CSS stage, and FFmpeg orchestration. Chromium/WebDriver and FFmpeg are declared external
executables. There is no Bun, Node, Playwright, JavaScript application, or screen recording.

## Choosing a recipe

- For a single feature, capture one scene group with `--scenes`, make a temporary copy of the JSON
  storyboard, trim it to an opening card, that feature's frames, and an outro, then use the generic
  `cargo xtask media compose --storyboard ...` command. Keep the scratch storyboard uncommitted.
- For a full release, derive four to six user-visible headlines from the release notes, update the
  canonical capture scenes and `media/launch/storyboard.json`, capture every referenced frame, then
  compose and encode both formats.
- For a custom tutorial or comparison, use the generic JSON capture and compose commands documented
  in `docs/terminal-media.md`; the canonical launch wrapper intentionally owns its storyboard.

## Full pipeline

Run from the repository root. Use absolute executable/font paths in automation.

```sh
# Capture all scenes. This builds target/debug/workdeck by default.
cargo xtask media launch capture --font /path/to/mono-font.ttf

# Compose. The driver and browser must be protocol-compatible.
cargo xtask media launch compose \
  --font /path/to/mono-font.ttf \
  --webdriver /path/to/chromedriver \
  --chromium /path/to/chromium

# Encode with an FFmpeg build containing libx264 and libvpx-vp9.
cargo xtask media launch encode --ffmpeg /path/to/ffmpeg
```

All commands default to `.video-work/`; use `--work-dir` consistently to override it. Capture accepts
`--binary` for an already-built executable. Compose also accepts the generic viewport and timing
flags. Encode accepts `--mp4` and `--webm`; relative output names are placed under the work directory.

Expect capture and composition to take several minutes. They report progress per scene, snap, and
frame. Use a long timeout or a supervised background terminal.

To iterate on one or more scene groups:

```sh
cargo xtask media launch capture --font /path/to/mono.ttf --scenes review
SCENES=triage,fileview cargo xtask media launch capture --font /path/to/mono.ttf
```

Valid groups are `review`, `stml`, `cli`, `pager`, `triage`, and `fileview`. Filtering only narrows
capture. `manifest.json` describes the current capture invocation, but `frames/` remains cumulative.
Composition preflights all 38 keyframes referenced by the canonical storyboard and names every
missing frame before launching WebDriver.

## Canonical editorial surface

`media/launch/storyboard.json` is the whole edit: cards, titles, captions, frame names, and durations.
The Rust functions in `xtask/src/term_video/launch.rs` are the current scene list and Workdeck-side
glue: isolated configuration, temporary Git repositories, the shell wrapper, native extension
staging, keyboard liveness probes, and exact input/snapshot sequences.

For each new release or scoped cut, review:

- every version/release badge, headline, install command, and footer;
- every claim-like badge and every feature caption;
- the ordering and duration of shots, including generated cursor and typing sequences;
- whether scenes demonstrate shipped functionality or clearly labelled example extensions.

Do not claim an unpublished installer or package. The checked-in reference card uses source-build
commands until a release channel is verifiably available. Shell-scene titles remain generic because
capture starts a clean interactive shell rather than a branded terminal.

## Authoring capture scenes

- Keep all scenes at 140x32 cells with one declared monospaced font. Default rendering uses 16px
  glyphs, 1.5 line height, and device-pixel ratio 2.
- Drive real Workdeck behavior. Never replace a hard-to-capture state with a mock image.
- Use a scene-specific text wait before the first snapshot and the `Workdeck help` keyboard probe
  before scripted navigation. Startup can race the first keypress.
- Animate movement or typing with one snapshot per meaningful keypress/character. Sparse stills read
  as a slideshow.
- Preserve stable frame names. The canonical cursor sequence is `review-walk-00..13`; changing loop
  bounds and storyboard references independently must fail preflight.
- The review scene builds a real Git repository from `examples/2-mini-app-refactor`, commits the
  `before` tree, and overlays `after` as its dirty working tree.
- STML content comes from `examples/9-agent-markup-notes`; CLI rendering uses a temporary `note.stml`.
- The pager scene runs `git diff | workdeck pager` from a clean shell with a temporary `workdeck`
  wrapper on `PATH`.
- Triage and file-view scenes stage and execute the compiled native examples. Explicit extension
  paths avoid repository trust prompts; they do not weaken normal repository-extension trust rules.

Capture actions and paths belong in the Rust launch module when they are part of the reusable
canonical demo. One-off evidence should use a temporary JSON capture script and stay uncommitted.

## Storyboard semantics

Each shot has `kind` and `dur`. Cards carry trusted stage HTML. Terminal shots carry `img`, `title`,
and optional `caption`/`capKey`. `enter` animates the full surface. A changed `capKey` animates a new
caption; continuation shots with the same key inherit the prior caption without replaying it.

The 41-shot reference cut uses 38 distinct terminal PNGs and its storyboard plan totals 63.57
seconds. Hold durations cost one composited PNG; entrance and caption windows expand to 30fps
frames. FFmpeg's image-demuxer time base can quantize the final container duration, so treat
`ffprobe` as authoritative for encoded output. Keep important frames for three to four seconds,
context frames for two to three, and cursor or typing steps near 0.2 to 0.6 seconds.

Allowed caption/card HTML is the stage vocabulary in `xtask/assets/terminal-stage.html`: `badge`,
`hl`, `dim`, headings, `sub`, `cmds`/`cmd`, and `foot`. The compositor escapes titles and generated
URLs but intentionally accepts canonical card/caption markup.

## External-tool checks

- Use a WebDriver matching the Chromium major version. Set explicit `--webdriver` and `--chromium`
  paths in CI. `WEBDRIVER_PATH` and `CHROMIUM_PATH` are supported by the generic compositor.
- Chromium receives headless, fixed-window, and local-file-access arguments. The latter is required
  because the static stage samples the terminal PNG background through a canvas.
- Verify `ffmpeg -encoders` lists both `libx264` and `libvpx-vp9`. Browser-bundled FFmpeg binaries may
  lack H.264 and are insufficient.
- Provide a licensed monospaced TrueType/OpenType font explicitly. Package pipelines may instead put
  `jetbrains-mono-nerd.ttf` under `share/workdeck/fonts/`.
- Do not add browser downloads, fonts, or encoded videos to Git implicitly. Their provenance and
  license must be explicit before any retained asset is committed or packaged.

## Verification and delivery

1. Inspect every new PNG in `.video-work/frames/`, especially cursors, combining/wide characters,
   extension panes, and short shell output.
2. Run compose and confirm its 38-keyframe preflight accepts the capture, then verify the reported
   295 planned frames and 63.57-second storyboard total.
3. Encode both outputs and check duration with `ffprobe`.
4. Extract stills around the opening, every feature transition, mid-caption animation, and outro:

   ```sh
   ffmpeg -y -ss 2 -i .video-work/launch.mp4 -frames:v 1 .video-work/check.png
   ffprobe -show_entries format=duration .video-work/launch.mp4
   ```

5. Confirm captions persist across continuation frames, the terminal background matches the window
   body, geometry does not jump, and the video is silent.
6. Keep raw frames, built stage, concat list, and verification stills together until review is done.

Outputs stay under `.video-work/` and are ignored by Git. Return both MP4 (chat/social) and WebM (web
embedding), report duration and file sizes, and flag unusually large deliverables. Do not upload,
publish, or edit a public release without separate authorization.
