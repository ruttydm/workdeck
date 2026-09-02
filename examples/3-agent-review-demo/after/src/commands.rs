use super::search::Command;

pub const COMMANDS: &[Command] = &[
    Command {
        id: "open-workspace",
        label: "Open workspace",
        keywords: &["project", "folder"],
    },
    Command {
        id: "toggle-sidebar",
        label: "Toggle sidebar",
        keywords: &["files", "panel"],
    },
    Command {
        id: "next-hunk",
        label: "Next hunk",
        keywords: &["jump", "change"],
    },
    Command {
        id: "open-help",
        label: "Open help",
        keywords: &["keyboard", "shortcuts", "short cuts"],
    },
];
