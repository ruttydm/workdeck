#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedGitPatchFilePaths {
    pub path: String,
    pub previous_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedGitPatch {
    pub text: String,
    pub file_paths: Vec<Option<SanitizedGitPatchFilePaths>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RewriteMode {
    Add,
    PrependPrefix,
    Strip,
}

#[derive(Debug)]
struct HeaderRewrite {
    line: String,
    rewrite_mode: Option<RewriteMode>,
    decoded_pair: Option<PathPair>,
}

#[derive(Debug, Clone)]
struct PathPair {
    old_path: String,
    new_path: String,
}

#[derive(Debug)]
struct CanonicalPair {
    pair: PathPair,
    rewrite_mode: RewriteMode,
    changed: bool,
    canonical: bool,
}

/// Canonicalize Git-format paths into the `a/` and `b/` form used by the Rust parser while
/// retaining exact decoded pathnames beside the parser-safe text.
pub fn sanitize_git_patch(patch_text: &str) -> SanitizedGitPatch {
    if !patch_text.contains("diff --git ") {
        return SanitizedGitPatch {
            text: patch_text.to_owned(),
            file_paths: Vec::new(),
        };
    }

    let mut normalized = Vec::new();
    let mut file_paths = Vec::new();
    let mut block = Vec::new();
    for line in patch_text.split('\n') {
        if line.starts_with("diff --git ") {
            flush_block(&mut block, &mut normalized, &mut file_paths);
            block.push(line);
        } else if block.is_empty() {
            normalized.push(line.to_owned());
        } else {
            block.push(line);
        }
    }
    flush_block(&mut block, &mut normalized, &mut file_paths);
    SanitizedGitPatch {
        text: normalized.join("\n"),
        file_paths,
    }
}

fn flush_block(
    block: &mut Vec<&str>,
    output: &mut Vec<String>,
    file_paths: &mut Vec<Option<SanitizedGitPatchFilePaths>>,
) {
    if block.is_empty() {
        return;
    }
    let rewrite = rewrite_git_diff_header(block[0], block);
    let mut mode = rewrite.rewrite_mode;
    output.push(rewrite.line);
    for line in block.iter().skip(1) {
        if let Some(active) = mode {
            if line.starts_with("--- ") {
                output.push(rewrite_unified_file_line(line, "--- ", "a/", active));
                continue;
            }
            if line.starts_with("+++ ") {
                output.push(rewrite_unified_file_line(line, "+++ ", "b/", active));
                mode = None;
                continue;
            }
        }
        output.push(rewrite_git_metadata_path_line(line));
    }
    file_paths.push(resolve_decoded_git_file_paths(rewrite.decoded_pair, block));
    block.clear();
}

fn rewrite_git_diff_header(line: &str, block: &[&str]) -> HeaderRewrite {
    let rest = line.strip_prefix("diff --git ").unwrap_or(line).trim_end();
    if let Some((quoted_old, quoted_new)) = parse_quoted_pair(rest) {
        let old_path = decode_git_quoted_utf8_path(quoted_old);
        let new_path = decode_git_quoted_utf8_path(quoted_new);
        let pair = canonicalize_git_path_pair(&old_path, &new_path, block);
        let decoded_pair = decode_git_quoted_path(quoted_old)
            .zip(decode_git_quoted_path(quoted_new))
            .map(|(old, new)| canonicalize_git_path_pair(&old, &new, block).pair);
        return HeaderRewrite {
            line: format!("diff --git {} {}", pair.pair.old_path, pair.pair.new_path),
            rewrite_mode: Some(pair.rewrite_mode),
            decoded_pair,
        };
    }

    let tokens = rest.split(' ').collect::<Vec<_>>();
    if tokens.len() >= 2 && tokens.len().is_multiple_of(2) {
        let half = tokens.len() / 2;
        let old_path = tokens[..half].join(" ");
        let new_path = tokens[half..].join(" ");
        if let Some(pair) = canonicalize_known_git_path_pair(&old_path, &new_path, block) {
            if pair.changed {
                return HeaderRewrite {
                    line: format!("diff --git {} {}", pair.pair.old_path, pair.pair.new_path),
                    rewrite_mode: Some(pair.rewrite_mode),
                    decoded_pair: None,
                };
            }
            if pair.canonical {
                return HeaderRewrite {
                    line: line.to_owned(),
                    rewrite_mode: None,
                    decoded_pair: None,
                };
            }
        }
        if old_path == new_path && !old_path.is_empty() {
            return HeaderRewrite {
                line: format!("diff --git a/{old_path} b/{new_path}"),
                rewrite_mode: Some(RewriteMode::PrependPrefix),
                decoded_pair: None,
            };
        }
    }
    if tokens.len() == 2 && !tokens[0].is_empty() && !tokens[1].is_empty() {
        return HeaderRewrite {
            line: format!("diff --git a/{} b/{}", tokens[0], tokens[1]),
            rewrite_mode: Some(RewriteMode::PrependPrefix),
            decoded_pair: None,
        };
    }
    HeaderRewrite {
        line: line.to_owned(),
        rewrite_mode: None,
        decoded_pair: None,
    }
}

fn parse_quoted_pair(value: &str) -> Option<(&str, &str)> {
    fn quoted_end(bytes: &[u8], start: usize) -> Option<usize> {
        if bytes.get(start) != Some(&b'"') {
            return None;
        }
        let mut index = start + 1;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index += 2,
                b'"' => return Some(index),
                _ => index += 1,
            }
        }
        None
    }
    let bytes = value.as_bytes();
    let first_end = quoted_end(bytes, 0)?;
    if bytes.get(first_end + 1) != Some(&b' ') {
        return None;
    }
    let second_start = first_end + 2;
    let second_end = quoted_end(bytes, second_start)?;
    if second_end + 1 != bytes.len() {
        return None;
    }
    Some((&value[1..first_end], &value[second_start + 1..second_end]))
}

