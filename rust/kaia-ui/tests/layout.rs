//! Layout math for the container components.

mod common;

use common::Probe;
use common::*;
use kaia_gfx::{Rect, Size};
use kaia_ui::components::{
    Align, Column, CrossAlign, Expanded, Frame, MainAlign, Padding, Row, SizedBox, Spacer,
};
use kaia_ui::{Alignment, Insets, Theme, WidgetPod};

const SMALL: Size = Size::new(300, 500);

/// Renders `root` into a `w` x `h` screen. The root sits in a Stack, which
/// gives it loose constraints, as a real child of a page would get.
fn run(root: impl kaia_ui::Widget + 'static, w: u32, h: u32, _bounds: &Bounds) {
    let t: Theme = theme(false);
    let wrapped = kaia_ui::components::Stack::new(vec![WidgetPod::new(root)]);
    kaia_ui::render(wrapped, Size::new(w, h), &t, &assets());
}

fn probe(name: &'static str, w: u32, h: u32, b: &Bounds) -> WidgetPod {
    WidgetPod::new(Probe::new(name, w, h, b))
}

#[test]
fn column_stacks_children_with_gap() {
    let b = new_bounds();
    let col = Column::new(vec![
        probe("a", 100, 40, &b),
        probe("b", 100, 20, &b),
        probe("c", 100, 30, &b),
    ])
    .gap(10)
    .shrink();
    run(col, SMALL.w, SMALL.h, &b);
    assert_eq!(bounds_of(&b, "a"), Rect::new(0, 0, 100, 40));
    assert_eq!(bounds_of(&b, "b"), Rect::new(0, 50, 100, 20));
    assert_eq!(bounds_of(&b, "c"), Rect::new(0, 80, 100, 30));
}

#[test]
fn column_main_alignment_uses_leftover_space() {
    // Fill height 500. Content is 40 + 20 + 30 plus two gaps of 10: 110. Leftover 390.
    let cases = [
        (MainAlign::Start, [0, 50, 80]),
        (MainAlign::Center, [195, 245, 275]),
        (MainAlign::End, [390, 440, 470]),
        (MainAlign::SpaceBetween, [0, 245, 470]),
    ];
    for (align, ys) in cases {
        let b = new_bounds();
        let col = Column::new(vec![
            probe("a", 100, 40, &b),
            probe("b", 100, 20, &b),
            probe("c", 100, 30, &b),
        ])
        .gap(10)
        .main_align(align);
        run(col, SMALL.w, SMALL.h, &b);
        for (name, y) in ["a", "b", "c"].iter().zip(ys) {
            assert_eq!(bounds_of(&b, name).y, y, "{name} under {align:?}");
        }
    }
}

#[test]
fn column_cross_alignment_and_stretch() {
    let cases = [
        (CrossAlign::Start, [0, 0]),
        (CrossAlign::Center, [50, 0]),
        (CrossAlign::End, [100, 0]),
    ];
    for (align, xs) in cases {
        let b = new_bounds();
        let col = Column::new(vec![probe("a", 100, 10, &b), probe("b", 200, 10, &b)])
            .cross_align(align)
            .shrink();
        run(col, SMALL.w, SMALL.h, &b);
        assert_eq!(bounds_of(&b, "a").x, xs[0], "a under {align:?}");
        assert_eq!(bounds_of(&b, "b").x, xs[1], "b under {align:?}");
    }
    let b = new_bounds();
    let col =
        Column::new(vec![probe("a", 100, 10, &b), probe("b", 200, 10, &b)]).cross_align(CrossAlign::Stretch);
    run(col, SMALL.w, SMALL.h, &b);
    assert_eq!(bounds_of(&b, "a").w, SMALL.w);
    assert_eq!(bounds_of(&b, "b").w, SMALL.w);
}

