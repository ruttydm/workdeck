#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    Free,
    Pro,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WelcomeUser {
    pub name: String,
    pub visits: u32,
    pub plan: Option<Plan>,
}

#[must_use]
pub fn render_welcome(user: &WelcomeUser) -> String {
    let display_name = user.name.trim();
    let badge = if user.plan == Some(Plan::Pro) {
        " ⭐"
    } else {
        ""
    };

    format!(
        "Welcome back, {display_name}{badge}. You have visited {} times.",
        user.visits
    )
}

#[must_use]
pub fn render_footer(user: &WelcomeUser) -> &'static str {
    if user.visits > 10 {
        "Thanks for sticking with us."
    } else {
        "Tell us what you'd like to build next."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_the_original_welcome_and_strict_footer_threshold() {
        let user = WelcomeUser {
            name: "  Ada  ".into(),
            visits: 10,
            plan: Some(Plan::Pro),
        };
        assert_eq!(
            render_welcome(&user),
            "Welcome back, Ada ⭐. You have visited 10 times."
        );
        assert_eq!(
            render_footer(&user),
            "Tell us what you'd like to build next."
        );

        assert_eq!(
            render_footer(&WelcomeUser { visits: 11, ..user }),
            "Thanks for sticking with us."
        );
    }
}
