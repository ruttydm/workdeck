use dioxus::prelude::*;
use dioxus_free_icons::{
    Icon,
    icons::ld_icons::{
        LdBox, LdChevronDown, LdChevronRight, LdCircleCheck, LdClock, LdCommand, LdExternalLink,
        LdFileCode, LdFilter, LdFolders, LdGitBranch, LdGitPullRequest, LdInbox, LdPanelLeft,
        LdPanelRight, LdPlay, LdPlus, LdRefreshCw, LdSearch, LdTriangleAlert,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconGlyph {
    Inbox,
    Workspaces,
    Git,
    Search,
    Artifacts,
    PullRequest,
    Ci,
    File,
    ChevronDown,
    ChevronRight,
    Check,
    Clock,
    Warning,
    Command,
    External,
    Navigator,
    Inspector,
    Refresh,
    Filter,
    Plus,
}

#[component]
pub fn WorkdeckIcon(glyph: IconGlyph, #[props(default = 16)] size: u32) -> Element {
    let props = |title: &str| Some(title.to_owned());
    match glyph {
        IconGlyph::Inbox => rsx!(Icon {
            icon: LdInbox,
            width: size,
            height: size,
            title: props("Inbox")
        }),
        IconGlyph::Workspaces => rsx!(Icon {
            icon: LdFolders,
            width: size,
            height: size,
            title: props("Workspaces")
        }),
        IconGlyph::Git => rsx!(Icon {
            icon: LdGitBranch,
            width: size,
            height: size,
            title: props("Git")
        }),
        IconGlyph::Search => rsx!(Icon {
            icon: LdSearch,
            width: size,
            height: size,
            title: props("Search")
        }),
        IconGlyph::Artifacts => rsx!(Icon {
            icon: LdBox,
            width: size,
            height: size,
            title: props("Artifacts")
        }),
        IconGlyph::PullRequest => rsx!(Icon {
            icon: LdGitPullRequest,
            width: size,
            height: size,
            title: props("Pull request")
        }),
        IconGlyph::Ci => rsx!(Icon {
            icon: LdPlay,
            width: size,
            height: size,
            title: props("CI")
        }),
        IconGlyph::File => rsx!(Icon {
            icon: LdFileCode,
            width: size,
            height: size,
            title: props("File")
        }),
        IconGlyph::ChevronDown => rsx!(Icon {
            icon: LdChevronDown,
            width: size,
            height: size,
            title: None
        }),
        IconGlyph::ChevronRight => rsx!(Icon {
            icon: LdChevronRight,
            width: size,
            height: size,
            title: None
        }),
        IconGlyph::Check => rsx!(Icon {
            icon: LdCircleCheck,
            width: size,
            height: size,
            title: props("Complete")
        }),
        IconGlyph::Clock => rsx!(Icon {
            icon: LdClock,
            width: size,
            height: size,
            title: props("Waiting")
        }),
        IconGlyph::Warning => rsx!(Icon {
            icon: LdTriangleAlert,
            width: size,
            height: size,
            title: props("Warning")
        }),
        IconGlyph::Command => rsx!(Icon {
            icon: LdCommand,
            width: size,
            height: size,
            title: props("Command")
        }),
        IconGlyph::External => rsx!(Icon {
            icon: LdExternalLink,
            width: size,
            height: size,
            title: props("Open externally")
        }),
        IconGlyph::Navigator => rsx!(Icon {
            icon: LdPanelLeft,
            width: size,
            height: size,
            title: props("Navigator")
        }),
        IconGlyph::Inspector => rsx!(Icon {
            icon: LdPanelRight,
            width: size,
            height: size,
            title: props("Inspector")
        }),
        IconGlyph::Refresh => rsx!(Icon {
            icon: LdRefreshCw,
            width: size,
            height: size,
            title: props("Refresh")
        }),
        IconGlyph::Filter => rsx!(Icon {
            icon: LdFilter,
            width: size,
            height: size,
            title: props("Filter")
        }),
        IconGlyph::Plus => rsx!(Icon {
            icon: LdPlus,
            width: size,
            height: size,
            title: props("Add")
        }),
    }
}