fn decode_git_quoted_utf8_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut output = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && octal3(bytes, index).is_some() {
            let start = index;
            let mut decoded = Vec::new();
            while let Some(byte) = octal3(bytes, index) {
                decoded.push(byte);
                index += 4;
            }
            let decoded_bytes = decoded
                .iter()
                .copied()
                .map(u8::try_from)
                .collect::<Result<Vec<_>, _>>();
            let safe_text = decoded_bytes.as_deref().ok().and_then(|bytes| {
                std::str::from_utf8(bytes).ok().filter(|text| {
                    !text.chars().any(|character| {
                        character <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&character)
                    })
                })
            });
            if let Some(text) = safe_text {
                if decoded.iter().all(|byte| *byte >= 0x80) {
                    output.push_str(text);
                } else {
                    output.push_str(&path[start..index]);
                }
            } else {
                output.push_str(&path[start..index]);
            }
            continue;
        }
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            output.push('\\');
            output.push(bytes[index + 1] as char);
            index += 2;
            continue;
        }
        let character = path[index..].chars().next().expect("valid string boundary");
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn octal3(bytes: &[u8], index: usize) -> Option<u16> {
    let digits = bytes.get(index + 1..index + 4)?;
    if bytes.get(index) != Some(&b'\\') || !digits.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
        return None;
    }
    Some(
        u16::from(digits[0] - b'0') * 64
            + u16::from(digits[1] - b'0') * 8
            + u16::from(digits[2] - b'0'),
    )
}

