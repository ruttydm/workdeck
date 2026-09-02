#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub keywords: &'static [&'static str],
}

#[must_use]
pub fn search_commands<'a>(query: &str, commands: &'a [Command]) -> Vec<&'a Command> {
    let needle = query.trim().to_lowercase();

    if needle.is_empty() {
        return commands.iter().collect();
    }

    commands
        .iter()
        .filter(|command| {
            std::iter::once(command.label)
                .chain(command.keywords.iter().copied())
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
                .contains(&needle)
        })
        .collect()
}
