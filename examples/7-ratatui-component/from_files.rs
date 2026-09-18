use workdeck_tui::{
    WorkdeckDiffLayout, WorkdeckFileComparisonOptions, WorkdeckFileSnapshot,
    diff_from_workdeck_file_snapshots,
};

use super::ratatui_component_support::{ExampleApp, ExampleProps, read_example_file};

const PATH: &str = "src/review_summary.rs";

pub fn example_from_files() -> Result<ExampleApp, String> {
    let before = read_example_file("before.rs").map_err(|error| error.to_string())?;
    let after = read_example_file("after.rs").map_err(|error| error.to_string())?;
    let diff = diff_from_workdeck_file_snapshots(
        WorkdeckFileSnapshot {
            cache_key: "example:before",
            contents: &before,
            name: PATH,
        },
        WorkdeckFileSnapshot {
            cache_key: "example:after",
            contents: &after,
            name: PATH,
        },
        WorkdeckFileComparisonOptions { context_radius: 3 },
    )
    .map_err(|error| error.to_string())?;

    Ok(ExampleApp::new(ExampleProps {
        title: "Workdeck diff view from file contents".into(),
        subtitle: "Built from file snapshots. Host app owns input and exit.".into(),
        diff,
        layout: WorkdeckDiffLayout::Split,
    }))
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect};

    use super::*;

    #[test]
    fn renders_from_file_contents_and_switches_layout_from_host_controls() {
        let mut app = example_from_files().expect("file example builds");
        let area = Rect::new(0, 0, 96, 24);
        let mut buffer = Buffer::empty(area);
        let map = app.render(area, &mut buffer);
        assert!(app.select_at(map.stack_button.x, map.stack_button.y, map));
        assert_eq!(app.active_layout(), WorkdeckDiffLayout::Stack);

        let mut stacked = Buffer::empty(area);
        app.render(area, &mut stacked);
        let contents = stacked
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(contents.contains("Workdeck diff view from file contents"));
        assert!(contents.contains("format_review_summary"));
    }
}
