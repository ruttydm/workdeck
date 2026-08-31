use anyhow::Result;
use dioxus::prelude::*;
use dioxus_desktop::{
    Config, LogicalPosition, LogicalSize, WindowBuilder, WindowCloseBehaviour,
    launch::launch_virtual_dom,
    muda::{
        Menu, MenuItem, PredefinedMenuItem, Submenu,
        accelerator::{Accelerator, CMD_OR_CTRL, Code},
    },
    use_muda_event_handler,
};
use std::cell::RefCell;
use std::process::Command;
use std::time::{Duration, Instant};
use workdeck_api::{FixtureWorkdeckClient, WorkdeckClient};
use workdeck_presenter::LocalWorkdeckClient;
use workdeck_ui::{NativeUiCommand, NativeUiInvocation, WorkdeckApp};

#[cfg(target_os = "macos")]
use dioxus_desktop::tao::platform::macos::WindowBuilderExtMacOS;
#[cfg(target_os = "macos")]
use objc2::MainThreadMarker;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSWindowCollectionBehavior};

#[derive(Default)]
struct MenuLifetimeGuard<T> {
    retained: Option<T>,
}

impl<T> MenuLifetimeGuard<T> {
    fn retain(&mut self, menu: T) {
        self.retained = Some(menu);
    }

    #[cfg(test)]
    fn is_retained(&self) -> bool {
        self.retained.is_some()
    }
}

thread_local! {
    /// On macOS the menu is process-global even though Dioxus stores it on a
    /// window. Retaining this clone prevents the menu children from being
    /// released when a secondary window closes (Dioxus #5753).
    static MENU_LIFETIME_GUARD: RefCell<MenuLifetimeGuard<Menu>> = const {
        RefCell::new(MenuLifetimeGuard { retained: None })
    };
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "workdeck=info".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let client = match std::env::var("WORKDECK_FIXTURE").ok().as_deref() {
        Some("polished") => FixtureWorkdeckClient::polished().into_client(),
        Some("empty") => FixtureWorkdeckClient::empty().into_client(),
        Some("offline") => FixtureWorkdeckClient::offline().into_client(),
        Some(value) => anyhow::bail!("unknown WORKDECK_FIXTURE scenario {value}"),
        None => LocalWorkdeckClient::spawn_default()?,
    };
    let virtual_dom = VirtualDom::new_with_props(DesktopRoot, DesktopRootProps { client });
    let menu = native_menu();
    MENU_LIFETIME_GUARD.with(|guard| guard.borrow_mut().retain(menu.clone()));

    let window = configured_window(window_profile_from_env()?);
    let reveal_until = Instant::now() + Duration::from_secs(2);
    let config = Config::new()
        .with_window(window)
        .with_menu(menu)
        .with_exits_when_last_window_closes(exits_when_last_window_closes())
        .with_close_behaviour(primary_window_close_behaviour())
        .with_disable_context_menu(true)
        .with_navigation_handler(handle_navigation)
        .with_custom_event_handler(move |event, _| {
            #[cfg(target_os = "macos")]
            {
                let should_reveal = (Instant::now() <= reveal_until
                    && matches!(event, dioxus_desktop::tao::event::Event::MainEventsCleared))
                    || matches!(
                        event,
                        dioxus_desktop::tao::event::Event::Reopen {
                            has_visible_windows: false,
                            ..
                        }
                    );
                if should_reveal {
                    reveal_primary_macos_window();
                }
            }
        })
        .with_background_color((29, 32, 29, 255));

    launch_virtual_dom(virtual_dom, config)
}

const MENU_INBOX: &str = "workdeck-navigation-inbox";
const MENU_WORKSPACES: &str = "workdeck-navigation-workspaces";
const MENU_GIT: &str = "workdeck-navigation-git";
const MENU_PULL_REQUESTS: &str = "workdeck-navigation-pull-requests";
const MENU_CI: &str = "workdeck-navigation-ci";
const MENU_SEARCH: &str = "workdeck-navigation-search";
const MENU_ARTIFACTS: &str = "workdeck-navigation-artifacts";
const MENU_COMMAND_PALETTE: &str = "workdeck-command-palette";

#[component]
fn DesktopRoot(client: WorkdeckClient) -> Element {
    let mut invocation = use_signal(NativeUiInvocation::default);
    use_muda_event_handler(move |event| {
        let Some(command) = command_for_menu_id(event.id().as_ref()) else {
            return;
        };
        let sequence = invocation().sequence.saturating_add(1);
        invocation.set(NativeUiInvocation { sequence, command });
    });
    rsx!(WorkdeckApp {
        client,
        native_command: invocation
    })
}

#[cfg(target_os = "macos")]
fn reveal_primary_macos_window() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let application = NSApplication::sharedApplication(mtm);
    let windows = application.windows();
    if let Some(window) = windows.firstObject() {
        window.setCollectionBehavior(NSWindowCollectionBehavior::MoveToActiveSpace);
        window.orderFrontRegardless();
        window.makeKeyAndOrderFront(None);
        #[allow(deprecated)]
        application.activateIgnoringOtherApps(true);
    }
}

fn command_for_menu_id(id: &str) -> Option<NativeUiCommand> {
    match id {
        MENU_INBOX => Some(NativeUiCommand::Inbox),
        MENU_WORKSPACES => Some(NativeUiCommand::Workspaces),
        MENU_GIT => Some(NativeUiCommand::Git),
        MENU_PULL_REQUESTS => Some(NativeUiCommand::PullRequests),
        MENU_CI => Some(NativeUiCommand::Ci),
        MENU_SEARCH => Some(NativeUiCommand::Search),
        MENU_ARTIFACTS => Some(NativeUiCommand::Artifacts),
        MENU_COMMAND_PALETTE => Some(NativeUiCommand::CommandPalette),
        _ => None,
    }
}

