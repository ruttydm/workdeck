//! Renderer-neutral terminal-cell wrapping and clipping.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A styled text fragment whose style type is owned by the caller's renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSegment<S> {
    pub text: String,
    pub style: S,
}

/// Wrap styled text on terminal-cell boundaries without splitting wide characters.
pub fn wrap_segments<S: Clone + PartialEq>(
    segments: Vec<TextSegment<S>>,
    width: usize,
) -> Vec<Vec<TextSegment<S>>> {
    if width == 0 {
        return vec![Vec::new()];
    }
    let mut rows = vec![Vec::new()];
    let mut used = 0_usize;
    for segment in segments {
        for character in segment.text.chars() {
            if character == '\n' {
                rows.push(Vec::new());
                used = 0;
                continue;
            }
            let character_width = character.width().unwrap_or(0);
            if used > 0 && used.saturating_add(character_width) > width {
                rows.push(Vec::new());
                used = 0;
            }
            push_character(
                rows.last_mut().expect("wrapped rows are never empty"),
                character,
                &segment.style,
            );
            used = used.saturating_add(character_width);
        }
    }
    rows
}

/// Clip styled text to one terminal row, preserving only complete display characters.
pub fn clip_segments<S: Clone + PartialEq>(
    segments: Vec<TextSegment<S>>,
    width: usize,
) -> Vec<TextSegment<S>> {
    let mut clipped = Vec::new();
    let mut used = 0_usize;
    'segments: for segment in segments {
        for character in segment.text.chars() {
            if character == '\n' {
                break 'segments;
            }
            let character_width = character.width().unwrap_or(0);
            if used.saturating_add(character_width) > width {
                break 'segments;
            }
            push_character(&mut clipped, character, &segment.style);
            used = used.saturating_add(character_width);
        }
    }
    clipped
}

/// Measure styled text using terminal display cells rather than UTF-8 bytes or scalar count.
pub fn segments_width<S>(segments: &[TextSegment<S>]) -> usize {
    segments.iter().map(|segment| segment.text.width()).sum()
}

fn push_character<S: Clone + PartialEq>(
    segments: &mut Vec<TextSegment<S>>,
    character: char,
    style: &S,
) {
    if let Some(last) = segments
        .last_mut()
        .filter(|segment| segment.style == *style)
    {
        last.text.push(character);
    } else {
        segments.push(TextSegment {
            text: character.to_string(),
            style: style.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_unicode_by_terminal_cells_and_preserves_styles() {
        let rows = wrap_segments(
            vec![
                TextSegment {
                    text: "ab界".into(),
                    style: 1,
                },
                TextSegment {
                    text: "cd".into(),
                    style: 2,
                },
            ],
            4,
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].text, "ab界");
        assert_eq!(rows[0][0].style, 1);
        assert_eq!(rows[1][0].text, "cd");
        assert_eq!(segments_width(&rows[0]), 4);
    }

    #[test]
    fn clips_at_newlines_and_never_splits_wide_characters() {
        assert_eq!(
            clip_segments(
                vec![TextSegment {
                    text: "abc界tail\nignored".into(),
                    style: (),
                }],
                4,
            )[0]
            .text,
            "abc"
        );
        assert_eq!(wrap_segments::<()>(Vec::new(), 0), vec![Vec::new()]);
    }
}
