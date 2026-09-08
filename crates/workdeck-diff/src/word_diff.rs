//! Inline word-difference emphasis compatible with Pierre's default `word-alt` policy.

use std::ops::Range;

use crate::HIGHLIGHT_WORD_DIFF_MAX_LINE_LENGTH_UTF16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordDiffRanges {
    pub old: Vec<Range<usize>>,
    pub new: Vec<Range<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeKind {
    Neutral,
    Removed,
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    kind: ChangeKind,
    text: String,
}

#[derive(Debug, Clone)]
struct Token<'a> {
    text: &'a str,
}

/// Return byte ranges to emphasize on a paired deletion/addition row.
///
/// Each word, punctuation mark, newline, or non-newline whitespace run is one comparison token.
/// A one-character neutral gap between changes joins the surrounding emphasis, matching Pierre's
/// default `word-alt` presentation. Lines above Pierre's 10,000 UTF-16-unit cap are left
/// undecorated.
pub fn word_diff_ranges(old: &str, new: &str) -> WordDiffRanges {
    if old.encode_utf16().count() > HIGHLIGHT_WORD_DIFF_MAX_LINE_LENGTH_UTF16
        || new.encode_utf16().count() > HIGHLIGHT_WORD_DIFF_MAX_LINE_LENGTH_UTF16
    {
        return WordDiffRanges {
            old: Vec::new(),
            new: Vec::new(),
        };
    }
    let old_tokens = tokenize(old);
    let new_tokens = tokenize(new);
    let changes = diff_tokens(&old_tokens, &new_tokens);
    WordDiffRanges {
        old: emphasis_ranges(&changes, ChangeKind::Removed),
        new: emphasis_ranges(&changes, ChangeKind::Added),
    }
}

fn tokenize(value: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut current_kind = None;
    for (index, character) in value.char_indices() {
        let kind = if character == '\n' || character == '\r' {
            2
        } else if character.is_whitespace() {
            0
        } else if is_word_character(character) {
            1
        } else {
            2
        };
        let continues = current_kind == Some(kind) && kind != 2;
        if !continues && index > start {
            tokens.push(Token {
                text: &value[start..index],
            });
            start = index;
        }
        current_kind = Some(kind);
        if kind == 2 {
            let end = index + character.len_utf8();
            tokens.push(Token {
                text: &value[index..end],
            });
            start = end;
            current_kind = None;
        }
    }
    if start < value.len() {
        tokens.push(Token {
            text: &value[start..],
        });
    }
    tokens
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '\u{ad}'
}

fn diff_tokens(old: &[Token<'_>], new: &[Token<'_>]) -> Vec<Change> {
    // The traceback always consumes equal leading tokens. Remove only that forced
    // prefix before allocating the quadratic table; suffix trimming can alter ties.
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(a, b)| a.text == b.text)
        .count();
    if prefix == 0 {
        return diff_tokens_untrimmed(old, new);
    }
    let mut changes = Vec::new();
    for token in &old[..prefix] {
        push_change(&mut changes, ChangeKind::Neutral, token.text);
    }
    for change in diff_tokens_untrimmed(&old[prefix..], &new[prefix..]) {
        push_change(&mut changes, change.kind, &change.text);
    }
    changes
}

fn diff_tokens_untrimmed(old: &[Token<'_>], new: &[Token<'_>]) -> Vec<Change> {
    let columns = new.len() + 1;
    let mut lcs = vec![0_u16; (old.len() + 1).saturating_mul(columns)];
    for old_index in (0..old.len()).rev() {
        for new_index in (0..new.len()).rev() {
            let value = if old[old_index].text == new[new_index].text {
                lcs[(old_index + 1) * columns + new_index + 1].saturating_add(1)
            } else {
                lcs[(old_index + 1) * columns + new_index]
                    .max(lcs[old_index * columns + new_index + 1])
            };
            lcs[old_index * columns + new_index] = value;
        }
    }

    let mut changes = Vec::new();
    let mut old_index = 0;
    let mut new_index = 0;
    while old_index < old.len() || new_index < new.len() {
        if old_index < old.len()
            && new_index < new.len()
            && old[old_index].text == new[new_index].text
        {
            push_change(&mut changes, ChangeKind::Neutral, old[old_index].text);
            old_index += 1;
            new_index += 1;
        } else if old_index < old.len()
            && (new_index == new.len()
                || lcs[(old_index + 1) * columns + new_index]
                    >= lcs[old_index * columns + new_index + 1])
        {
            push_change(&mut changes, ChangeKind::Removed, old[old_index].text);
            old_index += 1;
        } else {
            push_change(&mut changes, ChangeKind::Added, new[new_index].text);
            new_index += 1;
        }
    }
    changes
}

