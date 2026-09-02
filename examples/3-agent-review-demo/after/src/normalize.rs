#[must_use]
pub fn normalize_query(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut replacing_separator = false;
    for character in value.trim().to_lowercase().chars() {
        if matches!(character, '-' | '_') {
            if !replacing_separator {
                normalized.push(' ');
            }
            replacing_separator = true;
        } else {
            normalized.push(character);
            replacing_separator = false;
        }
    }
    normalized
}
