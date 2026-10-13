//! PagedList, Grid, CoverImage and BookTile: behavior through the harness and golden images.
//! Regenerate with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test composite_paged`.

mod common;

use common::*;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::{Color, PixelFormat, Pixmap, Point};
use kaia_ui::components::{Column, Expanded, Row};
use kaia_ui::composite::{BookInfo, BookTile, CoverImage, Grid, ListRow, PagedList};
use kaia_ui::widget::{AppAction, Out};
use kaia_ui::{render, Harness, Theme, UpdateCx, WidgetPod};

/// A list row with a stable key, so tests can find it on the page.
fn row(n: &u32, _t: &Theme) -> WidgetPod {
    WidgetPod::new(ListRow::new(format!("Item {n}")).secondary("Subtitle").chevron())
        .with_key(format!("row-{n}"))
}

fn list_of(count: u32) -> PagedList<u32> {
    PagedList::new((0..count).collect(), row)
}

fn list_screen(count: u32) -> Column {
    Column::new(vec![WidgetPod::new(list_of(count)).with_key("list")])
}

/// Where the nav strip puts its buttons, from the theme.
struct Strip {
    prev: Point,
    next: Point,
}

fn strip(t: &Theme) -> Strip {
    let touch = t.touch_target as i32;
    let side = t.space.sm as i32;
    let y = SCREEN.h as i32 - t.space.sm as i32 - touch / 2;
    let btn_w = (3 * touch).min(SCREEN.w as i32 / 3);
    Strip {
        prev: Point::new(side + btn_w / 2, y),
        next: Point::new(SCREEN.w as i32 - side - btn_w / 2, y),
    }
}

fn page_count(h: &mut Harness) -> usize {
    let mut count = 0;
    h.update::<PagedList<u32>, _>("list", |list, _| count = list.page_count());
    count
}

fn page(h: &mut Harness) -> usize {
    let mut page = 0;
    h.update::<PagedList<u32>, _>("list", |list, _| page = list.page());
    page
}

fn has_row(h: &mut Harness, n: u32) -> bool {
    h.find_rect(&format!("row-{n}")).is_some()
}

#[test]
fn rows_per_page_follow_the_height_and_nothing_scrolls() {
    let t = theme(false);
    let mut h = Harness::new(list_screen(30), SCREEN, &t, &assets());
    h.frame();
    let avail = SCREEN.h - (t.touch_target + t.space.sm);
    let per_page = avail / t.row_height;
    assert_eq!(page_count(&mut h), 30usize.div_ceil(per_page as usize));
    for n in 0..per_page {
        let r = h.find_rect(&format!("row-{n}")).expect("row on the first page");
        assert!(r.bottom() <= (avail as i32), "row {n} ends above the nav strip");
    }
    assert!(!has_row(&mut h, per_page), "the next row waits for its page");
}

#[test]
fn the_next_and_previous_buttons_turn_pages() {
    let t = theme(false);
    let mut h = Harness::new(list_screen(30), SCREEN, &t, &assets());
    h.frame();
    let s = strip(&t);
    assert!(
        h.tap(s.prev),
        "a tap on Previous on page 1 is still the strip's tap"
    );
    assert_eq!(page(&mut h), 0, "Previous does nothing on the first page");
    assert!(h.tap(s.next));
    h.frame();
    assert_eq!(page(&mut h), 1);
    assert!(has_row(&mut h, 9), "the first row of page 2 is on screen");
    assert!(!has_row(&mut h, 0), "the first page is gone");
    assert!(h.tap(s.prev));
    h.frame();
    assert_eq!(page(&mut h), 0);
}

#[test]
fn a_horizontal_swipe_turns_pages_and_a_vertical_one_does_not() {
    let t = theme(false);
    let mut h = Harness::new(list_screen(30), SCREEN, &t, &assets());
    h.frame();
    let y = 600;
    h.swipe(Point::new(900, y), Point::new(200, y));
    h.frame();
    assert_eq!(page(&mut h), 1, "a swipe to the left shows the next page");
    h.swipe(Point::new(200, y), Point::new(900, y));
    h.frame();
    assert_eq!(page(&mut h), 0, "a swipe to the right shows the previous page");
    h.swipe(Point::new(600, 1000), Point::new(600, 300));
    h.frame();
    assert_eq!(page(&mut h), 0, "a vertical drag leaves the page alone");
}

#[test]
fn set_items_through_a_keyed_update_resets_the_page() {
    let t = theme(false);
    let mut h = Harness::new(list_screen(30), SCREEN, &t, &assets());
    h.frame();
    let s = strip(&t);
    h.tap(s.next);
    h.frame();
    assert_eq!(page(&mut h), 1);
    let found = h.update::<PagedList<u32>, _>("list", |list, cx: &mut UpdateCx<'_>| {
        list.set_items((0..5).collect(), cx);
    });
    assert!(found);
    h.frame();
    assert_eq!(page(&mut h), 0);
    assert_eq!(page_count(&mut h), 1);
    assert!(has_row(&mut h, 4));
}

