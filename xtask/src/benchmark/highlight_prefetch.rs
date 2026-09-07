//! MIT translation of Hunk's highlighting-readiness workload using the production Ratatui canvas.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};
use std::time::Instant;
use workdeck_core::{Changeset, ChangesetSource, DiffFile};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::LayoutMode;
use workdeck_tui::{ReviewApp, ReviewOptions, render, resolve_theme};

fn file(index: usize, marker: &str) -> Result<DiffFile> {
    let path = format!("src/example{index}.ts");
    let before = format!(
        "export const {marker} = {index};\nexport function keep{index}(value: number) {{ return value + {index}; }}\nexport const tail{index} = true;\n"
    );
    let next = index + 1;
    let after = format!(
        "export const {marker} = {next};\nexport function keep{index}(value: number) {{ return value * {next}; }}\nexport const tail{index} = true;\n"
    );
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            name: &path,
            contents: &before,
            cache_key: &format!("prefetch:{index}:before"),
        },
        FileSnapshot {
            name: &path,
            contents: &after,
            cache_key: &format!("prefetch:{index}:after"),
        },
        FileComparisonOptions { context_radius: 3 },
    )?;
    file.runtime_id = format!("prefetch:{index}");
    file.patch.clear();
    file.language = Some("typescript".into());
    file.stats.additions = 2;
    file.stats.deletions = 2;
    file.refresh_identity();
    Ok(file)
}

fn app() -> Result<ReviewApp> {
    let files = ["alphaMarker", "betaMarker", "gammaMarker", "deltaMarker"]
        .into_iter()
        .enumerate()
        .map(|(index, marker)| file(index + 1, marker))
        .collect::<Result<Vec<_>>>()?;
    let mut changeset = Changeset {
        id: "changeset:prefetch-benchmark".into(),
        source_label: "repo".into(),
        title: "repo working tree".into(),
        summary: None,
        agent_summary: None,
        source: ChangesetSource::WorkingTree { staged: false },
        files,
    };
    changeset.refresh_review_identities();
    Ok(ReviewApp::new(
        changeset,
        ReviewOptions {
            layout: LayoutMode::Split,
            theme: resolve_theme(Some("midnight"), None, &[]),
            command_cwd: Some(std::env::current_dir()?),
            ..ReviewOptions::default()
        },
    ))
}

/// Reconstruct contiguous styled spans from the authoritative cells, then apply the source's
/// marker-segmentation heuristic. Never infer highlighting from raw text presence alone.
fn highlighted_marker(frame: &Buffer, marker: &str) -> bool {
    frame
        .content
        .chunks(usize::from(frame.area.width))
        .any(|row| {
            let mut spans: Vec<String> = Vec::new();
            let mut previous = None;
            for cell in row {
                let style = cell.style();
                if previous != Some(style) {
                    spans.push(String::new());
                    previous = Some(style);
                }
                spans.last_mut().unwrap().push_str(cell.symbol());
            }
            let text = spans.concat();
            text.contains(marker)
                && spans.iter().any(|span| {
                    span.contains(marker)
                        && crate::release_channel::trim_source_whitespace(span)
                            .encode_utf16()
                            .count()
                            < crate::release_channel::trim_source_whitespace(&text)
                                .encode_utf16()
                                .count()
                })
        })
}

fn frame(app: &ReviewApp, buffer: &mut Buffer) {
    buffer.reset();
    render(buffer.area, buffer, app);
    std::thread::yield_now();
}

struct Measurement {
    selected_startup_ms: f64,
    next_file_ready_ms: f64,
    adjacent_ready_before_move: bool,
    iterations: usize,
}

