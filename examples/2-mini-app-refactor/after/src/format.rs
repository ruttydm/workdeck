use super::tasks::{Task, TaskState};

fn status_chip(task: &Task) -> &'static str {
    if task.blocked {
        return "[blocked]";
    }

    if task.state == TaskState::Done {
        return "[done]";
    }

    if task.state == TaskState::Doing {
        "[active]"
    } else {
        "[queued]"
    }
}

#[must_use]
pub fn format_task(task: &Task) -> String {
    format!("{} {} — {}", status_chip(task), task.title, task.owner)
}
