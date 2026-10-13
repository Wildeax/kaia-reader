//! Menu registry and sorting, mirroring `frontend/ui/menusorter.lua` and the
//! order tables in `frontend/ui/elements/reader_menu_order.lua`.
//!
//! Modules register flat menu items with an id and text. An order table
//! decides where each item goes. Items that the order table does not place
//! become orphans: they go under their `sorting_hint` menu, or into the first
//! tab with a `NEW: ` prefix.
//!
//! Differences from `MenuSorter:sort`:
//! - Lists are built in a fixed order: the tab bar first, then the other order
//!   keys in sorted order. KOReader uses `pairs`, which has no fixed order, so
//!   an item listed in two places can end up in a different place.
//! - A submenu that no tab reaches stays unplaced and becomes an orphan.
//!   KOReader leaves it in its table and does the same.

use std::collections::BTreeMap;

/// The order key for the tab bar. `KOMenu:menu_buttons` in KOReader.
pub const TABS_ID: &str = "KOMenu:menu_buttons";
/// A separator line in an order list. KOReader's `separator_id`.
pub const SEPARATOR_ID: &str = "----------------------------";
/// Items listed here are removed from the menu. `KOMenu:disabled` in KOReader.
pub const DISABLED_ID: &str = "KOMenu:disabled";
/// Prefix for orphans that have no sorting hint. `MenuSorter.orphaned_prefix`.
pub const ORPHAN_PREFIX: &str = "NEW: ";

/// What a menu item does when chosen.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuKind {
    /// Runs the callback with this id. The Lua or Rust side maps the id to code.
    Callback(String),
    /// Opens a submenu with these items. Filled in by the order table when the item is placed.
    Submenu(Vec<MenuItem>),
}

/// A menu item. KOReader's menu table: `text`, `callback` or `sub_item_table`,
/// `sorting_hint` and `separator`.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub id: String,
    pub text: String,
    /// The id of the menu that gets this item when the order table does not
    /// place it. KOReader's `sorting_hint`. It also serves as the parent for
    /// orphans, because placement comes from the order table.
    pub sorting_hint: Option<String>,
    pub kind: MenuKind,
    /// Set by the sorter: draw a separator after this item.
    pub separator: bool,
}

impl MenuItem {
    /// An item that runs the callback `callback`.
    pub fn callback(id: impl Into<String>, text: impl Into<String>, callback: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            sorting_hint: None,
            kind: MenuKind::Callback(callback.into()),
            separator: false,
        }
    }

    /// A submenu. Its items come from the order table when it is placed, or from `items` when it is an orphan.
    pub fn submenu(id: impl Into<String>, text: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            sorting_hint: None,
            kind: MenuKind::Submenu(items),
            separator: false,
        }
    }

    pub fn with_sorting_hint(mut self, hint: impl Into<String>) -> Self {
        self.sorting_hint = Some(hint.into());
        self
    }
}

/// Order lists: a menu id to the ids of its items, in display order.
/// Order tables are read as data, so a user file can replace one list at a time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuOrder {
    lists: BTreeMap<String, Vec<String>>,
}

impl MenuOrder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the list for `id`, replacing any earlier one.
    pub fn set(&mut self, id: &str, items: &[&str]) -> &mut Self {
        self.lists.insert(
            id.to_owned(),
            items.iter().map(|item| (*item).to_owned()).collect(),
        );
        self
    }

    pub fn get(&self, id: &str) -> Option<&[String]> {
        self.lists.get(id).map(Vec::as_slice)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.lists.contains_key(id)
    }

    /// Copies every list from `user` over this order, key by key, as
    /// `MenuSorter:mergeAndSort` does with the user's `*_menu_order.lua` file.
    pub fn merge(&mut self, user: MenuOrder) {
        self.lists.extend(user.lists);
    }
}

/// The result of `MenuRegistry::build`: the tab bar and the warnings KOReader would log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuBuild {
    /// The tabs, in tab-bar order. Each tab is a `MenuKind::Submenu`.
    pub tabs: Vec<MenuItem>,
    pub warnings: Vec<String>,
}

/// Flat storage for menu items, by id.
#[derive(Debug, Clone, Default)]
pub struct MenuRegistry {
    items: BTreeMap<String, MenuItem>,
}