fn decode_git_quoted_path(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            let character = path[index..].chars().next()?;
            let mut buffer = [0; 4];
            decoded.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            index += character.len_utf8();
            continue;
        }
        index += 1;
        let next = *bytes.get(index)?;
        if (b'0'..=b'7').contains(&next) {
            let mut value = 0_u16;
            let mut count = 0;
            while count < 3 && index < bytes.len() && (b'0'..=b'7').contains(&bytes[index]) {
                value = value * 8 + u16::from(bytes[index] - b'0');
                index += 1;
                count += 1;
            }
            if value > u16::from(u8::MAX) {
                return None;
            }
            decoded.push(value as u8);
            continue;
        }
        decoded.push(match next {
            b'a' => 0x07,
            b'b' => 0x08,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 0x0b,
            b'f' => 0x0c,
            b'r' => b'\r',
            b'\\' => b'\\',
            b'"' => b'"',
            _ => return None,
        });
        index += 1;
    }
    String::from_utf8(decoded).ok()
}

fn split_mnemonic_prefix(path: &str) -> Option<(&str, &str)> {
    let (prefix, rest) = path.split_once('/')?;
    ["c", "i", "o", "w", "1", "2"]
        .contains(&prefix)
        .then_some((prefix, rest))
}

fn strip_git_path_quotes(path: &str) -> String {
    quoted_inner(path)
        .map(|inner| {
            decode_git_quoted_path(inner).unwrap_or_else(|| decode_git_quoted_utf8_path(inner))
        })
        .unwrap_or_else(|| path.to_owned())
}

fn rewrite_git_metadata_path_line(line: &str) -> String {
    for marker in ["rename from ", "rename to ", "copy from ", "copy to "] {
        if let Some(value) = line.strip_prefix(marker) {
            if let Some(quoted) = quoted_inner(value) {
                let decoded = decode_git_quoted_utf8_path(quoted);
                if decoded != quoted {
                    return format!("{marker}{decoded}");
                }
            }
            return line.to_owned();
        }
    }
    line.to_owned()
}

fn quoted_inner(value: &str) -> Option<&str> {
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return None;
    }
    let inner = &value[1..value.len() - 1];
    let mut escaped = false;
    for byte in inner.bytes() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return None;
        }
    }
    (!escaped).then_some(inner)
}

fn find_rename_or_copy_metadata(block: &[&str]) -> Option<PathPair> {
    for kind in ["rename", "copy"] {
        let old_marker = format!("{kind} from ");
        let new_marker = format!("{kind} to ");
        let old_path = block.iter().find_map(|line| line.strip_prefix(&old_marker));
        let new_path = block.iter().find_map(|line| line.strip_prefix(&new_marker));
        if let (Some(old_path), Some(new_path)) = (old_path, new_path) {
            return Some(PathPair {
                old_path: strip_git_path_quotes(old_path),
                new_path: strip_git_path_quotes(new_path),
            });
        }
    }
    None
}

fn resolve_decoded_git_file_paths(
    decoded_pair: Option<PathPair>,
    block: &[&str],
) -> Option<SanitizedGitPatchFilePaths> {
    let pair = decoded_pair?;
    let metadata = find_rename_or_copy_metadata(block);
    let previous_path = metadata.as_ref().map_or_else(
        || strip_prefix(&pair.old_path, "a/").to_owned(),
        |pair| pair.old_path.clone(),
    );
    let path = metadata.map_or_else(
        || strip_prefix(&pair.new_path, "b/").to_owned(),
        |pair| pair.new_path,
    );
    Some(SanitizedGitPatchFilePaths {
        previous_path: (previous_path != path).then_some(previous_path),
        path,
    })
}

fn should_strip_mnemonic_pair(old_path: &str, new_path: &str, block: &[&str]) -> Option<bool> {
    let old = split_mnemonic_prefix(old_path)?;
    let new = split_mnemonic_prefix(new_path)?;
    if old.0 == new.0 {
        return None;
    }
    let Some(metadata) = find_rename_or_copy_metadata(block) else {
        return Some(true);
    };
    if metadata.old_path == old_path && metadata.new_path == new_path {
        return Some(false);
    }
    Some(true)
}

