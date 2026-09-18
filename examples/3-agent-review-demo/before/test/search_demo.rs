use crate::agent_review_before::render_command_preview;

#[test]
fn filters_commands_by_substring() {
    assert!(render_command_preview("help").contains("Open help"));
    assert!(render_command_preview("panel").contains("Toggle sidebar"));
}
