#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Free,
    Pro,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub display_name: String,
    pub visits: u32,
    pub plan: Option<Plan>,
}

fn welcome_badge(viewer: &Viewer) -> &'static str {
    if viewer.plan == Some(Plan::Pro) {
        " · Pro"
    } else {
        ""
    }
}

#[must_use]
pub fn render_welcome(viewer: &Viewer) -> String {
    let name = viewer.display_name.trim();

    format!(
        "Welcome back, {name}{}. Session {}.",
        welcome_badge(viewer),
        viewer.visits
    )
}

#[must_use]
pub fn render_footer(viewer: &Viewer) -> &'static str {
    if viewer.visits >= 10 {
        "Thanks for sticking with Workdeck."
    } else {
        "Tip: press ] to jump to the next hunk."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_refactored_welcome_and_inclusive_footer_threshold() {
        let viewer = Viewer {
            display_name: "  Ada  ".into(),
            visits: 10,
            plan: Some(Plan::Pro),
        };
        assert_eq!(
            render_welcome(&viewer),
            "Welcome back, Ada · Pro. Session 10."
        );
        assert_eq!(render_footer(&viewer), "Thanks for sticking with Workdeck.");

        assert_eq!(
            render_footer(&Viewer {
                visits: 9,
                ..viewer
            }),
            "Tip: press ] to jump to the next hunk."
        );
    }
}
