//! MIT translation of Hunk's wrapped Japanese Markdown first-paint/burst workload.
//! Mount timing includes renderer setup; burst timing excludes synchronous syntax preparation.

use super::*;
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{buffer::Buffer, layout::Rect};
use std::time::{Duration, Instant};
use workdeck_core::{AppBootstrap, CliInput, InputLayoutMode};
use workdeck_review::LayoutMode;
use workdeck_tui::{ReviewApp, ReviewOptions, VIEWPORT_READ_COALESCE_MS, render, resolve_theme};

const VIEWPORT: Rect = Rect::new(0, 0, 240, 60);
const PHYSICAL_LINES: usize = 518;
const BURST_EVENTS: usize = 12;
const LONG_REPEATS: usize = 96;
const PARAGRAPH: &str = "日本語のコメント行および、改行入力されている前提だが、折り返し描画のコストは体感的な引っかかりを生む。長い文章を繰り返して、スクロール中の描画と選択位置が安定していることを確認する。";

fn issue_lines() -> Vec<String> {
    (0..PHYSICAL_LINES)
        .map(|index| {
            format!(
                "項目{}: {}",
                index + 1,
                if index % 3 == 0 {
                    "短い編集書き".into()
                } else {
                    PARAGRAPH.repeat(2)
                }
            )
        })
        .collect()
}

