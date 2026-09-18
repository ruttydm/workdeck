use super::tasks::{Task, TaskState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupedTasks<'a> {
    pub shipping_today: Vec<&'a Task>,
    pub needs_help: Vec<&'a Task>,
}

#[must_use]
pub fn group_tasks(tasks: &[Task]) -> GroupedTasks<'_> {
    GroupedTasks {
        shipping_today: tasks
            .iter()
            .filter(|task| matches!(task.state, TaskState::Done | TaskState::Doing))
            .collect(),
        needs_help: tasks.iter().filter(|task| task.blocked).collect(),
    }
}
