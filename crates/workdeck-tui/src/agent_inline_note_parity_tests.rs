//! One-for-one executable translation of Hunk's dedicated AgentInlineNote test corpus.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use workdeck_core::{AgentAnnotation, LineRange, ReviewSide};
use workdeck_diff::sanitize_terminal_line;
use workdeck_review::LayoutMode;

use crate::{
    AgentInlineNoteDraft, AgentInlineNoteViewOptions, AgentInlineNoteViewState,
    draft_visual_line_count, measure_agent_inline_note_height, paint_agent_inline_note,
    resolve_theme, short_review_note_age,
};

fn draft_annotation(body: &str) -> AgentAnnotation {
    AgentAnnotation {
        id: Some("draft:1".into()),
        old_range: None,
        new_range: Some(LineRange { start: 1, end: 1 }),
        summary: if body.is_empty() {
            " ".into()
        } else {
            body.into()
        },
        rationale: None,
        markup: None,
        tags: Vec::new(),
        confidence: None,
        source: Some("user-draft".into()),
        title: None,
        author: None,
        created_at: None,
        updated_at: None,
        editable: true,
    }
}

fn painted_draft(body: &str, width: usize, layout: LayoutMode) -> crate::PaintedAgentInlineNote {
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let annotation = draft_annotation(body);
    let mut options = AgentInlineNoteViewOptions::new(&annotation, layout, &theme, width);
    options.anchor_side = Some(ReviewSide::New);
    options.draft = Some(AgentInlineNoteDraft {
        body,
        focused: true,
        notify_focus: false,
        notify_blur: false,
    });
    paint_agent_inline_note(&AgentInlineNoteViewState::default(), options)
}

fn assert_card_matches_plan(body: &str, width: usize, layout: LayoutMode) {
    let annotation = draft_annotation(body);
    let painted = painted_draft(body, width, layout);
    assert_eq!(
        painted.lines.len(),
        measure_agent_inline_note_height(&annotation, Some(ReviewSide::New), layout, width, 0,)
    );
    assert!(painted.lines.first().unwrap().text().contains("╭─"));
    assert!(painted.lines.last().unwrap().text().contains('╯'));
}

#[test]
fn formats_compact_minute_hour_day_week_and_year_labels() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-30T12:00:00.000Z")
        .unwrap()
        .timestamp_millis();
    assert_eq!(
        short_review_note_age(Some("2026-08-30T11:59:45.000Z"), now),
        "now"
    );
    assert_eq!(
        short_review_note_age(Some("2026-08-30T11:18:00.000Z"), now),
        "42m"
    );
    assert_eq!(
        short_review_note_age(Some("2026-08-30T10:00:00.000Z"), now),
        "2h"
    );
    assert_eq!(
        short_review_note_age(Some("2026-08-28T12:00:00.000Z"), now),
        "2d"
    );
    assert_eq!(
        short_review_note_age(Some("2026-08-09T12:00:00.000Z"), now),
        "3w"
    );
    assert_eq!(
        short_review_note_age(Some("2025-08-30T12:00:00.000Z"), now),
        "1y"
    );
}

macro_rules! draft_count_case {
    ($name:ident, $text:expr, $width:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(draft_visual_line_count(&$text, $width), $expected);
        }
    };
}

