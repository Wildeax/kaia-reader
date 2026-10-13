//! Paged lists and grids build cells for the visible page only and keep them
//! across layouts (UI-M12).

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::*;
use kaia_ui::components::{Column, CrossAlign, Frame};
use kaia_ui::composite::{BookInfo, BookTile, Grid, ListRow, PagedList};
use kaia_ui::theme::Tone;
use kaia_ui::{Harness, Theme, WidgetPod};

fn counted_list(count: &Rc<Cell<usize>>) -> Frame {
    let count = Rc::clone(count);
    let list = PagedList::new((0..500u32).collect(), move |n: &u32, _t: &Theme| {
        count.set(count.get() + 1);
        WidgetPod::new(ListRow::new(format!("Book {n}")).secondary("An author")).with_key(format!("row{n}"))
    });
    Frame::new(WidgetPod::new(
        Column::new(vec![WidgetPod::new(list).with_key("list")]).cross_align(CrossAlign::Stretch),
    ))
    .fill(Tone::Surface)
}

#[test]
fn only_the_visible_page_is_built() {
    let count = Rc::new(Cell::new(0));
    let t = theme(false);
    let mut h = Harness::new(counted_list(&count), SCREEN, &t, &assets());
    h.frame();
    let built = count.get();
    assert!(built > 5, "a page of rows is built: {built}");
    assert!(
        built < 40,
        "500 items, but only a page (plus one probe) is built: {built}"
    );
}

#[test]
fn a_layout_request_reuses_the_cells() {
    let count = Rc::new(Cell::new(0));
    let t = theme(false);
    let mut h = Harness::new(counted_list(&count), SCREEN, &t, &assets());
    h.frame();
    let built = count.get();
    for _ in 0..3 {
        h.window().invalidate();
        h.frame();
        h.update::<PagedList<u32>, _>("list", |_, u| u.request_layout());
        h.frame();
    }
    assert_eq!(count.get(), built, "no cell is built again");
}

#[test]
fn a_keyed_update_on_a_cell_survives_a_layout() {
    let count = Rc::new(Cell::new(0));
    let t = theme(false);
    let mut h = Harness::new(counted_list(&count), SCREEN, &t, &assets());
    h.frame();
    h.update::<ListRow, _>("row0", |row, u| {
        row.set_value("changed");
        u.request_paint();
    });
    h.update::<PagedList<u32>, _>("list", |_, u| u.request_layout());
    h.frame();
    let mut value = String::new();
    h.update::<ListRow, _>("row0", |row, _| {
        value = match row.trailing() {
            kaia_ui::composite::Trailing::Value(v) => v.clone(),
            _ => String::new(),
        }
    });
    assert_eq!(value, "changed", "the cell kept its update");
}

#[test]
fn turning_the_page_and_setting_items_build_new_cells() {
    let count = Rc::new(Cell::new(0));
    let t = theme(false);
    let mut h = Harness::new(counted_list(&count), SCREEN, &t, &assets());
    h.frame();
    let first = count.get();
    h.update::<PagedList<u32>, _>("list", |l, u| l.set_page(1, u));
    h.frame();
    assert!(count.get() > first, "page 2 builds its own cells");
    assert!(h.find_rect("row0").is_none(), "page 1 rows are gone");
    let before = count.get();
    h.update::<PagedList<u32>, _>("list", |l, u| l.set_items((0..3).collect(), u));
    h.frame();
    assert_eq!(
        count.get() - before,
        3 + 1,
        "three new rows and one probe for the new first item"
    );
    assert!(h.find_rect("row2").is_some());
}

#[test]
fn a_grid_keeps_its_scaled_covers_across_layouts() {
    let t = theme(false);
    let builds = Rc::new(Cell::new(0));
    let b = Rc::clone(&builds);
    let grid = Grid::new((0..60u32).collect(), 3, move |n: &u32, th: &Theme| {
        b.set(b.get() + 1);
        let cover = kaia_gfx::Pixmap::new(300, 450, kaia_gfx::PixelFormat::Rgb24);
        WidgetPod::new(BookTile::new(
            th,
            &BookInfo {
                id: format!("b{n}"),
                title: format!("Book {n}"),
                author: Some("An author".into()),
                cover: Some(cover),
                progress: 0.2,
            },
        ))
    })
    .gap(t.space.md);
    let root = Frame::new(WidgetPod::new(Column::new(vec![
        WidgetPod::new(grid).with_key("grid")
    ])))
    .fill(Tone::Surface);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let built = builds.get();
    assert!(built < 30, "one page of tiles: {built}");
    h.update::<Grid<u32>, _>("grid", |_, u| u.request_layout());
    h.frame();
    assert_eq!(builds.get(), built);
}
