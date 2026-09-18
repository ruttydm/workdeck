use workdeck_tui::{WorkdeckDiffLayout, create_workdeck_diff_files_from_patch};

use super::ratatui_component_support::{ExampleApp, ExampleProps, read_example_file};

pub fn example_from_patch() -> Result<ExampleApp, String> {
    let patch = read_example_file("change.patch").map_err(|error| error.to_string())?;
    let diff = create_workdeck_diff_files_from_patch(&patch, "example:patch")
        .map_err(|error| error.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "expected one diff file in the component example patch".to_owned())?;

    Ok(ExampleApp::new(ExampleProps {
        title: "Workdeck diff view from patch text".into(),
        subtitle: "Built from unified patch text. Host app owns input and exit.".into(),
        diff,
        layout: WorkdeckDiffLayout::Split,
    }))
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect};

    use super::*;

    #[test]
    fn renders_from_patch_text_and_supports_host_owned_scrolling() {
        let mut app = example_from_patch().expect("patch example builds");
        app.set_vertical_offset(1);
        let area = Rect::new(0, 0, 96, 24);
        let mut buffer = Buffer::empty(area);
        app.render(area, &mut buffer);
        let contents = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(contents.contains("Workdeck diff view from patch text"));
        assert!(contents.contains("tags"));
    }
}
