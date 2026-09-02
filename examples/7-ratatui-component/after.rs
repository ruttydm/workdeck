#[derive(Debug, Clone, PartialEq)]
pub struct ReviewSummary {
    pub title: String,
    pub confidence: f64,
    pub tags: Vec<String>,
}

#[must_use]
pub fn format_review_summary(summary: &ReviewSummary) -> String {
    let tag_suffix = if summary.tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", summary.tags.join(", "))
    };
    format!("{} ({}){tag_suffix}", summary.title, summary.confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_optional_review_tags() {
        assert_eq!(
            format_review_summary(&ReviewSummary {
                title: "Ready".into(),
                confidence: 0.9,
                tags: vec!["safe".into(), "tested".into()],
            }),
            "Ready (0.9) [safe, tested]"
        );
        assert_eq!(
            format_review_summary(&ReviewSummary {
                title: "Pending".into(),
                confidence: 0.5,
                tags: Vec::new(),
            }),
            "Pending (0.5)"
        );
    }
}
