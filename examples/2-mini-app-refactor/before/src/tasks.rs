#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Todo,
    Doing,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Task {
    pub id: &'static str,
    pub title: &'static str,
    pub owner: &'static str,
    pub state: TaskState,
    pub blocked: bool,
}

pub const TASKS: &[Task] = &[
    Task {
        id: "T-101",
        title: "Review onboarding copy",
        owner: "Maya",
        state: TaskState::Done,
        blocked: false,
    },
    Task {
        id: "T-102",
        title: "Polish dashboard empty state",
        owner: "Lee",
        state: TaskState::Doing,
        blocked: false,
    },
    Task {
        id: "T-103",
        title: "Document keyboard shortcuts",
        owner: "Sam",
        state: TaskState::Todo,
        blocked: true,
    },
];
