/// Build the stable terminal-local id used for sidebar file rows.
#[must_use]
pub fn file_row_id(file_id: &str) -> String {
    format!("file-row:{file_id}")
}

/// Build the stable id for a file section in the main review stream.
#[must_use]
pub fn diff_section_id(file_id: &str) -> String {
    format!("diff-section:{file_id}")
}

/// Build the stable id for a hunk anchor in the main review stream.
#[must_use]
pub fn diff_hunk_id(file_id: &str, hunk_index: usize) -> String {
    format!("diff-hunk:{file_id}:{hunk_index}")
}

/// Build the stable id for one presentational row in the review stream.
#[must_use]
pub fn review_row_id(row_key: &str) -> String {
    format!("review-row:{row_key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_review_ids_keep_their_exact_namespaces() {
        assert_eq!(file_row_id("file:1"), "file-row:file:1");
        assert_eq!(diff_section_id("file:1"), "diff-section:file:1");
        assert_eq!(diff_hunk_id("file:1", 2), "diff-hunk:file:1:2");
        assert_eq!(review_row_id("line:2:new:8"), "review-row:line:2:new:8");
    }
}
