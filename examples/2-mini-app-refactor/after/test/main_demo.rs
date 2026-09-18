use crate::mini_app_after::render_morning_summary;

#[test]
fn renders_grouped_sections_for_the_morning_summary() {
    let output = render_morning_summary();

    assert!(output.contains("Shipping today"));
    assert!(output.contains("[active] Polish dashboard empty state"));
    assert!(output.contains("Needs help"));
    assert!(output.contains("[blocked] Document keyboard shortcuts"));
}
