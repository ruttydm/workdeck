use super::normalize::normalize_query;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub label: &'static str,
    pub keywords: &'static [&'static str],
}

#[must_use]
pub fn search_commands<'a>(query: &str, commands: &'a [Command]) -> Vec<&'a Command> {
    let needle = normalize_query(query);

    if needle.is_empty() {
        return commands.iter().collect();
    }

    let mut scored = commands
        .iter()
        .filter_map(|command| {
            let label = normalize_query(command.label);
            let keywords = command
                .keywords
                .iter()
                .map(|keyword| normalize_query(keyword))
                .collect::<Vec<_>>();

            let mut score = 0;
            if label.starts_with(&needle) {
                score += 4;
            }
            if label.contains(&needle) {
                score += 2;
            }
            if keywords.iter().any(|keyword| keyword == &needle) {
                score += 3;
            }
            if keywords.iter().any(|keyword| keyword.contains(&needle)) {
                score += 1;
            }

            (score > 0).then_some((command, score))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.label.cmp(right.label))
    });
    scored.into_iter().map(|(command, _)| command).collect()
}
