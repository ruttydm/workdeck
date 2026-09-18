pub const TOUR_LINE_01: &str =
    "Start at the first changed line and skim for the shape of the review.";
pub const TOUR_LINE_02: &str =
    "Use page-sized jumps first, then refine your place with smaller moves.";
pub const TOUR_LINE_03: &str =
    "Keep the main review stream in view instead of bouncing between files.";
pub const TOUR_LINE_04: &str = "Use the same layout long enough to build a reading rhythm.";
pub const TOUR_LINE_05: &str =
    "Stick with the default theme until the patch itself feels familiar.";
pub const TOUR_LINE_06: &str =
    "Capture the main intent before you zoom into any implementation detail.";
pub const TOUR_LINE_07: &str =
    "Pause on surprising hunks before you decide whether the change makes sense.";
pub const TOUR_LINE_08: &str =
    "Large diffs get easier once you scan the visible anchors and move with intent.";
pub const TOUR_LINE_09: &str =
    "Review notes should stay next to the code they explain, not in a detached summary.";
pub const TOUR_LINE_10: &str =
    "A steady reading order helps people narrate the patch without losing context.";
pub const TOUR_LINE_11: &str =
    "Give every changed block one clear reason to exist before you move on.";
pub const TOUR_LINE_12: &str =
    "Leave enough room for careful follow-up comments when the diff raises questions.";
pub const TOUR_LINE_13: &str =
    "Unchanged spacing between sections keeps separate hunks easy to spot.";
pub const TOUR_LINE_14: &str = "Short stretches of context make larger edits feel trustworthy.";
pub const TOUR_LINE_15: &str =
    "Preview the next stop before you jump ahead with a bigger movement.";
pub const TOUR_LINE_16: &str =
    "Use a calm pace when the patch mixes code, copy, and structural cleanup.";
pub const TOUR_LINE_17: &str = "Complex refactors still benefit from a simple review rhythm.";
pub const TOUR_LINE_18: &str =
    "A single-file walkthrough can teach the whole keyboard model surprisingly fast.";
pub const TOUR_LINE_19: &str =
    "Consistency matters more than memorizing every shortcut in one pass.";
pub const TOUR_LINE_20: &str = "Line-by-line movement should feel steady, local, and predictable.";
pub const TOUR_LINE_21: &str =
    "Page jumps should land near the next cluster of interesting changes.";
pub const TOUR_LINE_22: &str =
    "Home and End are perfect when you want to reset your bearings instantly.";
pub const TOUR_LINE_23: &str =
    "The best demos let you try scrolling without any setup or repo plumbing.";
pub const TOUR_LINE_24: &str =
    "Readable filler text makes navigation practice feel intentional instead of synthetic.";
pub const TOUR_LINE_25: &str = "A few distinct hunks are better than one endless wall of edits.";
pub const TOUR_LINE_26: &str = "Split view is great when you want both sides visible at once.";
pub const TOUR_LINE_27: &str = "Stack view can feel calmer when lines are long or heavily wrapped.";
pub const TOUR_LINE_28: &str =
    "Pager mode should stay familiar to people who already live in git diff and less.";
pub const TOUR_LINE_29: &str =
    "Clean copy changes are useful because they are easy to scan while you learn.";
pub const TOUR_LINE_30: &str =
    "Repeated line structure makes scrolling behavior obvious to first-time users.";
pub const TOUR_LINE_31: &str =
    "A review tool should reward curiosity instead of punishing it with awkward jumps.";
pub const TOUR_LINE_32: &str =
    "Watch how the current hunk stays visually anchored while you move around it.";
pub const TOUR_LINE_33: &str =
    "Try paging down first, then refine your position one line at a time with ↑ and ↓.";
pub const TOUR_LINE_34: &str =
    "When you overshoot, small upward steps should feel immediate and unsurprising.";
pub const TOUR_LINE_35: &str =
    "The footer hints are there to help you build muscle memory on demand.";
pub const TOUR_LINE_36: &str =
    "Once the basics feel good, hunk jumps become the faster path between review beats.";

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
    fn retains_all_revised_tour_lines_and_navigation_copy() {
        assert_eq!(TOUR_LINES.len(), 36);
        assert_eq!(TOUR_LINES[0], TOUR_LINE_01);
        assert_eq!(TOUR_LINES[35], TOUR_LINE_36);
        assert!(TOUR_LINES[32].contains("↑ and ↓"));
        assert!(TOUR_LINES.iter().all(|line| !line.is_empty()));
    }
}