draft_count_case!(empty_text, "", 10, 1);
draft_count_case!(short_ascii_fits, "hello", 10, 1);
draft_count_case!(ascii_exact_fit, "aaaaaaaaaa", 10, 1);
draft_count_case!(ascii_one_cell_over, "aaaaaaaaaaa", 10, 2);
draft_count_case!(long_unbroken_ascii, "a".repeat(50), 10, 5);
draft_count_case!(word_slack_packs_by_cells, "aaaaaa aaaaaa aaaaaa", 10, 2);
draft_count_case!(trailing_space_counts, "aaaaaaaaaa ", 10, 2);
draft_count_case!(spaces_only, "   ", 10, 1);
draft_count_case!(cjk_exact_fit, "阿斯蒂芬加", 10, 1);
draft_count_case!(cjk_one_cluster_over, "阿斯蒂芬加快", 10, 2);
draft_count_case!(long_unbroken_cjk, "阿".repeat(25), 10, 5);
draft_count_case!(
    wide_clusters_cannot_straddle_an_odd_width,
    "阿".repeat(25),
    25,
    3
);
draft_count_case!(cjk_punctuation, "你好,世界。你好!", 10, 2);
draft_count_case!(combining_marks_stay_attached, "e\u{301}".repeat(8), 10, 1);
draft_count_case!(
    combining_mark_at_the_boundary,
    format!("{}e\u{301}", "a".repeat(10)),
    10,
    2
);
draft_count_case!(zwj_emoji_cluster, "👨‍👩‍👧".repeat(6), 10, 2);
draft_count_case!(mixed_ascii_and_cjk, "ab阿cd", 10, 1);
draft_count_case!(emoji_run, "🎉".repeat(8), 10, 2);
draft_count_case!(mixed_emoji, "ab🎉cd🎉ef", 6, 2);
draft_count_case!(
    realistic_cjk_prose,
    "这个包主要是为了在普通的chatmodel外面包一层?",
    20,
    3
);
draft_count_case!(hard_newline, "aaa\nbbb", 10, 2);
draft_count_case!(trailing_newline, "aaa\n", 10, 2);
draft_count_case!(empty_middle_line, "aaa\n\nbbb", 10, 3);
draft_count_case!(newline_plus_wrap, "aaaaaaaaaaa\nbbb", 10, 3);
draft_count_case!(tab_is_two_cells, "a\tb", 3, 2);
draft_count_case!(tab_fits_wider_box, "a\tb", 4, 1);
draft_count_case!(tab_after_content, "aaaaaaaa\taa", 10, 2);
draft_count_case!(
    emoji_flag_with_combining_mark,
    format!("HEAD-{}-TAIL", "🇺🇸\u{301}".repeat(10)),
    24,
    2
);
draft_count_case!(bare_heart_emoji, "❤".repeat(13), 24, 2);
draft_count_case!(width_clamps_to_one, "ab", 0, 2);

fn editor_cluster_width(cluster: &str) -> usize {
    match cluster {
        "\t" | "❤" => 2,
        _ => UnicodeWidthStr::width(cluster),
    }
}

fn reference_editor_wrap_count(text: &str, width: usize) -> usize {
    let width = width.max(1);
    text.split('\n')
        .map(|line| {
            let safe = sanitize_terminal_line(line);
            let mut rows = 0_usize;
            let mut used = 0_usize;
            for cluster in safe.graphemes(true) {
                let cluster_width = editor_cluster_width(cluster);
                if used > 0 && used + cluster_width > width {
                    rows += 1;
                    used = 0;
                }
                if cluster_width > width {
                    rows += 1;
                } else {
                    used += cluster_width;
                }
            }
            (rows + usize::from(used > 0)).max(1)
        })
        .sum::<usize>()
        .max(1)
}

fn parity_texts() -> Vec<String> {
    vec![
        "".into(),
        "hello world".into(),
        "a".repeat(10),
        "a".repeat(50),
        "hello world this is a longer line with spaces to wrap properly ok".into(),
        "aaaaaa aaaaaa aaaaaa".into(),
        "阿斯蒂芬加".into(),
        "阿斯蒂芬加快".into(),
        "阿".repeat(25),
        "你好,世界。你好!".into(),
        "这个包主要是为了在普通的chatmodel外面包一层?".into(),
        "ab阿cd🎉ef".into(),
        "🎉".repeat(8),
        "e\u{301}".repeat(12),
        format!("{}e\u{301}", "a".repeat(10)),
        "👨‍👩‍👧".repeat(6),
        format!("HEAD-{}-TAIL", "🇺🇸\u{301}".repeat(10)),
        "❤".repeat(13),
        "a  b   c".into(),
        "aaaaaaaaaa ".into(),
        "aaa\nbbb".into(),
        "aaa\n".into(),
        "aaa\n\nbbb".into(),
        "a\tb".into(),
        "aaaaaaaa\taa".into(),
    ]
}