fn handle_navigation(url: &str) -> bool {
    if url.starts_with("dioxus://") || url == "about:blank" {
        return true;
    }
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() == "http"
        && parsed.host_str() == Some("127.0.0.1")
        && parsed.port().is_some()
    {
        return true;
    }
    if parsed.scheme() == "https" && parsed.host_str() == Some("github.com") {
        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("/usr/bin/open").arg(parsed.as_str()).spawn();
        }
    }
    false
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowProfile {
    Default,
    Minimum,
}

fn window_profile_from_env() -> Result<WindowProfile> {
    match std::env::var("WORKDECK_WINDOW_PROFILE").ok().as_deref() {
        None | Some("default") => Ok(WindowProfile::Default),
        Some("minimum") => Ok(WindowProfile::Minimum),
        Some(value) => anyhow::bail!("unknown WORKDECK_WINDOW_PROFILE {value}"),
    }
}

const fn initial_window_size(profile: WindowProfile) -> LogicalSize<f64> {
    match profile {
        WindowProfile::Default => LogicalSize::new(1440.0, 900.0),
        WindowProfile::Minimum => LogicalSize::new(900.0, 600.0),
    }
}

fn configured_window(profile: WindowProfile) -> WindowBuilder {
    let window = WindowBuilder::new()
        .with_title("Workdeck")
        .with_inner_size(initial_window_size(profile))
        .with_position(LogicalPosition::new(48.0, 48.0))
        .with_min_inner_size(LogicalSize::new(900.0, 600.0))
        .with_resizable(true)
        .with_visible(true);

    #[cfg(target_os = "macos")]
    let window = window
        .with_title_hidden(true)
        .with_titlebar_transparent(true)
        .with_automatic_window_tabbing(false)
        .with_fullsize_content_view(true);

    window
}

const fn exits_when_last_window_closes() -> bool {
    false
}

const fn primary_window_close_behaviour() -> WindowCloseBehaviour {
    WindowCloseBehaviour::WindowHides
}

fn native_menu() -> Menu {
    let menu = Menu::new();
    let app = Submenu::new("Workdeck", true);
    app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ])
    .expect("native Workdeck application menu");

    let edit = Submenu::new("Edit", true);
    edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ])
    .expect("native Workdeck edit menu");

    let view = Submenu::new("View", true);
    view.append_items(&[
        &MenuItem::with_id(
            MENU_INBOX,
            "Inbox",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit1)),
        ),
        &MenuItem::with_id(
            MENU_WORKSPACES,
            "Workspaces",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit2)),
        ),
        &MenuItem::with_id(
            MENU_GIT,
            "Commits",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit3)),
        ),
        &MenuItem::with_id(
            MENU_PULL_REQUESTS,
            "Pull Requests",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit4)),
        ),
        &MenuItem::with_id(
            MENU_CI,
            "CI",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit5)),
        ),
        &MenuItem::with_id(
            MENU_SEARCH,
            "Search",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit6)),
        ),
        &MenuItem::with_id(
            MENU_ARTIFACTS,
            "Artifacts",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit7)),
        ),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id(
            MENU_COMMAND_PALETTE,
            "Command Palette",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyK)),
        ),
    ])
    .expect("native Workdeck view menu");

    let window = Submenu::new("Window", true);
    window
        .append_items(&[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::fullscreen(None),
            &PredefinedMenuItem::close_window(None),
        ])
        .expect("native Workdeck window menu");

    menu.append_items(&[&app, &edit, &view, &window])
        .expect("native Workdeck menu bar");

    #[cfg(target_os = "macos")]
    window.set_as_windows_menu_for_nsapp();

    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_menu_lifetime_guard_retains_a_clone() {
        let mut guard = MenuLifetimeGuard::default();
        guard.retain(String::from("menu-clone"));
        assert!(guard.is_retained());
    }

    #[test]
    fn configured_window_has_product_minimum_size() {
        let window = configured_window(WindowProfile::Default);
        let attributes = &window.window;
        assert!(attributes.resizable);
        assert!(attributes.inner_size_constraints.has_min());
    }

    #[test]
    fn deterministic_minimum_window_profile_is_explicit_and_bounded() {
        let initial_size = initial_window_size(WindowProfile::Minimum);
        assert_eq!(initial_size.width, 900.0);
        assert_eq!(initial_size.height, 600.0);
        let window = configured_window(WindowProfile::Minimum);
        let attributes = &window.window;
        assert!(attributes.resizable);
        assert!(attributes.inner_size_constraints.has_min());
    }

    #[test]
    fn macos_close_policy_keeps_the_primary_window_available_for_reopen() {
        assert!(!exits_when_last_window_closes());
        assert_eq!(
            primary_window_close_behaviour(),
            WindowCloseBehaviour::WindowHides
        );
    }

    #[test]
    fn navigation_policy_is_loopback_exact_and_external_links_are_allowlisted() {
        assert!(handle_navigation("about:blank"));
        assert!(handle_navigation(
            "http://127.0.0.1:49152/session-token/index.html"
        ));
        assert!(!handle_navigation("file:///Users/example/report.html"));
        assert!(!handle_navigation("https://example.com/report"));
        assert!(!handle_navigation("javascript:alert(1)"));
    }

    #[test]
    fn native_menu_ids_map_to_renderer_commands() {
        assert_eq!(
            command_for_menu_id(MENU_INBOX),
            Some(NativeUiCommand::Inbox)
        );
        assert_eq!(
            command_for_menu_id(MENU_COMMAND_PALETTE),
            Some(NativeUiCommand::CommandPalette)
        );
        assert_eq!(command_for_menu_id("unknown"), None);
    }
}