fn canonicalize_known_git_path_pair(
    old_path: &str,
    new_path: &str,
    block: &[&str],
) -> Option<CanonicalPair> {
    let canonical = old_path.starts_with("a/") && new_path.starts_with("b/");
    if canonical {
        if find_rename_or_copy_metadata(block)
            .is_some_and(|metadata| metadata.old_path == old_path && metadata.new_path == new_path)
        {
            return None;
        }
        return Some(CanonicalPair {
            pair: PathPair {
                old_path: old_path.to_owned(),
                new_path: new_path.to_owned(),
            },
            rewrite_mode: RewriteMode::Add,
            changed: false,
            canonical: true,
        });
    }
    if should_strip_mnemonic_pair(old_path, new_path, block) == Some(true) {
        let old = split_mnemonic_prefix(old_path)?;
        let new = split_mnemonic_prefix(new_path)?;
        return Some(CanonicalPair {
            pair: PathPair {
                old_path: format!("a/{}", old.1),
                new_path: format!("b/{}", new.1),
            },
            rewrite_mode: RewriteMode::Strip,
            changed: true,
            canonical: false,
        });
    }
    None
}

fn canonicalize_git_path_pair(old_path: &str, new_path: &str, block: &[&str]) -> CanonicalPair {
    canonicalize_known_git_path_pair(old_path, new_path, block).unwrap_or_else(|| CanonicalPair {
        pair: PathPair {
            old_path: format!("a/{old_path}"),
            new_path: format!("b/{new_path}"),
        },
        rewrite_mode: RewriteMode::PrependPrefix,
        changed: true,
        canonical: false,
    })
}

fn rewrite_unified_file_line(line: &str, marker: &str, prefix: &str, mode: RewriteMode) -> String {
    let path = line.strip_prefix(marker).unwrap_or(line);
    let (path_name, suffix, quoted) = if path.starts_with('"') {
        match quoted_value_and_suffix(path) {
            Some((value, suffix)) => (value, suffix, true),
            None => (path, "", false),
        }
    } else {
        (path, "", false)
    };
    if path_name == "/dev/null" || path_name.starts_with("/dev/null\t") {
        return line.to_owned();
    }
    let decoded = if quoted {
        decode_git_quoted_utf8_path(path_name)
    } else {
        path_name.to_owned()
    };
    let normalized = if mode == RewriteMode::Strip {
        split_mnemonic_prefix(&decoded)
            .map_or(decoded.as_str(), |(_, path)| path)
            .to_owned()
    } else {
        decoded
    };
    let prefixed = if mode == RewriteMode::PrependPrefix || !normalized.starts_with(prefix) {
        format!("{prefix}{normalized}")
    } else {
        normalized
    };
    format!("{marker}{prefixed}{suffix}")
}

fn quoted_value_and_suffix(value: &str) -> Option<(&str, &str)> {
    let bytes = value.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some((&value[1..index], &value[index + 1..])),
            _ => index += 1,
        }
    }
    None
}