impl MenuRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces an item. Returns the item it replaced, if any.
    pub fn register(&mut self, item: MenuItem) -> Option<MenuItem> {
        self.items.insert(item.id.clone(), item)
    }

    pub fn unregister(&mut self, id: &str) -> Option<MenuItem> {
        self.items.remove(id)
    }

    pub fn get(&self, id: &str) -> Option<&MenuItem> {
        self.items.get(id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Builds the menu from the registered items and `order`. The registry is not changed.
    pub fn build(&self, order: &MenuOrder) -> MenuBuild {
        let mut pool = self.items.clone();
        let mut warnings = Vec::new();
        for id in order.lists.keys() {
            let exempt = id == TABS_ID || id == DISABLED_ID || id == "plus_menu";
            if !exempt && !pool.contains_key(id) {
                warnings.push(format!("menu id not found: {id}"));
            }
        }

        // Phase 1: place every list. Leaves leave the pool here, so they cannot become orphans.
        let root = place_list(order.get(TABS_ID).unwrap_or(&[]), order, &mut pool);
        let mut lists: BTreeMap<String, Vec<Placed>> = BTreeMap::new();
        let keys: Vec<String> = order
            .lists
            .keys()
            .filter(|key| key.as_str() != TABS_ID)
            .cloned()
            .collect();
        for key in keys {
            if pool.contains_key(&key) {
                let items = order.get(&key).unwrap_or(&[]);
                let placed = place_list(items, order, &mut pool);
                lists.insert(key, placed);
            }
        }

        // Phase 2: attach submenus to the tab bar, following references from the root.
        let root_refs: Vec<Placed> = root
            .into_iter()
            .filter(|p| matches!(p.slot, Slot::Ref(_)))
            .collect();
        let mut tabs = attach(root_refs, &mut lists, &mut pool);

        // Phase 3: disabled items are removed, then the rest become orphans.
        if let Some(disabled) = order.get(DISABLED_ID) {
            for id in disabled {
                pool.remove(id);
            }
        }
        for (_, mut item) in pool {
            if let Some(hint) = item.sorting_hint.clone() {
                if let Some(path) = submenu_path(&tabs, &hint) {
                    if let Some(children) = submenu_children_mut(&mut tabs, &path) {
                        children.push(item);
                    }
                    continue;
                }
                warnings.push(format!("sorting hint not found: {hint}"));
            }
            if !item.text.is_empty() {
                item.text = format!("{ORPHAN_PREFIX}{}", item.text);
            }
            match tabs.first_mut() {
                Some(MenuItem {
                    kind: MenuKind::Submenu(children),
                    ..
                }) => children.push(item),
                _ => warnings.push(format!("no tab for orphan: {}", item.id)),
            }
        }

        MenuBuild { tabs, warnings }
    }
}

/// An entry in a placed list: a leaf item or a reference to a submenu.
struct Placed {
    separator: bool,
    slot: Slot,
}

enum Slot {
    Leaf(MenuItem),
    Ref(String),
}

/// `MenuSorter:sort`'s list loop. Submenus become references, leaves are
/// taken from the pool, and separators mark the item before them. Leading
/// separators are ignored, and repeated ones collapse.
fn place_list(items: &[String], order: &MenuOrder, pool: &mut BTreeMap<String, MenuItem>) -> Vec<Placed> {
    let mut placed: Vec<Placed> = Vec::new();
    for id in items {
        if id == SEPARATOR_ID {
            if let Some(last) = placed.last_mut() {
                last.separator = true;
            }
            continue;
        }
        if pool.contains_key(id) && order.contains(id) {
            placed.push(Placed {
                separator: false,
                slot: Slot::Ref(id.clone()),
            });
        } else if let Some(item) = pool.remove(id) {
            placed.push(Placed {
                separator: false,
                slot: Slot::Leaf(item),
            });
        }
    }
    placed
}

/// Turns placed entries into menu items. A reference becomes a submenu whose
/// items come from its list. A reference is used once: it is removed from the pool.
fn attach(
    placed: Vec<Placed>,
    lists: &mut BTreeMap<String, Vec<Placed>>,
    pool: &mut BTreeMap<String, MenuItem>,
) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for entry in placed {
        match entry.slot {
            Slot::Leaf(mut item) => {
                item.separator = entry.separator;
                out.push(item);
            }
            Slot::Ref(id) => {
                let Some(mut node) = pool.remove(&id) else {
                    continue;
                };
                let children = lists
                    .remove(&id)
                    .map(|list| attach(list, lists, pool))
                    .unwrap_or_default();
                node.kind = MenuKind::Submenu(children);
                node.separator = entry.separator;
                out.push(node);
            }
        }
    }
    out
}

