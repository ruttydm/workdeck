//! Workdeck's Dioxus renderer. Components depend only on `workdeck-api`; all native I/O
//! remains behind an injected client.

use dioxus::prelude::{Asset, AssetOptions, asset};

mod app;
mod components;
mod model;
mod surfaces;

pub use app::{NativeUiCommand, NativeUiInvocation, WorkdeckApp, WorkdeckAppProps};
pub use components::ComponentGallery;
pub use model::{Area, ReviewLens, SearchActivation};

pub const WORKDECK_CSS: &str = include_str!("../assets/workdeck.css");

// The stylesheet owns the font declarations, while these unhashed asset
// registrations guarantee the same stable URLs in web and desktop bundles.
#[used]
static GEIST_FONT: Asset = asset!(
    "/assets/fonts/Geist.ttf",
    AssetOptions::builder().with_hash_suffix(false)
);

#[used]
static GEIST_MONO_FONT: Asset = asset!(
    "/assets/fonts/GeistMono.ttf",
    AssetOptions::builder().with_hash_suffix(false)
);

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus::prelude::*;
    use workdeck_api::FixtureWorkdeckClient;

    #[test]
    fn shell_renders_core_landmarks() {
        let client = FixtureWorkdeckClient::polished().into_client();
        let html = dioxus_ssr::render_element(rsx! { WorkdeckApp { client } });
        assert!(html.contains("Workdeck"));
        assert!(html.contains("Opening your workspace"));
        assert!(html.contains("Global navigation"));
    }

    #[test]
    fn empty_fixture_renders_without_panicking() {
        let client = FixtureWorkdeckClient::empty().into_client();
        let html = dioxus_ssr::render_element(rsx! { WorkdeckApp { client } });
        assert!(html.contains("Workdeck"));
    }

    #[test]
    fn compiled_design_system_uses_bundle_safe_font_urls() {
        assert!(WORKDECK_CSS.contains(".workdeck-app"));
        assert!(WORKDECK_CSS.contains("/assets/Geist.ttf"));
        assert!(WORKDECK_CSS.contains("/assets/GeistMono.ttf"));
        assert!(!WORKDECK_CSS.contains("url(./Geist"));
    }
}