fn strip_prefix<'a>(value: &'a str, prefix: &str) -> &'a str {
    value.strip_prefix(prefix).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(input: &str) -> String {
        sanitize_git_patch(input).text
    }

    #[test]
    fn leaves_non_git_and_canonical_patches_unchanged() {
        assert_eq!(text("hello\n--- no"), "hello\n--- no");
        let canonical =
            "diff --git a/foo.ts b/foo.ts\n--- a/foo.ts\n+++ b/foo.ts\n@@ -1 +1 @@\n-a\n+b";
        assert_eq!(text(canonical), canonical);
    }

    #[test]
    fn canonicalizes_noprefix_mnemonic_rename_and_dev_null_headers() {
        assert_eq!(
            text("diff --git foo.ts foo.ts\n--- foo.ts\n+++ foo.ts"),
            "diff --git a/foo.ts b/foo.ts\n--- a/foo.ts\n+++ b/foo.ts"
        );
        assert_eq!(
            text("diff --git i/foo.ts w/foo.ts\n--- i/foo.ts\n+++ w/foo.ts"),
            "diff --git a/foo.ts b/foo.ts\n--- a/foo.ts\n+++ b/foo.ts"
        );
        assert_eq!(
            text(
                "diff --git old.ts new.ts\nrename from old.ts\nrename to new.ts\n--- old.ts\n+++ new.ts"
            ),
            "diff --git a/old.ts b/new.ts\nrename from old.ts\nrename to new.ts\n--- a/old.ts\n+++ b/new.ts"
        );
        assert_eq!(
            text("diff --git new.ts new.ts\n--- /dev/null\n+++ new.ts"),
            "diff --git a/new.ts b/new.ts\n--- /dev/null\n+++ b/new.ts"
        );
    }

    #[test]
    fn decodes_quoted_utf8_and_preserves_exact_c_style_paths() {
        let escaped =
            r#"\345\233\275\351\232\233\345\214\226/tab\tquote\"back\\\360\237\247\252.txt"#;
        let patch = format!(
            "diff --git \"a/{escaped}\" \"b/{escaped}\"\n--- \"a/{escaped}\"\n+++ \"b/{escaped}\""
        );
        let normalized = sanitize_git_patch(&patch);
        assert!(normalized.text.contains(r#"tab\tquote\"back\\🧪.txt"#));
        assert_eq!(
            normalized.file_paths,
            vec![Some(SanitizedGitPatchFilePaths {
                path: "国際化/tab\tquote\"back\\🧪.txt".into(),
                previous_path: None,
            })]
        );
    }

    #[test]
    fn copy_metadata_preserves_real_mnemonic_looking_directories() {
        let old = r"i/\346\227\245\346\234\254\350\252\236.txt";
        let new = r"w/\355\225\234\352\265\255\354\226\264.txt";
        let patch = format!(
            "diff --git \"{old}\" \"{new}\"\nsimilarity index 100%\ncopy from \"{old}\"\ncopy to \"{new}\""
        );
        let normalized = sanitize_git_patch(&patch);
        assert_eq!(
            normalized.text,
            "diff --git a/i/日本語.txt b/w/한국어.txt\nsimilarity index 100%\ncopy from i/日本語.txt\ncopy to w/한국어.txt"
        );
        assert_eq!(
            normalized.file_paths[0],
            Some(SanitizedGitPatchFilePaths {
                path: "w/한국어.txt".into(),
                previous_path: Some("i/日本語.txt".into()),
            })
        );
    }

    #[test]
    fn does_not_guess_invalid_octets_or_rewrite_hunk_body_headers() {
        let invalid = r"a/bad\377-overflow\433-csi\302\233-name\\345.txt";
        let patch = format!(
            "diff --git \"{invalid}\" \"{}\"\n--- \"{invalid}\"\n+++ \"{}\"\n@@ -1 +1 @@\n-diff --git x y\n+changed",
            invalid.replacen("a/", "b/", 1),
            invalid.replacen("a/", "b/", 1)
        );
        let normalized = text(&patch);
        assert!(normalized.contains(r"bad\377-overflow\433-csi\302\233-name\\345.txt"));
        assert!(normalized.contains("-diff --git x y"));
    }

    #[test]
    fn normalizes_multiple_blocks_and_keeps_path_metadata_aligned() {
        let first = r"\346\227\245\346\234\254\350\252\236.txt";
        let second = r"\355\225\234\352\265\255\354\226\264.txt";
        let patch = format!(
            "diff --git \"a/{first}\" \"b/{first}\"\n--- \"a/{first}\"\n+++ \"b/{first}\"\n@@ -1 +1 @@\n-a\n+b\ndiff --git \"a/{second}\" \"b/{second}\"\n--- \"a/{second}\"\n+++ \"b/{second}\""
        );
        let normalized = sanitize_git_patch(&patch);
        assert_eq!(normalized.file_paths.len(), 2);
        assert_eq!(
            normalized.file_paths[0].as_ref().unwrap().path,
            "日本語.txt"
        );
        assert_eq!(
            normalized.file_paths[1].as_ref().unwrap().path,
            "한국어.txt"
        );
    }
}