fn assert_editor_parity(width: usize) {
    for text in parity_texts() {
        assert_eq!(
            draft_visual_line_count(&text, width),
            reference_editor_wrap_count(&text, width),
            "editor wrap mismatch at width {width} for {text:?}"
        );
    }
}

#[test]
fn matches_the_real_editor_wrap_count_at_width_24() {
    assert_editor_parity(24);
}

#[test]
fn matches_the_real_editor_wrap_count_at_width_25() {
    assert_editor_parity(25);
}

#[test]
fn matches_the_real_editor_wrap_count_at_width_40() {
    assert_editor_parity(40);
}

#[test]
fn matches_the_real_editor_wrap_count_at_width_72() {
    assert_editor_parity(72);
}

#[test]
fn renders_long_cjk_drafts_fully_wrapped_with_nothing_scrolled_away() {
    let body = "天地玄黄宇宙洪荒日月盈昃辰宿列张寒来暑往秋收冬藏闰余成岁律吕调阳云腾致雨露结为霜金生丽水玉出昆冈";
    let painted = painted_draft(body, 96, LayoutMode::Split);
    let frame = painted
        .lines
        .iter()
        .map(crate::PaintedAgentInlineNoteLine::text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(frame.contains(&body.chars().take(10).collect::<String>()));
    assert!(
        frame.contains(
            &body
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        )
    );
    assert_card_matches_plan(body, 96, LayoutMode::Split);
}

#[test]
fn keeps_every_typed_cjk_character_visible_while_text_wraps() {
    let mut typed = String::new();
    for character in "一二三四五六七八九十甲乙丙丁戊己庚辛壬癸子丑寅卯辰巳午未申酉戌亥完".chars()
    {
        typed.push(character);
        let painted = painted_draft(&typed, 96, LayoutMode::Split);
        let frame = painted
            .lines
            .iter()
            .map(crate::PaintedAgentInlineNoteLine::text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(frame.contains(&typed.chars().rev().take(1).collect::<String>()));
        assert_card_matches_plan(&typed, 96, LayoutMode::Split);
    }
}

#[test]
fn renders_drafts_with_clusters_that_js_width_tables_mismeasure() {
    let body = format!("HEAD-{}-TAIL", "🇺🇸\u{301}".repeat(10));
    let painted = painted_draft(&body, 34, LayoutMode::Stack);
    let frame = painted
        .lines
        .iter()
        .map(crate::PaintedAgentInlineNoteLine::text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(frame.contains("HEAD-"));
    assert!(frame.contains("TAIL"));
    assert_card_matches_plan(&body, 34, LayoutMode::Stack);
}

#[test]
fn grows_across_hard_newlines_with_wide_characters_before_and_after() {
    let body = "第一行内容\n第二行";
    let painted = painted_draft(body, 96, LayoutMode::Split);
    let frame = painted
        .lines
        .iter()
        .map(crate::PaintedAgentInlineNoteLine::text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(frame.contains("第一行内容"));
    assert!(frame.contains("第二行"));
    assert_card_matches_plan(body, 96, LayoutMode::Split);
}

#[test]
fn grows_to_fit_a_large_bracketed_paste() {
    let body = format!(
        "{}END标记",
        "pasted 文本内容 mixed english words ".repeat(6)
    );
    let painted = painted_draft(&body, 96, LayoutMode::Split);
    let frame = painted
        .lines
        .iter()
        .map(crate::PaintedAgentInlineNoteLine::text)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(frame.contains("pasted 文本"));
    assert!(frame.contains("标记"));
    assert_card_matches_plan(&body, 96, LayoutMode::Split);
}

#[test]
fn rendered_card_height_matches_the_planned_height_for_wide_bodies() {
    for body in [
        "阿".repeat(60),
        format!("{}tail", "🎉".repeat(30)),
        " tabs\tinside\t text ".repeat(4),
        "short\n阿斯蒂芬加快速度发卡号\nend".into(),
        "plain English text that keeps wrapping past one full row of the box".into(),
    ] {
        assert_card_matches_plan(&body, 96, LayoutMode::Split);
    }
}
