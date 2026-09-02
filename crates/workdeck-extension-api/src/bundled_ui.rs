//! Process-cached registrations for Workdeck's host-rendered bundled UI.

use crate::{
    ExtensionPaneSize, PanePlacement, PaneRegistration, Registration, WORKDECK_VENDOR_EXTENSION_ID,
};
use std::sync::OnceLock;

pub const BUNDLED_SIDEBAR_EXTENSION_ID: &str = WORKDECK_VENDOR_EXTENSION_ID;
pub const BUNDLED_SIDEBAR_VIEW_ID: &str = "files";
pub const BUNDLED_SIDEBAR_SOURCE: &str = "workdeck:bundled/ui/files";

/// Load host-rendered bundled registrations once, just as native subprocess registrations are
/// loaded once after their handshake. Ratatui owns the component implementation.
#[must_use]
pub fn bundled_ui_registry() -> &'static [Registration] {
    static REGISTRY: OnceLock<Vec<Registration>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        vec![Registration::Pane(PaneRegistration {
            id: BUNDLED_SIDEBAR_VIEW_ID.into(),
            title: "Files".into(),
            placement: PanePlacement::Left,
            default_open: true,
            preferred_size: None,
            width: Some(ExtensionPaneSize {
                preferred: 34,
                min: Some(22),
                max: Some(56),
                fraction: Some(0.16),
            }),
            height: None,
            replaces: None,
            current_line: false,
            available: false,
        })]
    })
}

#[must_use]
pub fn bundled_files_pane() -> &'static PaneRegistration {
    let Some(Registration::Pane(pane)) = bundled_ui_registry().first() else {
        unreachable!("the validated bundled UI registry always contains its files pane")
    };
    pane
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_only_the_built_in_files_pane() {
        let registry = bundled_ui_registry();
        assert_eq!(registry.len(), 1);
        assert_eq!(
            format!(
                "{}:{}",
                BUNDLED_SIDEBAR_EXTENSION_ID,
                bundled_files_pane().id
            ),
            crate::WORKDECK_FILES_PANE_KEY
        );
    }

    #[test]
    fn registration_uses_the_exact_responsive_size_contract() {
        let pane = bundled_files_pane();
        assert_eq!(pane.id, BUNDLED_SIDEBAR_VIEW_ID);
        assert_eq!(pane.title, "Files");
        assert_eq!(pane.placement, PanePlacement::Left);
        assert!(pane.default_open);
        assert_eq!(
            pane.width,
            Some(ExtensionPaneSize {
                preferred: 34,
                min: Some(22),
                max: Some(56),
                fraction: Some(0.16),
            })
        );
    }

    #[test]
    fn reserved_vendor_identity_cannot_be_minted_by_a_disk_extension() {
        assert_eq!(BUNDLED_SIDEBAR_EXTENSION_ID, "workdeck");
        assert_eq!(crate::WORKDECK_FILES_PANE_KEY, "workdeck:files");
    }

    #[test]
    fn loads_once_and_returns_the_same_registration() {
        assert!(std::ptr::eq(bundled_files_pane(), bundled_files_pane()));
    }

    #[test]
    fn optional_hunk_pane_lifecycle_fields_are_wire_compatible() {
        let mut pane = bundled_files_pane().clone();
        pane.replaces = Some("vendor:files".into());
        pane.current_line = true;
        pane.available = true;
        let value = serde_json::to_value(&pane).unwrap();
        assert_eq!(value["replaces"], "vendor:files");
        assert_eq!(value["currentLine"], true);
        assert_eq!(value["available"], true);

        let legacy = serde_json::json!({
            "id": "files",
            "title": "Files",
            "placement": "left",
            "default_open": true
        });
        let decoded: PaneRegistration = serde_json::from_value(legacy).unwrap();
        assert_eq!(decoded.replaces, None);
        assert!(!decoded.current_line);
        assert!(!decoded.available);
    }
}
