pub const TOUR_LINE_01: &str = "Start at the first changed line in the review stream.";
pub const TOUR_LINE_02: &str = "Move with page-sized jumps when the diff is tall.";
pub const TOUR_LINE_03: &str = "Keep an eye on the sidebar for file-level orientation.";
pub const TOUR_LINE_04: &str = "Use the same layout when reviewing small cleanups.";
pub const TOUR_LINE_05: &str = "Stick with the default theme for a neutral first pass.";
pub const TOUR_LINE_06: &str = "Capture the main intent before you zoom into details.";
pub const TOUR_LINE_07: &str = "Pause on surprising hunks before making a judgment.";
pub const TOUR_LINE_08: &str = "Large diffs are easier when you skim the headers first.";
pub const TOUR_LINE_09: &str = "Review notes should stay near the code they explain.";
pub const TOUR_LINE_10: &str = "A steady reading order helps teams narrate the patch.";
pub const TOUR_LINE_11: &str = "Give every changed block one clear reason to exist.";
pub const TOUR_LINE_12: &str = "Leave enough room for careful follow-up comments.";
pub const TOUR_LINE_13: &str = "Unchanged spacing between sections keeps separate hunks obvious.";
pub const TOUR_LINE_14: &str = "Short stretches of context make big edits easier to trust.";
pub const TOUR_LINE_15: &str = "Preview the next stop before you jump ahead.";
pub const TOUR_LINE_16: &str = "Use a calm pace when the patch mixes code and copy.";
pub const TOUR_LINE_17: &str = "Complex refactors still benefit from a simple review rhythm.";
pub const TOUR_LINE_18: &str = "A single-file walkthrough can teach the whole keyboard model.";
pub const TOUR_LINE_19: &str = "Consistency matters more than memorizing every shortcut at once.";
pub const TOUR_LINE_20: &str = "Line-by-line movement should feel steady and predictable.";
pub const TOUR_LINE_21: &str = "Page jumps should land you near the next cluster of changes.";
pub const TOUR_LINE_22: &str = "Home and End help when you want to reset your bearings.";
pub const TOUR_LINE_23: &str = "The best demos let you try scrolling without any setup.";
pub const TOUR_LINE_24: &str = "Readable filler text keeps navigation practice from feeling fake.";
pub const TOUR_LINE_25: &str = "A few distinct hunks are better than one endless wall of edits.";
pub const TOUR_LINE_26: &str = "Split view is great when you want both sides visible at once.";
pub const TOUR_LINE_27: &str = "Stack view can feel calmer when lines are long.";
pub const TOUR_LINE_28: &str = "Pager mode should stay familiar to people who live in git diff.";
pub const TOUR_LINE_29: &str = "Clean copy changes are useful because they are easy to scan.";
pub const TOUR_LINE_30: &str = "Repeated line structure makes scrolling behavior obvious.";
pub const TOUR_LINE_31: &str = "A review tool should reward curiosity instead of punishing it.";
pub const TOUR_LINE_32: &str = "Watch how the current hunk stays visually anchored while you move.";
pub const TOUR_LINE_33: &str =
    "Try paging down first, then refine your position one line at a time.";
pub const TOUR_LINE_34: &str = "When you overshoot, small upward steps should feel natural.";
pub const TOUR_LINE_35: &str = "The footer hints are there to help you build muscle memory.";
pub const TOUR_LINE_36: &str = "Once the basics feel good, hunk jumps become the faster path.";

pub const TOUR_LINES: [&str; 36] = [
    TOUR_LINE_01,
    TOUR_LINE_02,
    TOUR_LINE_03,
    TOUR_LINE_04,
    TOUR_LINE_05,
    TOUR_LINE_06,
    TOUR_LINE_07,
    TOUR_LINE_08,
    TOUR_LINE_09,
    TOUR_LINE_10,
    TOUR_LINE_11,
    TOUR_LINE_12,
    TOUR_LINE_13,
    TOUR_LINE_14,
    TOUR_LINE_15,
    TOUR_LINE_16,
    TOUR_LINE_17,
    TOUR_LINE_18,
    TOUR_LINE_19,
    TOUR_LINE_20,
    TOUR_LINE_21,
    TOUR_LINE_22,
    TOUR_LINE_23,
    TOUR_LINE_24,
    TOUR_LINE_25,
    TOUR_LINE_26,
    TOUR_LINE_27,
    TOUR_LINE_28,
    TOUR_LINE_29,
    TOUR_LINE_30,
    TOUR_LINE_31,
    TOUR_LINE_32,
    TOUR_LINE_33,
    TOUR_LINE_34,
    TOUR_LINE_35,
    TOUR_LINE_36,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_all_original_tour_lines() {
        assert_eq!(TOUR_LINES.len(), 36);
        assert_eq!(TOUR_LINES[0], TOUR_LINE_01);
        assert_eq!(TOUR_LINES[35], TOUR_LINE_36);
        assert!(TOUR_LINES.iter().all(|line| !line.is_empty()));
    }
}