/// The index path to the first submenu with `id`, searching depth first.
fn submenu_path(items: &[MenuItem], id: &str) -> Option<Vec<usize>> {
    for (index, item) in items.iter().enumerate() {
        if let MenuKind::Submenu(children) = &item.kind {
            if item.id == id {
                return Some(vec![index]);
            }
            if let Some(mut rest) = submenu_path(children, id) {
                rest.insert(0, index);
                return Some(rest);
            }
        }
    }
    None
}

/// The children of the submenu at `path`.
fn submenu_children_mut<'a>(items: &'a mut [MenuItem], path: &[usize]) -> Option<&'a mut Vec<MenuItem>> {
    let (&first, rest) = path.split_first()?;
    match &mut items.get_mut(first)?.kind {
        MenuKind::Submenu(children) => {
            if rest.is_empty() {
                Some(children)
            } else {
                submenu_children_mut(children, rest)
            }
        }
        MenuKind::Callback(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(id: &str) -> MenuItem {
        MenuItem::callback(id, format!("text {id}"), id)
    }

    fn ids(items: &[MenuItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    fn children(item: &MenuItem) -> &[MenuItem] {
        match &item.kind {
            MenuKind::Submenu(children) => children,
            MenuKind::Callback(_) => panic!("{} is not a submenu", item.id),
        }
    }

    fn sample_registry() -> MenuRegistry {
        let mut r = MenuRegistry::new();
        r.register(MenuItem::submenu("navi", "Navigation", vec![]));
        r.register(MenuItem::submenu("typeset", "Typeset", vec![]));
        r.register(MenuItem::submenu("navi_settings", "Navigation settings", vec![]));
        r.register(leaf("toc"));
        r.register(leaf("bookmarks"));
        r.register(leaf("toc_items"));
        r.register(leaf("go_to"));
        r.register(leaf("font"));
        r.register(leaf("hidden"));
        r.register(leaf("removed"));
        r.register(leaf("late_plugin").with_sorting_hint("navi"));
        r.register(leaf("orphan_plain"));
        r
    }

    fn sample_order() -> MenuOrder {
        let mut order = MenuOrder::new();
        order.set(TABS_ID, &["navi", "typeset", "missing_tab"]);
        order.set(
            "navi",
            &[
                "toc",
                "bookmarks",
                SEPARATOR_ID,
                "navi_settings",
                SEPARATOR_ID,
                "go_to",
                "unknown",
            ],
        );
        order.set("navi_settings", &["toc_items", SEPARATOR_ID, SEPARATOR_ID]);
        order.set("typeset", &[SEPARATOR_ID, "font"]);
        order.set(DISABLED_ID, &["removed", "hidden"]);
        order
    }

    #[test]
    fn places_items_by_order_table() {
        let build = sample_registry().build(&sample_order());
        assert_eq!(ids(&build.tabs), vec!["navi", "typeset"]);
        let navi = &build.tabs[0];
        // "orphan_plain" has no hint, so it lands in the first tab after the placed items.
        assert_eq!(
            ids(children(navi)),
            vec![
                "toc",
                "bookmarks",
                "navi_settings",
                "go_to",
                "late_plugin",
                "orphan_plain"
            ]
        );
    }

    #[test]
    fn separators_mark_the_previous_item_and_collapse() {
        let build = sample_registry().build(&sample_order());
        let navi = children(&build.tabs[0]);
        // "bookmarks" is followed by a separator, then "navi_settings", which is followed by one too.
        assert!(navi[1].separator);
        assert!(navi[2].separator);
        assert!(!navi[0].separator);
        // A separator before the first item of the list is ignored.
        let typeset = children(&build.tabs[1]);
        assert_eq!(ids(typeset), vec!["font"]);
        assert!(!typeset[0].separator);
    }

    #[test]
    fn nested_submenus_are_attached() {
        let build = sample_registry().build(&sample_order());
        let navi = children(&build.tabs[0]);
        let settings = navi.iter().find(|item| item.id == "navi_settings").unwrap();
        assert_eq!(ids(children(settings)), vec!["toc_items"]);
        assert_eq!(settings.text, "Navigation settings");
    }

    #[test]
    fn disabled_items_are_hidden_and_not_orphaned() {
        let build = sample_registry().build(&sample_order());
        let all: Vec<&str> = build
            .tabs
            .iter()
            .flat_map(|tab| children(tab).iter().map(|item| item.id.as_str()))
            .collect();
        assert!(!all.contains(&"removed"));
        assert!(!all.contains(&"hidden"));
    }

    #[test]
    fn unknown_order_ids_warn_and_are_skipped() {
        // Unknown ids inside lists are skipped without a warning, as in KOReader.
        let build = sample_registry().build(&sample_order());
        assert!(build.warnings.is_empty(), "{:?}", build.warnings);
        assert_eq!(build.tabs.len(), 2);
        // A list for a menu that is not registered is reported.
        let mut order = sample_order();
        order.set("ghost_menu", &["toc"]);
        let build = sample_registry().build(&order);
        assert!(build
            .warnings
            .contains(&"menu id not found: ghost_menu".to_owned()));
    }

    #[test]
    fn hinted_orphans_go_to_their_menu() {
        // "late_plugin" has a hint for "navi" and is not in the order table,
        // so it is appended to the navi tab without the NEW prefix.
        let build = sample_registry().build(&sample_order());
        let navi = children(&build.tabs[0]);
        let late = navi.iter().find(|item| item.id == "late_plugin").unwrap();
        assert_eq!(late.text, "text late_plugin");
        assert!(!build.warnings.iter().any(|w| w.contains("late_plugin")));
    }

    #[test]
    fn unhinted_orphans_go_to_the_first_tab_with_new_prefix() {
        let build = sample_registry().build(&sample_order());
        let navi = children(&build.tabs[0]);
        let orphan = navi.iter().find(|item| item.id == "orphan_plain").unwrap();
        assert_eq!(orphan.text, "NEW: text orphan_plain");
    }

    #[test]
    fn unknown_hint_falls_back_to_first_tab_and_warns() {
        let mut r = sample_registry();
        r.register(leaf("lost").with_sorting_hint("no_such_menu"));
        let build = r.build(&sample_order());
        assert!(build
            .warnings
            .contains(&"sorting hint not found: no_such_menu".to_owned()));
        let navi = children(&build.tabs[0]);
        assert!(navi
            .iter()
            .any(|item| item.id == "lost" && item.text == "NEW: text lost"));
    }

    #[test]
    fn items_listed_twice_are_placed_once() {
        let mut order = sample_order();
        order.set("typeset", &["font", "toc"]);
        let build = sample_registry().build(&order);
        let count = |id: &str| {
            build
                .tabs
                .iter()
                .flat_map(|tab| children(tab).iter())
                .filter(|item| item.id == id)
                .count()
        };
        assert_eq!(count("toc"), 1);
        assert_eq!(count("font"), 1);
    }

    #[test]
    fn user_order_replaces_lists_key_by_key() {
        let mut order = sample_order();
        let mut user = MenuOrder::new();
        user.set("navi", &["bookmarks"]);
        order.merge(user);
        let build = sample_registry().build(&order);
        let navi = children(&build.tabs[0]);
        assert_eq!(navi[0].id, "bookmarks");
        // "toc" and "navi_settings" are no longer listed, so they become orphans of the first tab.
        let toc = navi.iter().find(|item| item.id == "toc").unwrap();
        assert_eq!(toc.text, "NEW: text toc");
    }

    #[test]
    fn register_replaces_and_unregister_removes() {
        let mut r = MenuRegistry::new();
        assert!(r.register(leaf("a")).is_none());
        assert!(r.register(MenuItem::callback("a", "new", "cb")).is_some());
        assert_eq!(r.get("a").unwrap().text, "new");
        assert!(r.unregister("a").is_some());
        assert!(r.is_empty());
    }
}