fn push_change(changes: &mut Vec<Change>, kind: ChangeKind, text: &str) {
    if let Some(last) = changes.last_mut().filter(|change| change.kind == kind) {
        last.text.push_str(text);
    } else {
        changes.push(Change {
            kind,
            text: text.into(),
        });
    }
}

fn emphasis_ranges(changes: &[Change], side: ChangeKind) -> Vec<Range<usize>> {
    let mut spans: Vec<(bool, String)> = Vec::new();
    for (index, change) in changes.iter().enumerate() {
        if change.kind != ChangeKind::Neutral && change.kind != side {
            continue;
        }
        let neutral = change.kind == ChangeKind::Neutral;
        let is_last_item = index + 1 == changes.len();
        let join = spans.last().is_some_and(|(highlighted, _)| {
            neutral != *highlighted
                || (neutral
                    && !is_last_item
                    && change.text.encode_utf16().count() == 1
                    && *highlighted)
        });
        if join && !is_last_item {
            spans
                .last_mut()
                .expect("span exists")
                .1
                .push_str(&change.text);
        } else {
            spans.push((!neutral, change.text.clone()));
        }
    }

    let mut offset = 0;
    let mut ranges = Vec::new();
    for (highlighted, text) in spans {
        let end = offset + text.len();
        if highlighted && end > offset {
            ranges.push(offset..end);
        }
        offset = end;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_reduction_preserves_traceback_and_emphasis_for_exhaustive_short_tokens() {
        let alphabet = ["a", "b", " ", "+", "日", "🚀"];
        let mut sequences = vec![Vec::new()];
        for length in 1..=3 {
            for encoded in 0..6usize.pow(length) {
                let mut value = encoded;
                sequences.push(
                    (0..length)
                        .map(|_| {
                            let token = Token {
                                text: alphabet[value % 6],
                            };
                            value /= 6;
                            token
                        })
                        .collect(),
                );
            }
        }
        for old in &sequences {
            for new in &sequences {
                let expected = diff_tokens_untrimmed(old, new);
                let actual = diff_tokens(old, new);
                assert_eq!(actual, expected, "old={old:?} new={new:?}");
                for side in [ChangeKind::Removed, ChangeKind::Added] {
                    assert_eq!(
                        emphasis_ranges(&actual, side),
                        emphasis_ranges(&expected, side)
                    );
                }
            }
        }
    }

    #[test]
    fn emphasizes_only_the_inserted_call_argument() {
        let diff = word_diff_ranges(
            "return computeTotal(items, taxRate);",
            "return computeTotal(items, taxRate, discount);",
        );
        assert!(diff.old.is_empty());
        assert_eq!(
            diff.new
                .iter()
                .map(|range| &"return computeTotal(items, taxRate, discount);"[range.clone()])
                .collect::<Vec<_>>(),
            vec![", discount"]
        );
    }

    #[test]
    fn word_alt_joins_single_space_gaps_but_not_a_neutral_suffix() {
        let changed = word_diff_ranges("alpha beta tail", "gamma delta tail");
        assert_eq!(&"alpha beta tail"[changed.old[0].clone()], "alpha beta");
        assert_eq!(&"gamma delta tail"[changed.new[0].clone()], "gamma delta");
        assert_eq!(changed.old[0].end, "alpha beta".len());
    }

    #[test]
    fn skips_pathological_long_lines() {
        let old = "a".repeat(10_001);
        let new = "b".repeat(10_001);
        assert_eq!(
            word_diff_ranges(&old, &new),
            WordDiffRanges {
                old: Vec::new(),
                new: Vec::new()
            }
        );
    }

    #[test]
    fn retains_word_emphasis_at_the_worker_length_boundary() {
        let old = "a".repeat(HIGHLIGHT_WORD_DIFF_MAX_LINE_LENGTH_UTF16);
        let new = "b".repeat(HIGHLIGHT_WORD_DIFF_MAX_LINE_LENGTH_UTF16);
        assert_eq!(
            word_diff_ranges(&old, &new),
            WordDiffRanges {
                old: std::iter::once(0..old.len()).collect(),
                new: std::iter::once(0..new.len()).collect()
            }
        );
    }

    #[test]
    fn tokenizes_words_whitespace_punctuation_and_unicode() {
        assert_eq!(
            tokenize("sum_1 + 界界")
                .into_iter()
                .map(|token| token.text)
                .collect::<Vec<_>>(),
            vec!["sum_1", " ", "+", " ", "界界"]
        );
    }
}
