mod format;
mod tasks;

use format::render_task_line;
use tasks::TASKS;

#[must_use]
pub fn render_morning_summary() -> String {
    std::iter::once("Morning summary".to_owned())
        .chain(TASKS.iter().map(render_task_line))
        .collect::<Vec<_>>()
        .join("\n")
}
