//! [`Menu`]: a settings-style menu built from a `kaia_events` menu registry.
//!
//! The build result has one submenu per tab. Each tab page reads like this:
//!
//! - a submenu child of the tab is a section. Its name becomes a heading and
//!   its children are rows under it;
//! - a leaf child is a row;
//! - a submenu child of a section (or of a page below it) is a row with a
//!   chevron. A tap opens that submenu as a new page with a back header.
//!
//! Leaf rows queue their callback id as an action. A switch row adds the new
//! state as a bool argument. The menu does not store settings. The app keeps
//! them and reports them back with [`Menu::set_toggle`] and [`Menu::set_value`].
//! Rows show the stored state until then.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use kaia_events::{MenuBuild, MenuItem, MenuKind};
use kaia_gfx::{Point, Size};

use super::{Header, ListRow, PagedList};
use crate::components::{CrossAlign, Label, Padding, Row, SizedBox};
use crate::controls::{Button, ButtonStyle};
use crate::geometry::{Constraints, Insets};
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{LayoutCx, UpdateCx, Widget};

/// Navigation and the state the app reports, shared with the row hooks.
#[derive(Default)]
struct MenuState {
    tab: usize,
    /// Indices of the submenus opened from the tab, outermost first.
    path: Vec<usize>,
    toggles: BTreeMap<String, bool>,
    values: BTreeMap<String, String>,
}

type Shared = Rc<RefCell<MenuState>>;

/// One line of the menu. The list is a [`PagedList`] of these.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Heading(String),
    Gap,
    /// A leaf. `id` keys the toggle and value state. `callback` is the action id.
    Action {
        id: String,
        callback: String,
        text: String,
    },
    /// A submenu. `path` is the full path to it from the tab.
    Push {
        path: Vec<usize>,
        text: String,
    },
}

/// Children of a submenu. Leaves have none.
fn children(item: &MenuItem) -> &[MenuItem] {
    match &item.kind {
        MenuKind::Submenu(items) => items,
        MenuKind::Callback(_) => &[],
    }
}

/// The submenu at `path` below `tab`. An empty path is the tab itself.
fn node_at<'a>(tab: &'a MenuItem, path: &[usize]) -> Option<&'a MenuItem> {
    let mut node = tab;
    for &index in path {
        node = children(node).get(index)?;
    }
    matches!(node.kind, MenuKind::Submenu(_)).then_some(node)
}

fn push_entry(out: &mut Vec<Entry>, item: &MenuItem, path: Vec<usize>) {
    match &item.kind {
        MenuKind::Callback(callback) => out.push(Entry::Action {
            id: item.id.clone(),
            callback: callback.clone(),
            text: item.text.clone(),
        }),
        MenuKind::Submenu(_) => out.push(Entry::Push {
            path,
            text: item.text.clone(),
        }),
    }
    if item.separator {
        out.push(Entry::Gap);
    }
}

