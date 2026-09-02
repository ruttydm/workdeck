#[must_use]
pub fn review_button_label(file_count: usize) -> String {
    if file_count == 1 {
        "Review 1 file".to_owned()
    } else {
        format!("Review {file_count} files")
    }
}

#[must_use]
pub fn review_timestamp_label(last_updated: &str) -> String {
    format!("Updated {last_updated}")
}
