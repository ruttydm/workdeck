mod commands;
mod normalize;
mod search;

use commands::COMMANDS;
use search::search_commands;

#[must_use]
pub fn render_command_preview(query: &str) -> String {
    search_commands(query, COMMANDS)
        .into_iter()
        .take(3)
        .map(|command| format!("• {}", command.label))
        .collect::<Vec<_>>()
        .join("\n")
}
