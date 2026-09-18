use crate::agent_review_after::render_command_preview;

#[test]
fn prefers_normalized_shortcut_matches() {
    let output = render_command_preview("short-cuts");

    assert_eq!(output.lines().next(), Some("• Open help"));
    assert!(render_command_preview("panel").contains("Toggle sidebar"));
}
