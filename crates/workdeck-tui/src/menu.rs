//! Deterministic top-bar and dropdown menu geometry.

use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MenuId {
    File,
    View,
    Navigate,
    Agent,
    Extensions,
    Help,
}

impl MenuId {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::View => "View",
            Self::Navigate => "Navigate",
            Self::Agent => "Agent",
            Self::Extensions => "Extensions",
            Self::Help => "Help",
        }
    }
}

pub const MENU_ORDER: [MenuId; 6] = [
    MenuId::File,
    MenuId::View,
    MenuId::Navigate,
    MenuId::Agent,
    MenuId::Extensions,
    MenuId::Help,
];

/// Rust uses a command ID as the action boundary; the dispatcher owns closures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        label: String,
        command_id: Option<String>,
        hint: Option<String>,
        checked: Option<bool>,
    },
    Separator,
}

impl MenuEntry {
    #[must_use]
    pub fn item(label: impl Into<String>) -> Self {
        Self::Item {
            label: label.into(),
            command_id: None,
            hint: None,
            checked: None,
        }
    }
}

pub type AppMenus = BTreeMap<MenuId, Vec<MenuEntry>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSpec {
    pub id: MenuId,
    pub left: usize,
    pub width: usize,
    pub label: &'static str,
}

#[must_use]
pub fn menu_entries(menus: &AppMenus, id: MenuId) -> &[MenuEntry] {
    menus.get(&id).map(Vec::as_slice).unwrap_or_default()
}

/// Lay out only non-empty menus, in the one shared keyboard/render order.
#[must_use]
pub fn build_menu_specs(menus: &AppMenus) -> Vec<MenuSpec> {
    let mut items: Vec<MenuSpec> = Vec::new();
    for id in MENU_ORDER {
        if menu_entries(menus, id).is_empty() {
            continue;
        }
        let left = items
            .last()
            .map_or(1, |previous| previous.left + previous.width);
        let label = id.label();
        items.push(MenuSpec {
            id,
            left,
            width: label.len() + 2,
            label,
        });
    }
    items
}

/// Find the next selectable item, wrapping and skipping separators.
#[must_use]
pub fn next_menu_item_index(entries: &[MenuEntry], current_index: isize, delta: isize) -> usize {
    if entries.is_empty() {
        return 0;
    }
    let length = entries.len() as isize;
    let mut candidate = current_index;
    for _ in 0..entries.len() {
        candidate = (candidate + delta).rem_euclid(length);
        if matches!(entries[candidate as usize], MenuEntry::Item { .. }) {
            return candidate as usize;
        }
    }
    0
}

fn menu_entry_text(entry: &MenuEntry) -> Option<String> {
    let MenuEntry::Item {
        label,
        hint,
        checked,
        ..
    } = entry
    else {
        return None;
    };
    let check = match checked {
        None => "    ",
        Some(true) => "[x] ",
        Some(false) => "[ ] ",
    };
    let hint = hint
        .as_ref()
        .map_or(String::new(), |hint| format!(" {hint}"));
    Some(format!("{check}{label}{hint}"))
}

/// Fit the widest possible dropdown row with two cells of breathing room.
#[must_use]
pub fn menu_width(entries: &[MenuEntry]) -> usize {
    entries.iter().fold(20, |width, entry| {
        let entry_width = menu_entry_text(entry).map_or(6, |text| text.width() + 2);
        width.max(entry_width)
    })
}

/// Cells available for the changeset title beside the rendered menu specs.
#[must_use]
pub fn menu_bar_title_width(specs: &[MenuSpec], terminal_width: usize) -> usize {
    terminal_width.saturating_sub(specs.iter().map(|spec| spec.width).sum::<usize>() + 6)
}

#[must_use]
pub const fn menu_box_height(entries: &[MenuEntry]) -> usize {
    entries.len() + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> MenuEntry {
        MenuEntry::item("One")
    }

    fn base_menus() -> AppMenus {
        BTreeMap::from([
            (MenuId::File, vec![item()]),
            (MenuId::View, vec![item()]),
            (MenuId::Navigate, vec![item()]),
            (MenuId::Agent, vec![item()]),
            (MenuId::Help, vec![item()]),
        ])
    }

    #[test]
    fn specs_pack_present_menus_in_stable_order() {
        let specs = build_menu_specs(&base_menus());
        assert_eq!(
            specs
                .iter()
                .map(|spec| (spec.id, spec.left, spec.width, spec.label))
                .collect::<Vec<_>>(),
            [
                (MenuId::File, 1, 6, "File"),
                (MenuId::View, 7, 6, "View"),
                (MenuId::Navigate, 13, 10, "Navigate"),
                (MenuId::Agent, 23, 7, "Agent"),
                (MenuId::Help, 30, 6, "Help"),
            ]
        );

        let mut menus = base_menus();
        menus.insert(MenuId::Extensions, vec![item()]);
        let specs = build_menu_specs(&menus);
        assert_eq!(specs[4].id, MenuId::Extensions);
        assert_eq!((specs[4].left, specs[4].width), (30, 12));
        assert_eq!((specs[5].id, specs[5].left), (MenuId::Help, 42));
        menus.insert(MenuId::Extensions, Vec::new());
        assert_eq!(build_menu_specs(&menus).len(), 5);
    }

    #[test]
    fn keyboard_selection_skips_separators_in_both_directions() {
        let entries = [
            MenuEntry::Separator,
            item(),
            MenuEntry::Separator,
            MenuEntry::item("Two"),
        ];
        assert_eq!(next_menu_item_index(&entries, -1, 1), 1);
        assert_eq!(next_menu_item_index(&entries, 1, 1), 3);
        assert_eq!(next_menu_item_index(&entries, 1, -1), 3);
        assert_eq!(next_menu_item_index(&[], 0, 1), 0);
    }

    #[test]
    fn dropdown_geometry_accounts_for_checks_hints_and_wide_cells() {
        let entries = [
            MenuEntry::Item {
                label: "Split view".into(),
                command_id: None,
                hint: Some("1".into()),
                checked: Some(true),
            },
            MenuEntry::Separator,
            MenuEntry::Item {
                label: "Line numbers".into(),
                command_id: None,
                hint: Some("l".into()),
                checked: Some(false),
            },
        ];
        assert!(menu_width(&entries) >= 18);
        assert_eq!(menu_box_height(&entries), 5);

        let ascii = [MenuEntry::item("12345678901234567890")];
        let wide = [MenuEntry::item("拡張機能のコマンドを実行します")];
        assert_eq!(menu_width(&wide), menu_width(&ascii) + 10);
    }

    #[test]
    fn title_width_cedes_exact_space_to_visible_menus() {
        let without_extensions = build_menu_specs(&base_menus());
        let mut menus = base_menus();
        menus.insert(MenuId::Extensions, vec![item()]);
        let with_extensions = build_menu_specs(&menus);
        assert_eq!(menu_bar_title_width(&without_extensions, 80), 39);
        assert_eq!(menu_bar_title_width(&with_extensions, 80), 27);
        assert_eq!(menu_bar_title_width(&with_extensions, 40), 0);
    }
}
