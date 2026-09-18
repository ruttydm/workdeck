use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Paragraph, Widget},
};
use workdeck_tui::{
    WorkdeckDiffBodyOptions, WorkdeckDiffFile, WorkdeckDiffFileHeaderOptions, WorkdeckDiffLayout,
    WorkdeckDiffSelection, WorkdeckFileNavOptions, WorkdeckFileNavRenderMap,
    WorkdeckReviewStreamOptions, create_workdeck_diff_files_from_patch, fit_text, pad_text,
    render_workdeck_diff_body, render_workdeck_diff_file_header, render_workdeck_file_nav,
    render_workdeck_review_stream, workdeck_file_nav_selection_at,
};

const PATCH: &str = r#"diff --git a/src/search.rs b/src/search.rs
--- a/src/search.rs
+++ b/src/search.rs
@@ -1,16 +1,34 @@
 pub struct Command {
     pub id: String,
     pub label: String,
     pub keywords: Vec<String>,
 }

 pub fn search_commands<'a>(commands: &'a [Command], query: &str) -> Vec<&'a Command> {
-    let needle = query.trim().to_lowercase();
-    commands
+    let needle = normalize_query(query);
+    if needle.is_empty() {
+        return commands.iter().collect();
+    }
+
+    let mut matches = commands
         .iter()
-        .filter(|command| command.label.to_lowercase().contains(&needle))
-        .collect()
+        .filter_map(|command| {
+            let score = score_command(command, &needle);
+            (score > 0).then_some((command, score))
+        })
+        .collect::<Vec<_>>();
+    matches.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
+    matches.into_iter().map(|(command, _)| command).collect()
+}
+
+fn normalize_query(value: &str) -> String {
+    value.trim().to_lowercase()
+}
+
+fn score_command(command: &Command, needle: &str) -> u8 {
+    if command.label.to_lowercase().starts_with(needle) {
+        3
+    } else {
+        0
+    }
 }
diff --git a/src/commands.rs b/src/commands.rs
--- a/src/commands.rs
+++ b/src/commands.rs
@@ -22,7 +22,13 @@ pub fn commands() -> Vec<Command> {
         Command {
             id: "open-help".into(),
             label: "Open help".into(),
-            keywords: vec!["keyboard".into(), "shortcuts".into()],
+            keywords: vec!["keyboard".into(), "shortcuts".into(), "short cuts".into()],
+        },
+        Command {
+            id: "copy-command".into(),
+            label: "Copy command id".into(),
+            keywords: vec!["clipboard".into(), "copy".into()],
         },
     ]
 }
"#;

