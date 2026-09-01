//! Similarity-aware split-view line alignment.
//!
//! The decisive-offset algorithm is translated from Pierre's Apache-2.0
//! `realignChangeContent.ts`; see `THIRD_PARTY_NOTICES`. Workdeck applies the same comparison cap,
//! positional bias, whitespace normalization, and prefix/suffix similarity while producing a
//! renderer-neutral row plan over its Rust diff model.

use workdeck_core::{DiffLine, DiffLineKind};

const MAX_ALIGNMENT_COMPARISONS: usize = 4096;
const MIN_IMPROVEMENT_PER_PAIR: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitLinePair {
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
}

/// Plan split rows, realigning unequal replacement blocks only for a decisive similarity win.
pub fn plan_split_line_pairs(lines: &[DiffLine]) -> Vec<SplitLinePair> {
    let mut pairs = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].kind == DiffLineKind::Context {
            pairs.push(SplitLinePair {
                old_index: Some(index),
                new_index: Some(index),
            });
            index += 1;
            continue;
        }
        let start = index;
        while index < lines.len() && lines[index].kind != DiffLineKind::Context {
            index += 1;
        }
        let deletions = (start..index)
            .filter(|line| lines[*line].kind == DiffLineKind::Deletion)
            .collect::<Vec<_>>();
        let additions = (start..index)
            .filter(|line| lines[*line].kind == DiffLineKind::Addition)
            .collect::<Vec<_>>();
        pairs.extend(align_change_block(lines, &deletions, &additions));
    }
    pairs
}

fn align_change_block(
    lines: &[DiffLine],
    deletions: &[usize],
    additions: &[usize],
) -> Vec<SplitLinePair> {
    let pair_count = deletions.len().min(additions.len());
    let surplus = additions.len().abs_diff(deletions.len());
    if pair_count == 0
        || surplus == 0
        || pair_count.saturating_mul(surplus.saturating_add(1)) > MAX_ALIGNMENT_COMPARISONS
    {
        return positional_pairs(deletions, additions);
    }

    let stripped_deletions = deletions
        .iter()
        .map(|index| strip_whitespace_utf16(&lines[*index].content))
        .collect::<Vec<_>>();
    let stripped_additions = additions
        .iter()
        .map(|index| strip_whitespace_utf16(&lines[*index].content))
        .collect::<Vec<_>>();
    let additions_are_longer = additions.len() > deletions.len();
    let mut best_offset = 0;
    let mut best_score = -1.0;
    for offset in 0..=surplus {
        let score = (0..pair_count)
            .map(|pair| {
                let deletion = pair + usize::from(!additions_are_longer) * offset;
                let addition = pair + usize::from(additions_are_longer) * offset;
                line_similarity(&stripped_deletions[deletion], &stripped_additions[addition])
            })
            .sum::<f64>();
        if offset == 0 {
            best_score = score + pair_count as f64 * MIN_IMPROVEMENT_PER_PAIR;
        } else if score > best_score {
            best_score = score;
            best_offset = offset;
        }
    }
    if best_offset == 0 {
        return positional_pairs(deletions, additions);
    }

    let mut pairs = Vec::with_capacity(deletions.len().max(additions.len()));
    if additions_are_longer {
        pairs.extend(additions[..best_offset].iter().map(|index| SplitLinePair {
            old_index: None,
            new_index: Some(*index),
        }));
        pairs.extend((0..pair_count).map(|pair| SplitLinePair {
            old_index: Some(deletions[pair]),
            new_index: Some(additions[best_offset + pair]),
        }));
        pairs.extend(
            additions[best_offset + pair_count..]
                .iter()
                .map(|index| SplitLinePair {
                    old_index: None,
                    new_index: Some(*index),
                }),
        );
    } else {
        pairs.extend(deletions[..best_offset].iter().map(|index| SplitLinePair {
            old_index: Some(*index),
            new_index: None,
        }));
        pairs.extend((0..pair_count).map(|pair| SplitLinePair {
            old_index: Some(deletions[best_offset + pair]),
            new_index: Some(additions[pair]),
        }));
        pairs.extend(
            deletions[best_offset + pair_count..]
                .iter()
                .map(|index| SplitLinePair {
                    old_index: Some(*index),
                    new_index: None,
                }),
        );
    }
    pairs
}

