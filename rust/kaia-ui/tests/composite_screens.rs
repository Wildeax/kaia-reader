//! Full screens built from the composite components, one golden each:
//! settings, library, list and dialog. Regenerate with
//! `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test composite_screens`.

mod common;

use common::*;
use kaia_gfx::golden::assert_matches_golden;
use kaia_ui::components::{Column, CrossAlign, Expanded};
use kaia_ui::composite::{
    BookInfo, BookTile, Dialog, DialogButton, Grid, Header, ListRow, Menu, NavBar, NavItem, PagedList, Toast,
};
use kaia_ui::{render, Theme, WidgetPod, Window};

/// A body above a nav bar. The body takes the space the bar does not need.
fn with_nav(body: WidgetPod, active: NavItem) -> Column {
    Column::new(vec![
        WidgetPod::new(Expanded::new(body, 1)),
        WidgetPod::new(NavBar::new(active)),
    ])
    .cross_align(CrossAlign::Stretch)
}

/// Settings: the menu's header, its tab row, and its sections with switches and values.
fn settings_screen() -> Column {
    let menu = Menu::new("Settings", settings_build())
        .toggle("justify", true)
        .toggle("hyphenate", false)
        .toggle("dark", true)
        .value("font_size", "20")
        .value("top", "Default");
    with_nav(WidgetPod::new(menu), NavItem::Settings)
}

fn books(count: u32) -> Vec<BookInfo> {
    (1..=count)
        .map(|i| BookInfo {
            id: format!("book-{i}"),
            title: match i % 3 {
                0 => "The Left Hand of Darkness".to_string(),
                1 => "A Wizard of Earthsea".to_string(),
                _ => "The Dispossessed".to_string(),
            },
            author: Some("Ursula K. Le Guin".to_string()),
            cover: None,
            progress: (i as f32 * 0.17).fract(),
        })
        .collect()
}

/// Library: a header with a search icon, a grid of book tiles with placeholder covers, and the nav bar.
fn library_screen(t: &Theme) -> Column {
    let grid = Grid::new(books(6), 3, |book: &BookInfo, t: &Theme| {
        WidgetPod::new(BookTile::new(t, book).on_action("book.open"))
    })
    .gap(t.space.md);
    let body = Column::new(vec![
        WidgetPod::new(
            Header::new(t, "Library")
                .action("appbar.search", "library.search")
                .action("appbar.menu", "library.menu"),
        ),
        WidgetPod::new(Expanded::new(WidgetPod::new(grid), 1)),
    ])
    .cross_align(CrossAlign::Stretch);
    with_nav(WidgetPod::new(body), NavItem::Library)
}

fn note_rows(count: u32) -> PagedList<u32> {
    PagedList::new((0..count).collect(), |n: &u32, _t: &Theme| {
        WidgetPod::new(
            ListRow::new(format!("Highlight {} in chapter {}", n + 1, n / 3 + 1))
                .secondary("The Left Hand of Darkness")
                .chevron(),
        )
    })
}

/// List: a header and a paged list of rows with the page indicator, then the nav bar.
fn list_screen(t: &Theme) -> Column {
    let body = Column::new(vec![
        WidgetPod::new(Header::new(t, "Notes")),
        WidgetPod::new(Expanded::new(WidgetPod::new(note_rows(30)), 1)),
    ])
    .cross_align(CrossAlign::Stretch);
    with_nav(WidgetPod::new(body), NavItem::Notes)
}

#[test]
fn settings_screen_golden_matches() {
    let t = theme(false);
    let pm = render(settings_screen(), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("screen_settings_gray.png"), 0);
}

#[test]
fn library_screen_golden_matches() {
    let t = theme(false);
    let pm = render(library_screen(&t), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("screen_library_gray.png"), 0);
}

#[test]
fn list_screen_golden_matches() {
    let t = theme(false);
    let pm = render(list_screen(&t), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("screen_list_gray.png"), 0);
}

#[test]
fn dialog_screen_golden_matches() {
    let t = theme(false);
    let a = assets();
    // The list behind the dialog is dimmed by the dialog's scrim.
    let list = Window::new(list_screen(&t), t.clone(), a.clone(), SCREEN);
    let dialog = Window::new(
        Dialog::new(
            &t,
            "Delete this book?",
            "The file is removed from the device. Notes and highlights are kept in the notes folder.",
            vec![
                DialogButton::new("Cancel", "dialog.cancel"),
                DialogButton::new("Delete", "book.delete"),
            ],
        ),
        t.clone(),
        a.clone(),
        SCREEN,
    );
    let toast = Window::new(
        Toast::new(&t, "Book removed").above(t.nav_bar + t.touch_target + t.space.md),
        t.clone(),
        a.clone(),
        SCREEN,
    );
    let pm = paint_stack(&[&list, &dialog, &toast], &t);
    assert_matches_golden(&pm, &golden("screen_dialog_gray.png"), 0);
}