fn measure() -> Result<Measurement> {
    let mut app = app()?;
    let mut buffer = Buffer::empty(Rect::new(0, 0, 240, 24));
    let start = Instant::now();
    let mut result = Measurement {
        selected_startup_ms: 0.0,
        next_file_ready_ms: 0.0,
        adjacent_ready_before_move: false,
        iterations: 0,
    };
    while result.iterations < 400 {
        result.iterations += 1;
        frame(&app, &mut buffer);
        if highlighted_marker(&buffer, "alphaMarker") {
            result.selected_startup_ms = start.elapsed().as_secs_f64() * 1000.0;
            result.adjacent_ready_before_move = highlighted_marker(&buffer, "betaMarker");
            break;
        }
    }
    frame(&app, &mut buffer);
    frame(&app, &mut buffer);
    let start = Instant::now();
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    frame(&app, &mut buffer);
    while result.iterations < 800 {
        result.iterations += 1;
        frame(&app, &mut buffer);
        if highlighted_marker(&buffer, "betaMarker") {
            result.next_file_ready_ms = start.elapsed().as_secs_f64() * 1000.0;
            break;
        }
    }
    // Dropping the production app retires owned extension and highlighting resources.
    Ok(result)
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark highlight-prefetch accepts no arguments");
    }
    let result = measure()?;
    println!(
        "METRIC selected_startup_ms={}",
        fixed(result.selected_startup_ms, 2)
    );
    println!(
        "METRIC next_file_ready_ms={}",
        fixed(result.next_file_ready_ms, 2)
    );
    println!(
        "METRIC adjacent_ready_before_move={}",
        usize::from(result.adjacent_ready_before_move)
    );
    println!("METRIC files=4");
    println!("METRIC iterations={}", result.iterations);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};

    #[test]
    fn cell_marker_detection_requires_source_style_segmentation() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
        buffer.set_string(0, 0, "export const alphaMarker = 1;", Style::default());
        assert!(!highlighted_marker(&buffer, "alphaMarker"));
        buffer.set_string(0, 0, "export", Style::default().fg(Color::Red));
        assert!(highlighted_marker(&buffer, "alphaMarker"));
        assert!(!highlighted_marker(&buffer, "betaMarker"));
    }

    #[test]
    fn production_canvas_reaches_selected_and_adjacent_highlight_readiness() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-highlight-prefetch.json"
        ))
        .unwrap();
        assert_eq!(oracle["baselineMetrics"]["files"], 4);
        assert_eq!(oracle["stableMetrics"]["files"], 4);
        for expected in oracle["files"].as_array().unwrap() {
            let index = expected["index"].as_u64().unwrap() as usize;
            let file = file(index, expected["marker"].as_str().unwrap()).unwrap();
            assert_eq!(file.runtime_id, format!("prefetch:{index}"));
            assert_eq!(file.path, format!("src/example{index}.ts"));
            assert_eq!(file.language.as_deref(), Some("typescript"));
            assert_eq!(
                file.sources.old.as_ref().unwrap().content,
                expected["before"].as_str().unwrap()
            );
            assert_eq!(
                file.sources.new.as_ref().unwrap().content,
                expected["after"].as_str().unwrap()
            );
            assert_eq!(
                file.split_row_count as u64,
                oracle["geometry"]["splitRows"].as_u64().unwrap()
            );
            assert_eq!(
                file.stack_row_count as u64,
                oracle["geometry"]["stackRows"].as_u64().unwrap()
            );
            assert_eq!(file.hunks.len(), 1);
            assert_eq!(
                (
                    file.hunks[0].old_start,
                    file.hunks[0].old_count,
                    file.hunks[0].new_start,
                    file.hunks[0].new_count
                ),
                (1, 3, 1, 3)
            );
            assert!(!file.flags.partial);
            assert!(file.patch.is_empty() && file.agent.is_none());
            assert_eq!((file.stats.additions, file.stats.deletions), (2, 2));
        }
        let mut app = app().unwrap();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 240, 24));
        frame(&app, &mut buffer);
        assert_eq!(app.shared_state().lock().unwrap().selection().file_index, 0);
        let before_selection = app.shared_state().lock().unwrap().selection();
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        frame(&app, &mut buffer);
        // The pinned command catalog binds Down to stepDown (one review row), not nextFile.
        // Preserve the workload's actual input despite its "next file" timing label.
        assert_eq!(app.shared_state().lock().unwrap().selection().file_index, 0);
        // Split navigation can advance sides on the same painted row.
        assert_ne!(
            app.shared_state().lock().unwrap().selection(),
            before_selection
        );
        let result = measure().unwrap();
        assert!(result.selected_startup_ms > 0.0);
        assert!(result.next_file_ready_ms > 0.0);
        assert!(result.adjacent_ready_before_move);
        assert!(result.iterations >= 2 && result.iterations < 800);
        assert!(run(["extra".into()].into_iter()).is_err());
    }
}