#[test]
fn the_pager_shows_one_row_fewer_when_a_page_would_overflow() {
    // Rows of different heights: the first row is short, the others carry a
    // long two-line title. The page must still fit.
    let t = theme(false);
    let build = |n: &u32, _t: &Theme| {
        let title = if *n == 0 {
            "Short".to_string()
        } else {
            "A title that takes two lines of body text in this narrow column of the list".to_string()
        };
        WidgetPod::new(ListRow::new(title).secondary("Subtitle").chevron()).with_key(format!("row-{n}"))
    };
    let root = Column::new(vec![
        WidgetPod::new(PagedList::new((0..20).collect(), build)).with_key("list")
    ]);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let avail = SCREEN.h - (t.touch_target + t.space.sm);
    let mut n = 0;
    while let Some(r) = h.find_rect(&format!("row-{n}")) {
        assert!(r.bottom() <= avail as i32, "row {n} fits in the page");
        n += 1;
    }
    assert!(
        (1..20).contains(&n),
        "some rows are on the page and the rest wait"
    );
}

fn book(id: u32, cover: Option<Pixmap>) -> BookInfo {
    BookInfo {
        id: format!("book-{id}"),
        title: format!("Book number {id}"),
        author: Some("An author".to_string()),
        cover,
        progress: id as f32 / 6.0,
    }
}

/// A gradient cover so a pixmap cover shows in the golden.
fn gradient_cover(w: u32, h: u32) -> Pixmap {
    let mut pm = Pixmap::new(w, h, PixelFormat::Gray8);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let v = ((x as f32 / w as f32) * 180.0 + 40.0) as u8;
            pm.put(x, y, Color::gray(v));
        }
    }
    pm
}

fn grid_screen(t: &Theme, books: Vec<BookInfo>) -> Column {
    let items = books;
    let grid = Grid::new(items, 3, |b: &BookInfo, t: &Theme| {
        WidgetPod::new(BookTile::new(t, b).on_action("book.open")).with_key(format!("tile-{}", b.id))
    })
    .gap(t.space.md);
    Column::new(vec![WidgetPod::new(grid).with_key("grid")])
}

#[test]
fn a_grid_lays_out_fixed_columns_with_gaps() {
    let t = theme(false);
    let books: Vec<BookInfo> = (1..=6).map(|i| book(i, None)).collect();
    let mut h = Harness::new(grid_screen(&t, books), SCREEN, &t, &assets());
    h.frame();
    let a = h.find_rect("tile-book-1").expect("first tile");
    let b = h.find_rect("tile-book-2").expect("second tile");
    let d = h.find_rect("tile-book-4").expect("fourth tile");
    assert_eq!(a.y, b.y, "tiles in one row share a top edge");
    assert_eq!(
        b.x - a.x,
        a.w as i32 + t.space.md as i32,
        "columns are separated by the gap"
    );
    assert_eq!(d.x, a.x, "the fourth tile starts the second row");
    assert!(d.y > a.y);
}

#[test]
fn tapping_a_book_tile_queues_its_action_with_the_book_id() {
    let t = theme(false);
    let books: Vec<BookInfo> = (1..=3).map(|i| book(i, None)).collect();
    let mut h = Harness::new(grid_screen(&t, books), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("tile-book-2").expect("tile");
    assert!(h.tap(Point::new(r.x + r.w as i32 / 2, r.y + 40)));
    match h.outbox().as_slice() {
        [Out::Action(AppAction { id, args })] => {
            assert_eq!(id, "book.open");
            assert_eq!(args.len(), 1);
            assert_eq!(args[0].as_str(), Some("book-2"));
        }
        other => panic!("expected one action, got {other:?}"),
    }
}

#[test]
fn a_cover_takes_a_two_by_three_box_inside_its_constraints() {
    let t = theme(false);
    let root = Column::new(vec![
        WidgetPod::new(CoverImage::new("Title", None)).with_key("cover")
    ]);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("cover").expect("cover is laid out");
    assert_eq!(r.h, r.w * 3 / 2, "the box keeps the 2 by 3 proportion");
}

#[test]
fn grid_golden_matches() {
    let t = theme(false);
    let cover = gradient_cover(400, 600);
    let books = vec![
        book(1, Some(cover)),
        book(2, None),
        book(3, None),
        book(4, None),
        book(5, None),
        book(6, None),
    ];
    let pm = render(grid_screen(&t, books), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("grid_gray.png"), 0);
}

#[test]
fn paged_list_golden_matches() {
    let t = theme(false);
    let root = Column::new(vec![WidgetPod::new(list_of(12))]);
    let pm = render(root, SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("paged_list_gray.png"), 0);
}

#[test]
fn cover_placeholder_and_image_golden_matches() {
    let t = theme(false);
    let cover = gradient_cover(400, 600);
    let root = Column::new(vec![WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                WidgetPod::new(CoverImage::new("No cover for this book", None)),
                1,
            )),
            WidgetPod::new(Expanded::new(
                WidgetPod::new(CoverImage::new("Has a cover", Some(cover))),
                1,
            )),
        ])
        .gap(t.space.md),
    )]);
    let pm = render(root, SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("cover_gray.png"), 0);
}
