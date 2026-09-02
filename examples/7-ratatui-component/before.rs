#[derive(Debug, Clone, PartialEq)]
pub struct ReviewSummary {
    pub title: String,
    pub confidence: f64,
}

#[must_use]
pub fn summarize_review(summary: &ReviewSummary) -> String {
    format!("{} ({})", summary.title, summary.confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_title_and_confidence() {
        assert_eq!(
            summarize_review(&ReviewSummary {
                title: "Ready".into(),
                confidence: 0.9,
            }),
            "Ready (0.9)"
        );
    }
}