fn positional_pairs(deletions: &[usize], additions: &[usize]) -> Vec<SplitLinePair> {
    (0..deletions.len().max(additions.len()))
        .map(|index| SplitLinePair {
            old_index: deletions.get(index).copied(),
            new_index: additions.get(index).copied(),
        })
        .collect()
}

fn strip_whitespace_utf16(line: &str) -> Vec<u16> {
    line.chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(|character| {
            let mut encoded = [0; 2];
            let length = character.encode_utf16(&mut encoded).len();
            encoded.into_iter().take(length)
        })
        .collect()
}

fn line_similarity(left: &[u16], right: &[u16]) -> f64 {
    if left == right {
        return 1.0;
    }
    let max_length = left.len().max(right.len());
    let min_length = left.len().min(right.len());
    if min_length == 0 {
        return 0.0;
    }
    let mut prefix = 0;
    while prefix < min_length && left[prefix] == right[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < min_length - prefix
        && left[left.len() - 1 - suffix] == right[right.len() - 1 - suffix]
    {
        suffix += 1;
    }
    (prefix + suffix) as f64 / max_length as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: DiffLineKind, content: &str) -> DiffLine {
        DiffLine {
            kind,
            content: content.into(),
            old_line: None,
            new_line: None,
            moved: false,
            no_newline_at_eof: false,
        }
    }

    #[test]
    fn pairs_a_deletion_with_the_similar_later_addition() {
        let lines = vec![
            line(
                DiffLineKind::Deletion,
                "  return computeTotal(items, taxRate);",
            ),
            line(DiffLineKind::Addition, "  const items = loadItems();"),
            line(DiffLineKind::Addition, "  const taxRate = getTaxRate();"),
            line(
                DiffLineKind::Addition,
                "  return computeTotal(items, taxRate, discount);",
            ),
        ];

        assert_eq!(
            plan_split_line_pairs(&lines),
            vec![
                SplitLinePair {
                    old_index: None,
                    new_index: Some(1)
                },
                SplitLinePair {
                    old_index: None,
                    new_index: Some(2)
                },
                SplitLinePair {
                    old_index: Some(0),
                    new_index: Some(3)
                },
            ]
        );
    }

    #[test]
    fn preserves_positional_pairing_for_balanced_blocks_and_near_ties() {
        let balanced = vec![
            line(DiffLineKind::Deletion, "old one"),
            line(DiffLineKind::Addition, "new one"),
        ];
        assert_eq!(
            plan_split_line_pairs(&balanced),
            vec![SplitLinePair {
                old_index: Some(0),
                new_index: Some(1)
            }]
        );

        let near_tie = vec![
            line(DiffLineKind::Deletion, "import alpha"),
            line(DiffLineKind::Addition, "import beta"),
            line(DiffLineKind::Addition, "import alpha two"),
        ];
        assert_eq!(
            plan_split_line_pairs(&near_tie)[0],
            SplitLinePair {
                old_index: Some(0),
                new_index: Some(1)
            }
        );
    }

    #[test]
    fn handles_the_longer_deletion_side_and_context_rows() {
        let lines = vec![
            line(DiffLineKind::Context, "before"),
            line(DiffLineKind::Deletion, "inserted unrelated"),
            line(DiffLineKind::Deletion, "same call(old)"),
            line(DiffLineKind::Addition, "same call(new)"),
            line(DiffLineKind::Context, "after"),
        ];
        assert_eq!(
            plan_split_line_pairs(&lines),
            vec![
                SplitLinePair {
                    old_index: Some(0),
                    new_index: Some(0)
                },
                SplitLinePair {
                    old_index: Some(1),
                    new_index: None
                },
                SplitLinePair {
                    old_index: Some(2),
                    new_index: Some(3)
                },
                SplitLinePair {
                    old_index: Some(4),
                    new_index: Some(4)
                },
            ]
        );
    }
}