#[test]
fn row_splits_leftover_by_flex_weight() {
    // 300 wide. Three children have two gaps of 10, so 300 - 50 - 20 = 230 is left.
    // Weights 1 and 2 give 76 and 153 (rounded down).
    let b = new_bounds();
    let row = Row::new(vec![
        probe("fixed", 50, 20, &b),
        WidgetPod::new(Expanded::new(WidgetPod::new(Probe::new("one", 0, 20, &b)), 1)),
        WidgetPod::new(Expanded::new(WidgetPod::new(Probe::new("two", 0, 20, &b)), 2)),
    ])
    .gap(10);
    run(row, SMALL.w, 40, &b);
    assert_eq!(bounds_of(&b, "fixed"), Rect::new(0, 0, 50, 20));
    assert_eq!(bounds_of(&b, "one"), Rect::new(60, 0, 76, 20));
    assert_eq!(bounds_of(&b, "two"), Rect::new(146, 0, 153, 20));
}

#[test]
fn spacer_pushes_the_last_child_to_the_end() {
    let b = new_bounds();
    let row = Row::new(vec![
        probe("left", 50, 20, &b),
        WidgetPod::new(Spacer::new(1)),
        probe("right", 50, 20, &b),
    ]);
    run(row, SMALL.w, 40, &b);
    assert_eq!(bounds_of(&b, "right").x, 250);
}

#[test]
fn flex_children_share_space_by_weight_in_a_column() {
    let b = new_bounds();
    let col = Column::new(vec![
        probe("top", 10, 100, &b),
        WidgetPod::new(Spacer::new(1)),
        WidgetPod::new(Expanded::new(WidgetPod::new(Probe::new("mid", 10, 0, &b)), 3)),
    ]);
    run(col, SMALL.w, SMALL.h, &b);
    // 500 - 100 = 400 free: spacer gets 100, mid gets 300.
    assert_eq!(bounds_of(&b, "mid").h, 300);
    assert_eq!(bounds_of(&b, "mid").y, 200);
}

#[test]
fn children_never_leave_the_constraints() {
    let b = new_bounds();
    let col = Column::new(vec![probe("huge", 1000, 1000, &b)]).shrink();
    run(col, SMALL.w, SMALL.h, &b);
    let r = bounds_of(&b, "huge");
    assert!(r.w <= SMALL.w && r.h <= SMALL.h, "{r:?}");
}

#[test]
fn sized_box_fixes_the_child_size() {
    let b = new_bounds();
    let sb = SizedBox::new(Some(probe("child", 500, 500, &b)))
        .width(120)
        .height(40);
    run(sb, SMALL.w, SMALL.h, &b);
    assert_eq!(bounds_of(&b, "child"), Rect::new(0, 0, 120, 40));
}

#[test]
fn padding_offsets_the_child_by_its_insets() {
    let b = new_bounds();
    let pad = Padding::new(
        probe("child", 50, 20, &b),
        Insets {
            left: 7,
            top: 3,
            right: 0,
            bottom: 0,
        },
    );
    run(pad, SMALL.w, SMALL.h, &b);
    assert_eq!(bounds_of(&b, "child"), Rect::new(7, 3, 50, 20));
}

#[test]
fn frame_padding_leaves_room_around_the_child() {
    let b = new_bounds();
    let frame = Frame::new(probe("child", 50, 20, &b)).padding(Insets::all(10));
    run(frame, SMALL.w, SMALL.h, &b);
    assert_eq!(bounds_of(&b, "child"), Rect::new(10, 10, 50, 20));
}

#[test]
fn align_centers_a_shrunk_column() {
    // A shrink-wrapped column of 40 + 20 = 60 high, centered in 500: top at 220.
    let b = new_bounds();
    let col = Column::new(vec![probe("a", 100, 40, &b), probe("b", 100, 20, &b)]).shrink();
    let al = Align::new(WidgetPod::new(col), Alignment::Center, Alignment::Center);
    run(al, SMALL.w, SMALL.h, &b);
    assert_eq!(bounds_of(&b, "a").y, 220);
    assert_eq!(bounds_of(&b, "b").y, 260);
    assert_eq!(bounds_of(&b, "a").x, 100);
}

#[test]
fn hidden_children_take_no_space() {
    let b = new_bounds();
    let col = Column::new(vec![
        probe("a", 100, 40, &b),
        WidgetPod::new(Probe::new("hidden", 100, 400, &b)).visible(false),
        probe("c", 100, 30, &b),
    ])
    .shrink();
    run(col, SMALL.w, SMALL.h, &b);
    assert_eq!(bounds_of(&b, "c").y, 40);
    assert!(!b.borrow().iter().any(|(n, _)| n == "hidden"));
}
