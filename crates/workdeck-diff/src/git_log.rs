/// Strip `git log -p` / `git show -p` commit metadata while preserving patch bodies.
///
/// A boundary is exactly `commit ` followed by 4-64 lowercase hexadecimal digits and then either
/// a space or end-of-line. Once found, metadata is discarded until the next Git or unified patch
/// header. Context lines beginning with ` commit` are therefore never mistaken for boundaries.
pub fn strip_git_log_metadata(text: &str) -> String {
    if !text.lines().any(is_commit_boundary) {
        return text.to_owned();
    }

    let mut output = Vec::new();
    let mut in_header = false;
    for line in text.split('\n') {
        if is_commit_boundary(line) {
            in_header = true;
            continue;
        }
        if in_header {
            if line.starts_with("diff --git ")
                || line.starts_with("--- ")
                || line.starts_with("+++ ")
            {
                in_header = false;
                output.push(line);
            }
            continue;
        }
        output.push(line);
    }
    output.join("\n")
}

fn is_commit_boundary(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("commit ") else {
        return false;
    };
    let digits = rest
        .bytes()
        .take_while(u8::is_ascii_hexdigit)
        .take_while(|byte| !byte.is_ascii_uppercase())
        .count();
    (4..=64).contains(&digits) && rest.as_bytes().get(digits).is_none_or(|byte| *byte == b' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_regular_patches_unchanged() {
        let patch = "diff --git a/foo b/foo\n@@ -1 +1 @@\n-old\n+new\n";
        assert_eq!(strip_git_log_metadata(patch), patch);
    }

    #[test]
    fn strips_multiple_decorated_commits_stats_and_empty_tail() {
        let input = concat!(
            "commit 1a2b3c4 (HEAD -> main)\n",
            "Author: A\n\n    first\n foo | 2 +-\n",
            "diff --git a/foo b/foo\n@@ -1 +1 @@\n-a\n+b\n\n",
            "commit aaaabbbbccccddddeeeeffff0000111122223333\n",
            "Author: B\n\n    second\n",
        );
        assert_eq!(
            strip_git_log_metadata(input),
            "diff --git a/foo b/foo\n@@ -1 +1 @@\n-a\n+b\n"
        );
    }

    #[test]
    fn accepts_sha256_and_preserves_context_that_mentions_commit() {
        let sha = "a".repeat(64);
        let input = format!(
            "commit {sha}\nAuthor: A\n\ndiff --git a/f b/f\n@@ -1 +1 @@\n commit deadbeef is content\n"
        );
        assert_eq!(
            strip_git_log_metadata(&input),
            "diff --git a/f b/f\n@@ -1 +1 @@\n commit deadbeef is content\n"
        );
    }

    #[test]
    fn rejects_short_uppercase_and_overlong_pseudo_boundaries() {
        for boundary in [
            "commit abc",
            "commit ABCD",
            "commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            let input = format!("{boundary}\nkeep\n");
            assert_eq!(strip_git_log_metadata(&input), input);
        }
    }
}
