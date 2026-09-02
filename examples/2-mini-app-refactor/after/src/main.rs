mod format;
mod group_tasks;
mod tasks;

use format::format_task;
use group_tasks::group_tasks;
use tasks::TASKS;

#[must_use]
pub fn render_morning_summary() -> String {
    let grouped = group_tasks(TASKS);

    std::iter::once("Morning summary".to_owned())
        .chain(std::iter::once(String::new()))
        .chain(std::iter::once("Shipping today".to_owned()))
        .chain(grouped.shipping_today.into_iter().map(format_task))
        .chain(std::iter::once(String::new()))
        .chain(std::iter::once("Needs help".to_owned()))
        .chain(grouped.needs_help.into_iter().map(format_task))
        .collect::<Vec<_>>()
        .join("\n")
}
