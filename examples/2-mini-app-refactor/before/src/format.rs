use super::tasks::{Task, TaskState};

#[must_use]
pub fn render_task_line(task: &Task) -> String {
    let marker = match task.state {
        TaskState::Done => "✓",
        TaskState::Doing => "•",
        TaskState::Todo => "○",
    };
    let blocked = if task.blocked { " (blocked)" } else { "" };

    format!("{marker} {} — {}{blocked}", task.title, task.owner)
}
