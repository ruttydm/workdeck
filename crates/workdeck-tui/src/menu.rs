//! Deterministic top-bar and dropdown menu geometry.

use std::collections::BTreeMap;

use crate::measure_text_width;

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
        let entry_width = menu_entry_text(entry).map_or(6, |text| measure_text_width(&text) + 2);
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

/// Stateful controller for the menu bar. The menu contents remain a fresh,
/// caller-owned value so live hints, checks, and extension registrations never
/// become stale inside the controller.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuController {
    active_menu_id: Option<MenuId>,
    active_menu_item_index: usize,
}

impl MenuController {
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.active_menu_id.is_some()
    }

    #[must_use]
    pub fn active_menu_id(&mut self, menus: &AppMenus) -> Option<MenuId> {
        if self
            .active_menu_id
            .is_some_and(|id| build_menu_specs(menus).iter().all(|spec| spec.id != id))
        {
            self.active_menu_id = None;
        }
        self.active_menu_id
    }

    pub fn close(&mut self) {
        self.active_menu_id = None;
    }

    pub fn open(&mut self, menus: &AppMenus, id: MenuId) {
        if menu_entries(menus, id).is_empty() {
            self.close();
            return;
        }
        self.active_menu_id = Some(id);
        self.active_menu_item_index = next_menu_item_index(menu_entries(menus, id), -1, 1);
    }

    pub fn toggle(&mut self, menus: &AppMenus, id: MenuId) {
        if self.active_menu_id(menus) == Some(id) {
            self.close();
        } else {
            self.open(menus, id);
        }
    }

    pub fn switch(&mut self, menus: &AppMenus, delta: isize) {
        let specs = build_menu_specs(menus);
        if specs.is_empty() {
            return;
        }
        let current = self
            .active_menu_id(menus)
            .and_then(|id| specs.iter().position(|spec| spec.id == id))
            .unwrap_or(0);
        let next = (isize::try_from(current).unwrap_or(isize::MAX) + delta)
            .rem_euclid(isize::try_from(specs.len()).unwrap_or(isize::MAX));
        self.open(menus, specs[next as usize].id);
    }

    #[must_use]
    pub fn active_entries<'a>(&mut self, menus: &'a AppMenus) -> &'a [MenuEntry] {
        self.active_menu_id(menus)
            .map_or(&[], |id| menu_entries(menus, id))
    }

    #[must_use]
    pub fn selected_index(&mut self, menus: &AppMenus) -> usize {
        let entries = self.active_entries(menus);
        let current = self.active_menu_item_index;
        let resolved = if matches!(entries.get(current), Some(MenuEntry::Item { .. })) {
            current
        } else {
            next_menu_item_index(
                entries,
                isize::try_from(current.min(entries.len()))
                    .unwrap_or(isize::MAX)
                    .saturating_sub(1),
                1,
            )
        };
        self.active_menu_item_index = resolved;
        resolved
    }

    pub fn set_selected_index(&mut self, menus: &AppMenus, index: usize) {
        self.active_menu_item_index = index;
        let _ = self.selected_index(menus);
    }

    pub fn move_item(&mut self, menus: &AppMenus, delta: isize) {
        let current = self.selected_index(menus);
        self.active_menu_item_index = next_menu_item_index(
            self.active_entries(menus),
            isize::try_from(current).unwrap_or(isize::MAX),
            delta,
        );
    }

    /// Select the highlighted row and close the dropdown. Command execution is
    /// deliberately left to the shared dispatcher.
    #[must_use]
    pub fn activate(&mut self, menus: &AppMenus) -> Option<String> {
        let index = self.selected_index(menus);
        let command_id = match self.active_entries(menus).get(index) {
            Some(MenuEntry::Item { command_id, .. }) => command_id.clone(),
            _ => None,
        };
        self.close();
        command_id
    }
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
        assert!(
            with_extensions.iter().map(|spec| spec.width).sum::<usize>()
                + menu_bar_title_width(&with_extensions, 80)
                <= 80 - 3
        );
        assert_eq!(menu_bar_title_width(&with_extensions, 40), 0);
    }

    #[test]
    fn controller_closes_a_vanished_menu_without_reopening_on_return() {
        let mut controller = MenuController::default();
        let mut menus = base_menus();
        menus.insert(MenuId::Extensions, vec![item()]);
        controller.open(&menus, MenuId::Extensions);
        assert_eq!(controller.active_menu_id(&menus), Some(MenuId::Extensions));

        let base = base_menus();
        assert_eq!(controller.active_menu_id(&base), None);
        controller.toggle(&base, MenuId::File);
        assert_eq!(controller.active_menu_id(&base), Some(MenuId::File));
        controller.close();

        assert_eq!(controller.active_menu_id(&menus), None);
    }

    #[test]
    fn controller_reanchors_selection_when_entries_change() {
        let mut controller = MenuController::default();
        let long = BTreeMap::from([(
            MenuId::File,
            vec![
                MenuEntry::item("Focus"),
                MenuEntry::item("Reload"),
                MenuEntry::item("Quit"),
            ],
        )]);
        controller.open(&long, MenuId::File);
        controller.move_item(&long, 1);
        assert_eq!(controller.selected_index(&long), 1);

        let shrunk = BTreeMap::from([(
            MenuId::File,
            vec![
                MenuEntry::item("Focus"),
                MenuEntry::Separator,
                MenuEntry::Item {
                    label: "Quit".into(),
                    command_id: Some("quit".into()),
                    hint: None,
                    checked: None,
                },
            ],
        )]);
        assert_eq!(controller.selected_index(&shrunk), 2);
        assert_eq!(controller.activate(&shrunk).as_deref(), Some("quit"));

        controller.open(&shrunk, MenuId::File);
        controller.move_item(&shrunk, 1);
        let shortest = BTreeMap::from([(
            MenuId::File,
            vec![MenuEntry::Item {
                label: "Focus".into(),
                command_id: Some("focus".into()),
                hint: None,
                checked: None,
            }],
        )]);
        assert_eq!(controller.selected_index(&shortest), 0);
        assert_eq!(controller.activate(&shortest).as_deref(), Some("focus"));
    }

    #[test]
    fn controller_cycles_only_visible_menus_and_wraps() {
        let menus = BTreeMap::from([(MenuId::File, vec![item()]), (MenuId::View, vec![item()])]);
        let mut controller = MenuController::default();
        controller.open(&menus, MenuId::File);
        controller.switch(&menus, 1);
        assert_eq!(controller.active_menu_id(&menus), Some(MenuId::View));
        controller.switch(&menus, 1);
        assert_eq!(controller.active_menu_id(&menus), Some(MenuId::File));
    }
}
