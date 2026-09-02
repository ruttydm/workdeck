use crate::mini_app_before::render_morning_summary;

#[test]
fn renders_a_flat_morning_summary() {
    let output = render_morning_summary();

    assert!(output.contains("Morning summary"));
    assert!(output.contains("Document keyboard shortcuts"));
}
