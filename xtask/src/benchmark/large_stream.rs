//! MIT translation of Hunk's large-stream first-frame and four-wheel-tick workload.

use super::*;
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{buffer::Buffer, layout::Rect};
use std::time::{Duration, Instant};
use workdeck_review::LayoutMode;
use workdeck_tui::{ReviewApp, ReviewOptions, VIEWPORT_READ_COALESCE_MS, render, resolve_theme};

pub(super) const VIEWPORT: Rect = Rect::new(0, 0, 240, 28);
const SCROLL_TICKS: usize = 4;
const SCROLL_TARGET: (u16, u16) = (170, 12);
const SELECTED_HIGHLIGHT_MARKER: &str = "stream1_40";

pub(super) struct Renderer {
    pub(super) app: ReviewApp,
    pub(super) buffer: Buffer,
    viewport: Rect,
}

impl Renderer {
    fn new() -> Result<Self> {
        Self::with_content(
            stream::DEFAULT_FILE_COUNT,
            stream::DEFAULT_LINES_PER_FILE,
            false,
        )
    }

    pub(super) fn with_content(files: usize, lines: usize, non_ascii: bool) -> Result<Self> {
        let bootstrap =
            stream::large_bootstrap(std::env::current_dir()?, files, lines, 37, 84, non_ascii)?;
        Ok(Self::from_bootstrap(bootstrap))
    }

    pub(super) fn from_bootstrap(bootstrap: workdeck_core::AppBootstrap) -> Self {
        Self::from_bootstrap_at_viewport(bootstrap, VIEWPORT)
    }

    pub(super) fn from_bootstrap_at_viewport(
        bootstrap: workdeck_core::AppBootstrap,
        viewport: Rect,
    ) -> Self {
        let app = ReviewApp::new(
            bootstrap.changeset,
            ReviewOptions {
                layout: LayoutMode::Split,
                theme: resolve_theme(bootstrap.initial_theme.as_deref(), None, &[]),
                command_cwd: Some(bootstrap.reload_context.cwd),
                review_input: Some(bootstrap.input),
                ..ReviewOptions::default()
            },
        );
        Self {
            app,
            buffer: Buffer::empty(viewport),
            viewport,
        }
    }

    pub(super) fn render_pass(&mut self, passes: usize) {
        for _ in 0..passes {
            self.buffer.reset();
            render(self.viewport, &mut self.buffer, &self.app);
        }
    }

    pub(super) fn resize(&mut self, width: u16, height: u16) {
        self.viewport = Rect::new(0, 0, width, height);
        self.buffer = Buffer::empty(self.viewport);
    }

    fn flush_selected_highlight(&mut self) -> bool {
        for _ in 0..200 {
            self.render_pass(1);
            if super::highlight_prefetch::highlighted_marker(
                &self.buffer,
                SELECTED_HIGHLIGHT_MARKER,
            ) {
                return true;
            }
        }
        false
    }
}

fn first_frame() -> Result<(f64, bool)> {
    let mut setup = Renderer::new()?;
    let start = Instant::now();
    setup.render_pass(1);
    let duration = start.elapsed().as_secs_f64() * 1000.0;
    // Source settles highlighting outside the measured interval before retiring each app.
    let ready = setup.flush_selected_highlight();
    Ok((duration, ready))
}

fn scroll_ticks() -> Result<(f64, usize)> {
    let mut setup = Renderer::new()?;
    setup.render_pass(2);
    let start = Instant::now();
    for _ in 0..SCROLL_TICKS {
        setup.app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: SCROLL_TARGET.0,
            row: SCROLL_TARGET.1,
            modifiers: KeyModifiers::NONE,
        });
        std::thread::sleep(Duration::from_millis(VIEWPORT_READ_COALESCE_MS + 1));
        setup.render_pass(1);
    }
    let duration = start.elapsed().as_secs_f64() * 1000.0;
    let scroll = setup.app.review_scroll();
    setup.flush_selected_highlight();
    Ok((duration, scroll))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark large-stream accepts no arguments");
    }
    let (cold, _) = first_frame()?;
    let (warm, _) = first_frame()?;
    let (scroll, _) = scroll_ticks()?;
    for (name, value) in [
        ("cold_first_frame_ms", cold),
        ("warm_first_frame_ms", warm),
        ("windowed_scroll_ticks_ms", scroll),
    ] {
        println!("METRIC {name}={}", fixed(value, 2));
    }
    println!("METRIC scroll_ticks={SCROLL_TICKS}");
    println!("METRIC files={}", stream::DEFAULT_FILE_COUNT);
    println!("METRIC lines_per_file={}", stream::DEFAULT_LINES_PER_FILE);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_stream_renders_highlighted_first_frames_and_real_wheel_scrolls() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-large-stream.json"
        ))
        .unwrap();
        assert_eq!(
            u64::from(VIEWPORT.width),
            oracle["viewport"]["width"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(VIEWPORT.height),
            oracle["viewport"]["height"].as_u64().unwrap()
        );
        assert_eq!(
            SCROLL_TICKS as u64,
            oracle["counts"]["scroll_ticks"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(SCROLL_TARGET.0),
            oracle["scrollTarget"]["x"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(SCROLL_TARGET.1),
            oracle["scrollTarget"]["y"].as_u64().unwrap()
        );
        assert_eq!(
            stream::DEFAULT_FILE_COUNT as u64,
            oracle["counts"]["files"].as_u64().unwrap()
        );
        assert_eq!(
            stream::DEFAULT_LINES_PER_FILE as u64,
            oracle["counts"]["lines_per_file"].as_u64().unwrap()
        );
        let (cold, cold_ready) = first_frame().unwrap();
        let (warm, warm_ready) = first_frame().unwrap();
        let (scroll, offset) = scroll_ticks().unwrap();
        assert!(cold.is_finite() && cold > 0.0 && warm.is_finite() && warm > 0.0);
        assert!(cold_ready && warm_ready);
        assert!(
            scroll.is_finite()
                && scroll >= (SCROLL_TICKS as f64 * (VIEWPORT_READ_COALESCE_MS + 1) as f64)
        );
        assert!(offset > 0, "wheel events must move the production viewport");
        assert!(run(["extra".into()].into_iter()).is_err());
    }
}