const OUTER_BACKGROUND: Color = Color::Rgb(0x08, 0x11, 0x1f);
const HEADER_BACKGROUND: Color = Color::Rgb(0x13, 0x24, 0x3a);
const HEADER_FOREGROUND: Color = Color::Rgb(0xee, 0xf4, 0xff);
const FRAME_BORDER: Color = Color::Rgb(0x28, 0x42, 0x64);
const FRAME_BACKGROUND: Color = Color::Rgb(0x0e, 0x1b, 0x2e);
const FRAME_TITLE: Color = Color::Rgb(0x7f, 0xd1, 0xff);
const EMPTY_TEXT: Color = Color::Rgb(0x8d, 0xa5, 0xc7);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoKey {
    Character(char),
    Escape,
    Tab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoAction {
    Continue,
    Quit,
}

#[derive(Debug, Clone)]
pub struct PrimitivesRenderMap {
    pub file_nav: WorkdeckFileNavRenderMap,
}

#[derive(Debug, Clone)]
pub struct PrimitivesDemoApp {
    files: Vec<WorkdeckDiffFile>,
    layout: WorkdeckDiffLayout,
    selected_file_id: String,
}

impl PrimitivesDemoApp {
    pub fn new() -> Result<Self, String> {
        let files = create_workdeck_diff_files_from_patch(PATCH, "primitives-demo")
            .map_err(|error| error.to_string())?;
        let selected_file_id = files
            .first()
            .map(|file| file_id(file).to_owned())
            .unwrap_or_default();
        Ok(Self {
            files,
            layout: WorkdeckDiffLayout::Split,
            selected_file_id,
        })
    }

    #[must_use]
    pub const fn layout(&self) -> WorkdeckDiffLayout {
        self.layout
    }

    #[must_use]
    pub fn selected_file_id(&self) -> &str {
        &self.selected_file_id
    }

    pub fn handle_key(&mut self, key: DemoKey) -> DemoAction {
        match key {
            DemoKey::Character('q') | DemoKey::Escape => DemoAction::Quit,
            DemoKey::Character('1') => {
                self.layout = WorkdeckDiffLayout::Split;
                DemoAction::Continue
            }
            DemoKey::Character('2') => {
                self.layout = WorkdeckDiffLayout::Stack;
                DemoAction::Continue
            }
            DemoKey::Tab if self.files.len() > 1 => {
                let current = self
                    .files
                    .iter()
                    .position(|file| file_id(file) == self.selected_file_id)
                    .unwrap_or_default();
                let next = (current + 1) % self.files.len();
                self.selected_file_id = file_id(&self.files[next]).to_owned();
                DemoAction::Continue
            }
            DemoKey::Tab | DemoKey::Character(_) => DemoAction::Continue,
        }
    }

    pub fn select_file_at(&mut self, row: u16, map: &PrimitivesRenderMap) -> bool {
        let Some(file_id) = workdeck_file_nav_selection_at(&map.file_nav, row) else {
            return false;
        };
        self.selected_file_id = file_id.to_owned();
        true
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) -> PrimitivesRenderMap {
        Block::new()
            .style(Style::default().bg(OUTER_BACKGROUND))
            .render(area, buffer);
        let width = area.width.saturating_sub(2);
        let header = Rect::new(area.x.saturating_add(1), area.y.saturating_add(1), width, 1);
        Paragraph::new(pad_text(
            &fit_text(
                " Workdeck primitives as app windows — q quit · Tab next file · 1 split · 2 stack ",
                usize::from(width).max(1),
                None,
            ),
            usize::from(width).max(1),
        ))
        .style(Style::default().fg(HEADER_FOREGROUND).bg(HEADER_BACKGROUND))
        .render(header, buffer);

        let content = Rect::new(
            header.x,
            header.y.saturating_add(2),
            width,
            area.height.saturating_sub(4),
        );
        let desired_sidebar = ((u32::from(width) * 28) / 100).clamp(24, 34) as u16;
        let sidebar_width = desired_sidebar.min(content.width);
        let gap = u16::from(content.width > sidebar_width);
        let main_width = content.width.saturating_sub(sidebar_width + gap);
        let sidebar = Rect::new(content.x, content.y, sidebar_width, content.height);
        let main = Rect::new(
            content.x.saturating_add(sidebar_width + gap),
            content.y,
            main_width,
            content.height,
        );

        let nav_body = render_window_frame(sidebar, buffer, "WorkdeckFileNav");
        let file_nav = render_workdeck_file_nav(
            nav_body,
            buffer,
            &self.files,
            &WorkdeckFileNavOptions {
                selected_file_id: Some(self.selected_file_id.clone()),
                theme: "midnight".into(),
            },
        );

        let header_height = 4.min(main.height);
        let remaining = main.height.saturating_sub(header_height + gap);
        let body_height = ((u32::from(remaining) * 52) / 100) as u16;
        let stream_height = remaining.saturating_sub(body_height + gap);
        let header_window = Rect::new(main.x, main.y, main.width, header_height);
        let body_window = Rect::new(
            main.x,
            main.y.saturating_add(header_height + gap),
            main.width,
            body_height,
        );
        let stream_window = Rect::new(
            main.x,
            body_window.y.saturating_add(body_height + gap),
            main.width,
            stream_height,
        );

        let selected = self
            .files
            .iter()
            .find(|file| file_id(file) == self.selected_file_id)
            .or_else(|| self.files.first());
        let header_body = render_window_frame(header_window, buffer, "WorkdeckDiffFileHeader");
        if let Some(file) = selected {
            render_workdeck_diff_file_header(
                header_body,
                buffer,
                file,
                &WorkdeckDiffFileHeaderOptions {
                    theme: "midnight".into(),
                    selected: true,
                },
            );
        } else {
            Paragraph::new("No file selected.")
                .style(Style::default().fg(EMPTY_TEXT))
                .render(header_body, buffer);
        }

        let layout_name = match self.layout {
            WorkdeckDiffLayout::Stack => "stack",
            WorkdeckDiffLayout::Auto | WorkdeckDiffLayout::Split => "split",
        };
        let body_title = format!("WorkdeckDiffBody ({layout_name})");
        let body = render_window_frame(body_window, buffer, &body_title);
        render_workdeck_diff_body(
            body,
            buffer,
            selected,
            &WorkdeckDiffBodyOptions {
                layout: self.layout,
                theme: "midnight".into(),
                ..WorkdeckDiffBodyOptions::default()
            },
        );

        let stream = render_window_frame(stream_window, buffer, "WorkdeckReviewStream");
        render_workdeck_review_stream(
            stream,
            buffer,
            &self.files,
            &WorkdeckReviewStreamOptions {
                body: WorkdeckDiffBodyOptions {
                    layout: self.layout,
                    theme: "midnight".into(),
                    ..WorkdeckDiffBodyOptions::default()
                },
                selection: selected.map(|file| WorkdeckDiffSelection {
                    file_id: file_id(file).to_owned(),
                    hunk_index: 0,
                }),
                show_file_separators: false,
                ..WorkdeckReviewStreamOptions::default()
            },
        );

        PrimitivesRenderMap { file_nav }
    }
}

fn file_id(file: &WorkdeckDiffFile) -> &str {
    if file.runtime_id.is_empty() {
        &file.key
    } else {
        &file.runtime_id
    }
}

fn render_window_frame(area: Rect, buffer: &mut Buffer, title: &str) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }
    let block = Block::bordered()
        .border_style(Style::default().fg(FRAME_BORDER))
        .style(Style::default().bg(FRAME_BACKGROUND));
    let inner = block.inner(area);
    block.render(area, buffer);
    let title_row = Rect::new(inner.x, inner.y, inner.width, u16::from(inner.height > 0));
    Paragraph::new(fit_text(
        &format!(" {title} "),
        usize::from(inner.width).max(1),
        None,
    ))
    .style(Style::default().fg(FRAME_TITLE))
    .render(title_row, buffer);
    Rect::new(
        inner.x.saturating_add(1),
        inner.y.saturating_add(1),
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_all_primitives_and_keeps_navigation_in_the_host() {
        let mut app = PrimitivesDemoApp::new().expect("translated patch parses");
        let area = Rect::new(0, 0, 140, 42);
        let mut buffer = Buffer::empty(area);
        let map = app.render(area, &mut buffer);
        let initial_file = app.selected_file_id().to_owned();

        assert_eq!(
            app.handle_key(DemoKey::Character('2')),
            DemoAction::Continue
        );
        assert_eq!(app.layout(), WorkdeckDiffLayout::Stack);
        assert_eq!(app.handle_key(DemoKey::Tab), DemoAction::Continue);
        assert_ne!(app.selected_file_id(), initial_file);
        let first_file_row = map
            .file_nav
            .file_rows
            .first()
            .expect("navigator exposes the first file row")
            .row;
        assert!(app.select_file_at(first_file_row, &map));
        assert_eq!(app.selected_file_id(), initial_file);
        assert_eq!(app.handle_key(DemoKey::Character('q')), DemoAction::Quit);

        let contents = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(contents.contains("Workdeck primitives as app windows"));
        assert!(contents.contains("WorkdeckFileNav"));
        assert!(contents.contains("WorkdeckDiffFileHeader"));
        assert!(contents.contains("WorkdeckDiffBody (split)"));
        assert!(contents.contains("WorkdeckReviewStream"));
        assert!(contents.contains("src/search.rs"));
    }
}