fn bootstrap(id: &str, lines: &[String]) -> Result<AppBootstrap> {
    let path = format!("benchmarks/{id}.md");
    let mut patch = format!(
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    let mut file = workdeck_diff::parse_single_file_patch(&patch, &path, None)?;
    file.runtime_id = id.into();
    file.language = Some("markdown".into());
    let mut bootstrap = stream::bootstrap(
        std::env::current_dir()?,
        vec![file],
        format!("changeset:{id}"),
    );
    if let CliInput::Vcs(input) = &mut bootstrap.input {
        input.options.mode = Some(InputLayoutMode::Split);
        input.options.wrap_lines = Some(true);
    }
    bootstrap.initial_wrap_lines = true;
    bootstrap.initial_theme = Some("github-dark-default".into());
    Ok(bootstrap)
}

struct Renderer {
    app: ReviewApp,
    buffer: Buffer,
}

impl Renderer {
    fn new(bootstrap: AppBootstrap) -> Self {
        Self {
            app: ReviewApp::new(
                bootstrap.changeset,
                ReviewOptions {
                    layout: LayoutMode::Split,
                    wrap_lines: true,
                    theme: resolve_theme(bootstrap.initial_theme.as_deref(), None, &[]),
                    command_cwd: Some(bootstrap.reload_context.cwd),
                    review_input: Some(bootstrap.input),
                    ..ReviewOptions::default()
                },
            ),
            buffer: Buffer::empty(VIEWPORT),
        }
    }
    fn render_pass(&mut self, passes: usize) {
        for _ in 0..passes {
            self.buffer.reset();
            render(VIEWPORT, &mut self.buffer, &self.app);
        }
    }
    fn content_rows(&self) -> usize {
        content_rows(&self.character_frame())
    }
    fn character_frame(&self) -> String {
        self.buffer
            .content
            .chunks(usize::from(VIEWPORT.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn content_rows(frame: &str) -> usize {
    let japanese = regex::Regex::new(r"[\p{Han}\p{Hiragana}\p{Katakana}]").unwrap();
    frame
        .split('\n')
        .filter(|line| japanese.is_match(line))
        .count()
}

fn first_frame(bootstrap: AppBootstrap) -> Result<f64> {
    let start = Instant::now();
    let mut setup = Renderer::new(bootstrap);
    setup.render_pass(1);
    let duration = start.elapsed().as_secs_f64() * 1000.0;
    let rows = setup.content_rows();
    let minimum = usize::from(VIEWPORT.height) * 8 / 10;
    if rows < minimum {
        bail!("Wrapped CJK first frame exposed blank rows: content={rows}, minimum={minimum}");
    }
    Ok(duration)
}

fn wheel_burst(bootstrap: AppBootstrap) -> Result<[f64; 5]> {
    if bootstrap.changeset.files.is_empty() {
        bail!("Wrapped CJK wheel benchmark requires one diff file");
    }
    let mut setup = Renderer::new(bootstrap);
    // Unlike the source module-global cache, native cache ownership is per app. Resolve into
    // that exact cache after construction, still before settlement and the wheel timer.
    if !setup.app.prefetch_file_highlights(0) {
        bail!("Wrapped CJK wheel benchmark could not prefetch its diff file");
    }
    setup.render_pass(2);
    std::thread::sleep(Duration::from_millis(VIEWPORT_READ_COALESCE_MS + 1));
    setup.render_pass(2);
    let initial = setup.character_frame();
    let initial_rows = content_rows(&initial);
    let start = Instant::now();
    for _ in 0..BURST_EVENTS {
        setup.app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 170,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
    }
    setup.render_pass(1);
    let immediate_ms = start.elapsed().as_secs_f64() * 1000.0;
    let immediate = setup.character_frame();
    std::thread::sleep(Duration::from_millis(VIEWPORT_READ_COALESCE_MS + 1));
    setup.render_pass(1);
    let settled_ms = start.elapsed().as_secs_f64() * 1000.0;
    let settled = setup.character_frame();
    if initial == immediate && initial == settled {
        bail!("Wrapped CJK wheel burst did not move the review viewport");
    }
    let settled_rows = content_rows(&settled);
    let immediate_rows = content_rows(&immediate);
    let minimum = (initial_rows * 8 / 10).max(1);
    if immediate_rows < minimum || settled_rows < minimum {
        bail!(
            "Wrapped CJK wheel burst exposed blank rows: initial={initial_rows}, immediate={immediate_rows}, settled={settled_rows}"
        );
    }
    Ok([
        immediate_ms,
        settled_ms,
        initial_rows as f64,
        immediate_rows as f64,
        settled_rows as f64,
    ])
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark wrapped-cjk accepts no arguments");
    }
    let lines = issue_lines();
    let issue = first_frame(bootstrap("cjk-wrap-518-first-frame", &lines)?)?;
    let long = PARAGRAPH.repeat(LONG_REPEATS);
    let long_ms = first_frame(bootstrap(
        "cjk-wrap-single-long-line",
        std::slice::from_ref(&long),
    )?)?;
    let burst = wheel_burst(bootstrap("cjk-wrap-518-wheel-burst", &lines)?)?;
    for (name, value) in [
        ("wrapped_cjk_518_mount_first_frame_ms", issue),
        ("wrapped_cjk_long_line_mount_first_frame_ms", long_ms),
        ("wrapped_cjk_wheel_burst_immediate_ms", burst[0]),
        ("wrapped_cjk_wheel_burst_settled_ms", burst[1]),
    ] {
        println!("METRIC {name}={}", fixed(value, 2));
    }
    for (name, value) in [
        (
            "wrapped_cjk_wheel_burst_initial_content_rows",
            burst[2] as usize,
        ),
        (
            "wrapped_cjk_wheel_burst_immediate_content_rows",
            burst[3] as usize,
        ),
        (
            "wrapped_cjk_wheel_burst_settled_content_rows",
            burst[4] as usize,
        ),
        ("physical_lines", PHYSICAL_LINES),
        ("long_line_characters", long.encode_utf16().count()),
        ("wheel_burst_events", BURST_EVENTS),
    ] {
        println!("METRIC {name}={value}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn content_counter_counts_japanese_rows_not_glyphs_or_other_scripts() {
        assert_eq!(content_rows("abc\n日本語かなカナ\n한글\n中文\n🚀\n"), 2);
        assert_eq!(content_rows("\n \n"), 0);
    }
    #[test]
    fn wrapped_cjk_production_frames_and_burst_do_not_expose_blank_rows() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-wrapped-cjk.json"
        ))
        .unwrap();
        let lines = issue_lines();
        assert_eq!(
            lines.len(),
            oracle["counts"]["physical_lines"].as_u64().unwrap() as usize
        );
        let long = PARAGRAPH.repeat(LONG_REPEATS);
        assert_eq!(
            long.encode_utf16().count(),
            oracle["counts"]["long_line_characters"].as_u64().unwrap() as usize
        );
        assert!(first_frame(bootstrap("cjk-wrap-518-first-frame", &lines).unwrap()).unwrap() > 0.0);
        assert!(
            first_frame(bootstrap("cjk-wrap-single-long-line", &[long]).unwrap()).unwrap() > 0.0
        );
        let burst = wheel_burst(bootstrap("cjk-wrap-518-wheel-burst", &lines).unwrap()).unwrap();
        for (index, name) in [
            (2, "initial_content_rows"),
            (3, "immediate_content_rows"),
            (4, "settled_content_rows"),
        ] {
            assert_eq!(burst[index], oracle["counts"][name].as_f64().unwrap());
        }
        for run in oracle["runs"].as_array().unwrap() {
            assert_eq!(run["exitCode"], 0);
            let metrics = runner::parse_metrics(run["combinedOutput"].as_str().unwrap());
            assert_eq!(metrics.len(), 10);
            for (name, expected) in [
                ("physical_lines", PHYSICAL_LINES),
                ("long_line_characters", 8736),
                ("wheel_burst_events", BURST_EVENTS),
            ] {
                assert_eq!(
                    metrics.iter().find(|(key, _)| key == name).unwrap().1,
                    expected as f64
                );
            }
        }
        assert!(run(["extra".into()].into_iter()).is_err());
    }

    #[test]
    fn explicit_highlight_prefetch_does_not_navigate_or_render() {
        let setup = Renderer::new(bootstrap("prefetch-probe", &issue_lines()).unwrap());
        let selection = setup.app.shared_state().lock().unwrap().selection();
        let buffer = setup.buffer.clone();
        assert!(setup.app.prefetch_file_highlights(0));
        assert!(setup.app.prefetch_file_highlights(0));
        assert!(!setup.app.prefetch_file_highlights(1));
        assert_eq!(
            setup.app.shared_state().lock().unwrap().selection(),
            selection
        );
        assert_eq!(setup.buffer, buffer);
    }
}