/// The entries of the page the state points at.
fn entries_for(tabs: &[MenuItem], st: &MenuState) -> Vec<Entry> {
    let Some(tab) = tabs.get(st.tab) else {
        return Vec::new();
    };
    let Some(node) = node_at(tab, &st.path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if st.path.is_empty() {
        for (i, item) in children(node).iter().enumerate() {
            match &item.kind {
                MenuKind::Submenu(section) => {
                    out.push(Entry::Heading(item.text.clone()));
                    for (j, child) in section.iter().enumerate() {
                        push_entry(&mut out, child, vec![i, j]);
                    }
                    if item.separator {
                        out.push(Entry::Gap);
                    }
                }
                MenuKind::Callback(_) => push_entry(&mut out, item, vec![i]),
            }
        }
    } else {
        for (j, child) in children(node).iter().enumerate() {
            let mut path = st.path.clone();
            path.push(j);
            push_entry(&mut out, child, path);
        }
    }
    out
}

/// Keys rows `row:<id>` for leaves and `push:<path>` for submenus, so tests and
/// apps can find them.
fn entry_row(entry: &Entry, t: &Theme, state: &Shared) -> WidgetPod {
    match entry {
        Entry::Heading(text) => WidgetPod::new(Padding::new(
            WidgetPod::new(Label::new(text.clone(), t.font.label.clone(), Tone::InkSecondary)),
            Insets {
                left: t.space.md,
                top: t.space.md,
                right: t.space.md,
                bottom: t.space.xs,
            },
        )),
        Entry::Gap => WidgetPod::new(SizedBox::new(None).height(t.space.md)),
        Entry::Action { id, callback, text } => {
            let st = state.borrow();
            let mut row = ListRow::new(text.clone());
            if let Some(on) = st.toggles.get(id) {
                row = row.toggle(*on);
            } else if let Some(value) = st.values.get(id) {
                row = row.value(value.clone());
            }
            WidgetPod::new(row.on_action(callback.clone())).with_key(format!("row:{id}"))
        }
        Entry::Push { path, text } => {
            let state = Rc::clone(state);
            let path = path.clone();
            let key = format!("push:{path:?}");
            WidgetPod::new(ListRow::new(text.clone()).chevron().on_hook(move |cx| {
                state.borrow_mut().path = path.clone();
                cx.request_layout();
                cx.request_paint();
            }))
            .with_key(key)
        }
    }
}

/// A tabbed or sectioned settings menu. See the module docs for the layout rules.
pub struct Menu {
    title: String,
    tabs: Vec<MenuItem>,
    state: Shared,
    entries: Option<Vec<Entry>>,
    header: Option<WidgetPod>,
    tab_bar: Option<WidgetPod>,
    list: WidgetPod,
}

impl Menu {
    /// A menu for `build`. `title` heads the first page. The build result
    /// comes from `MenuRegistry::build`.
    pub fn new(title: impl Into<String>, build: MenuBuild) -> Self {
        let state: Shared = Rc::new(RefCell::new(MenuState::default()));
        let row_state = Rc::clone(&state);
        let list = PagedList::new(Vec::<Entry>::new(), move |entry: &Entry, t: &Theme| {
            entry_row(entry, t, &row_state)
        });
        Self {
            title: title.into(),
            tabs: build.tabs,
            state,
            entries: None,
            header: None,
            tab_bar: None,
            list: WidgetPod::new(list),
        }
    }

    /// Sets a switch state before the first layout.
    pub fn toggle(self, id: impl Into<String>, on: bool) -> Self {
        self.state.borrow_mut().toggles.insert(id.into(), on);
        self
    }

    /// Sets a value shown at the trailing edge of a leaf row.
    pub fn value(self, id: impl Into<String>, text: impl Into<String>) -> Self {
        self.state.borrow_mut().values.insert(id.into(), text.into());
        self
    }

    /// Stores the state of a switch row and repaints it. Call it when the app's setting changes.
    pub fn set_toggle(&mut self, id: &str, on: bool, cx: &mut UpdateCx<'_>) {
        self.state.borrow_mut().toggles.insert(id.to_string(), on);
        cx.request_layout();
        cx.request_paint();
    }

    /// Stores the value of a leaf row and repaints it.
    pub fn set_value(&mut self, id: &str, text: impl Into<String>, cx: &mut UpdateCx<'_>) {
        self.state.borrow_mut().values.insert(id.to_string(), text.into());
        cx.request_layout();
        cx.request_paint();
    }

    /// The number of submenus open above the tab page. Zero on the tab page.
    pub fn depth(&self) -> usize {
        self.state.borrow().path.len()
    }

    /// The index of the tab on screen.
    pub fn tab(&self) -> usize {
        self.state.borrow().tab
    }

    /// The tab row. It shows on the tab page only, and only when there are two or more tabs.
    fn tab_bar(&self, t: &Theme) -> Option<WidgetPod> {
        let state = self.state.borrow();
        if self.tabs.len() < 2 || !state.path.is_empty() {
            return None;
        }
        let buttons: Vec<WidgetPod> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let style = if i == state.tab {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                };
                let shared = Rc::clone(&self.state);
                WidgetPod::new(Button::new(style).text(tab.text.clone()).on_hook(move |cx| {
                    let mut st = shared.borrow_mut();
                    st.tab = i;
                    st.path.clear();
                    cx.request_layout();
                    cx.request_paint();
                }))
            })
            .collect();
        let keyed: Vec<WidgetPod> = buttons
            .into_iter()
            .enumerate()
            .map(|(i, b)| b.with_key(format!("menu-tab:{i}")))
            .collect();
        let row = WidgetPod::new(Row::new(keyed).gap(t.space.sm).cross_align(CrossAlign::Center));
        Some(WidgetPod::new(Padding::new(
            row,
            Insets {
                left: t.space.md,
                top: t.space.sm,
                right: t.space.md,
                bottom: t.space.sm,
            },
        )))
    }

    fn page_header(&self, t: &Theme) -> WidgetPod {
        let (title, deep) = {
            let state = self.state.borrow();
            let title = match self.tabs.get(state.tab).and_then(|tab| node_at(tab, &state.path)) {
                Some(node) if !state.path.is_empty() => node.text.clone(),
                _ => self.title.clone(),
            };
            (title, !state.path.is_empty())
        };
        let mut header = Header::new(t, title);
        if deep {
            let shared = Rc::clone(&self.state);
            header = header.back_hook(move |cx| {
                shared.borrow_mut().path.pop();
                cx.request_layout();
                cx.request_paint();
            });
        }
        WidgetPod::new(header).with_key("menu-header")
    }
}

impl Widget for Menu {
    crate::impl_as_any!();

    /// Stacks the header, the tab row and the page list. The list takes the rest.
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let size = c.constrain(c.max);
        let (w, h) = (size.w, size.h);

        self.header = Some(self.page_header(t));
        self.tab_bar = self.tab_bar(t);

        let entries = entries_for(&self.tabs, &self.state.borrow());
        if self.entries.as_ref() != Some(&entries) {
            if let Some(list) = self
                .list
                .widget_mut()
                .as_any_mut()
                .downcast_mut::<PagedList<Entry>>()
            {
                list.reset_items(entries.clone());
            }
            self.entries = Some(entries);
        }

        let mut y = 0u32;
        if let Some(header) = &mut self.header {
            let s = header.layout(cx, Constraints::loose(Size::new(w, h)));
            header.set_origin(Point::new(0, 0));
            y += s.h;
        }
        if let Some(bar) = &mut self.tab_bar {
            let s = bar.layout(cx, Constraints::loose(Size::new(w, h.saturating_sub(y))));
            bar.set_origin(Point::new(0, y as i32));
            y += s.h;
        }
        let rest = h.saturating_sub(y);
        self.list.layout(cx, Constraints::tight(Size::new(w, rest)));
        self.list.set_origin(Point::new(0, y as i32));
        size
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        let mut out: Vec<&mut WidgetPod> = Vec::new();
        if let Some(header) = &mut self.header {
            out.push(header);
        }
        if let Some(bar) = &mut self.tab_bar {
            out.push(bar);
        }
        out.push(&mut self.list);
        out
    }
}
