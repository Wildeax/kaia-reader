//! Painting a stack of windows: closing an overlay, repainting under one,
//! the scrim, and limiting a layout request to the widgets that moved.
//! These follow the app's loop (`MiniApp` in `common`).

mod common;

use common::*;
use kaia_events::ShowOpts;
use kaia_gfx::Rect;
use kaia_ui::components::{Align, Column, CrossAlign, Frame, Label, Paragraph, SizedBox};
use kaia_ui::composite::{Dialog, DialogButton, Toast};
use kaia_ui::theme::Tone;
use kaia_ui::{Alignment, WidgetPod, Window};

const SCREEN_AREA: i64 = 1264 * 1680;

fn full_screen() -> ShowOpts {
    ShowOpts {
        covers_fullscreen: true,
        ..ShowOpts::default()
    }
}

/// A full-screen page with a centered label keyed "lbl".
fn centered_page(app: &MiniApp, text: &str) -> Window {
    let t = &app.theme;
    let label = WidgetPod::new(Label::new(text, t.font.title.clone(), Tone::Ink)).with_key("lbl");
    let root = Frame::new(WidgetPod::new(Align::new(
        label,
        Alignment::Center,
        Alignment::Center,
    )))
    .fill(Tone::Surface);
    app.window(root)
}

fn toast_window(app: &MiniApp, text: &str) -> Window {
    app.window(Toast::new(&app.theme, text))
}

fn dialog_window(app: &MiniApp) -> Window {
    let body = "Body text that is long enough to cover the middle of the screen. ".repeat(4);
    let d = Dialog::new(
        &app.theme,
        "Title",
        body,
        vec![
            DialogButton::new("Cancel", "dialog.cancel"),
            DialogButton::new("OK", "dialog.ok"),
        ],
    );
    app.window(d)
}

#[test]
fn closing_a_toast_erases_it_from_the_frame() {
    let mut app = MiniApp::new();
    let base = centered_page(&app, "Base screen");
    app.show(&base, full_screen());
    app.repaint();
    let before = app.frame.clone();

    let toast = toast_window(&app, "Bookmark added");
    let id = app.show(
        &toast,
        ShowOpts {
            toast: true,
            ..ShowOpts::default()
        },
    );
    app.repaint();
    assert!(
        app.dark_pixels(Rect::new(0, 1300, 1264, 380)) > 1000,
        "the toast box is drawn"
    );
    assert!(
        app.refreshed_area() < SCREEN_AREA / 4,
        "showing a toast refreshes only the toast box"
    );

    app.close(id);
    assert!(app.repaint(), "closing repaints the uncovered window");
    assert!(app.frame == before, "the frame is the same as before the toast");
    assert!(
        app.refreshed_area() < SCREEN_AREA / 4,
        "the close refreshes only the toast box"
    );
    assert!(app.refreshed_area() > 0);
}

#[test]
fn closing_a_dialog_removes_the_scrim_and_the_panel() {
    let mut app = MiniApp::new();
    let base = centered_page(&app, "Base screen");
    app.show(&base, full_screen());
    app.repaint();
    let before = app.frame.clone();

    let dialog = dialog_window(&app);
    let id = app.show(&dialog, Dialog::show_opts());
    app.repaint();
    assert!(app.px(10, 1600) < 255, "the scrim dims the base");

    app.close(id);
    app.repaint();
    assert_eq!(app.px(10, 1600), 255, "the scrim is gone");
    assert!(app.frame == before, "the frame is the same as before the dialog");
}

#[test]
fn closing_the_top_toast_keeps_the_dialog_below_it_dimmed_once() {
    let mut app = MiniApp::new();
    let base = centered_page(&app, "Base screen");
    app.show(&base, full_screen());
    let dialog = dialog_window(&app);
    app.show(&dialog, Dialog::show_opts());
    app.repaint();
    let with_dialog = app.frame.clone();

    let toast = toast_window(&app, "Saved");
    let id = app.show(
        &toast,
        ShowOpts {
            toast: true,
            ..ShowOpts::default()
        },
    );
    app.repaint();
    app.close(id);
    app.repaint();
    assert!(
        app.frame == with_dialog,
        "base, scrim and dialog are as before the toast"
    );
}

#[test]
fn a_lower_window_repaint_leaves_the_dialog_on_top() {
    let mut app = MiniApp::new();
    let base = centered_page(&app, "Scanning 1 of 10");
    app.show(&base, full_screen());
    app.repaint();
    let dialog = dialog_window(&app);
    app.show(&dialog, Dialog::show_opts());
    app.repaint();
    let lbl = base.find_rect("lbl").unwrap();
    let before = app.frame.clone();

    // The base window changes its label, as a scan progress event does.
    base.update::<Label, _>(&mut app.ctx, "lbl", |l, u| {
        l.set_text("Scanning 2 of 10");
        u.request_paint();
    });
    assert!(app.repaint());
    assert!(
        app.frame == before,
        "the dialog and its scrim are still on top of the base label"
    );
    assert!(
        app.refreshed_area() <= (lbl.w as i64 + 8) * (lbl.h as i64 + 8),
        "only the label area refreshes"
    );
}

#[test]
fn a_dialog_that_lays_out_again_does_not_darken_the_scrim() {
    let mut app = MiniApp::new();
    let base = centered_page(&app, "Base screen");
    app.show(&base, full_screen());
    let dialog = dialog_window(&app);
    let id = app.show(&dialog, Dialog::show_opts());
    app.repaint();
    let first = app.frame.clone();
    let scrim = app.px(10, 10);
    assert!(scrim < 255);

    dialog.invalidate();
    app.ctx.dirty.mark(id);
    app.repaint();
    assert_eq!(app.px(10, 10), scrim, "the scrim darkened on a full repaint");
    assert!(
        app.frame == first,
        "a full repaint of the dialog gives the same frame"
    );

    // Again, after a keyed layout request on a button.
    dialog.update::<kaia_ui::controls::Button, _>(&mut app.ctx, "dialog-button:dialog.ok", |b, u| {
        b.set_text("Okay");
        u.request_layout();
    });
    app.repaint();
    assert_eq!(app.px(10, 10), scrim, "the scrim darkened after a layout request");
    assert_eq!(app.px(10, 1600), first.get(10, 1600).unwrap().r);
}

/// A column of fixed boxes with a status label in the middle.
fn status_page(app: &MiniApp) -> Window {
    let t = &app.theme;
    let col = Column::new(vec![
        WidgetPod::new(SizedBox::new(None).height(300)),
        WidgetPod::new(Label::new("Idle", t.font.title.clone(), Tone::Ink)).with_key("status"),
        WidgetPod::new(SizedBox::new(None).height(300)),
        WidgetPod::new(Label::new("Footer", t.font.title.clone(), Tone::Ink)).with_key("footer"),
    ])
    .cross_align(CrossAlign::Start);
    app.window(Frame::new(WidgetPod::new(col)).fill(Tone::Surface))
}

#[test]
fn a_layout_request_refreshes_only_the_widgets_that_changed() {
    let mut app = MiniApp::new();
    let page = status_page(&app);
    app.show(&page, full_screen());
    app.repaint();
    assert_eq!(
        app.refreshed_area(),
        SCREEN_AREA,
        "the first paint refreshes the screen"
    );
    let status = page.find_rect("status").unwrap();
    let footer = page.find_rect("footer").unwrap();

    // Same size, new text: the label asks for layout and repaint.
    page.update::<Label, _>(&mut app.ctx, "status", |l, u| {
        l.set_text("Busy");
        u.request_layout();
    });
    assert!(app.repaint());
    assert!(app.refreshed_area() > 0);
    assert!(
        app.refreshed_area() <= (status.w as i64 + 600) * (status.h as i64 + 8),
        "refreshed {} px for a label of {}x{}",
        app.refreshed_area(),
        status.w,
        status.h
    );
    for r in &app.last {
        let g = r.region.expect("a region, not the whole screen");
        assert!(
            g.y + g.h <= footer.y,
            "the footer did not move, so it is not refreshed: {g:?}"
        );
    }
}

/// A column with a wrapping paragraph (keyed "status") above a footer label.
fn growing_page(app: &MiniApp, text: &str) -> Window {
    let t = &app.theme;
    let status = Paragraph::new(text, t.font.title.clone(), Tone::Ink);
    let col = Column::new(vec![
        WidgetPod::new(SizedBox::new(None).height(300)),
        WidgetPod::new(SizedBox::new(Some(WidgetPod::new(status).with_key("status"))).width(500)),
        WidgetPod::new(Label::new("Footer", t.font.title.clone(), Tone::Ink)).with_key("footer"),
    ])
    .cross_align(CrossAlign::Start);
    app.window(Frame::new(WidgetPod::new(col)).fill(Tone::Surface))
}

#[test]
fn a_widget_that_grows_refreshes_what_it_pushes_and_nothing_above() {
    let mut app = MiniApp::new();
    let page = growing_page(&app, "Short");
    app.show(&page, full_screen());
    app.repaint();
    let status = page.find_rect("status").unwrap();
    let footer = page.find_rect("footer").unwrap();

    let long = "A much longer status line that wraps onto a second and a third line";
    page.update::<Paragraph, _>(&mut app.ctx, "status", |p, u| {
        p.set_text(long);
        u.request_layout();
    });
    assert!(app.repaint());
    let moved_footer = page.find_rect("footer").unwrap();
    assert!(moved_footer.y > footer.y, "the footer moved down");
    for r in &app.last {
        let g = r.region.expect("a region");
        assert!(g.y >= status.y, "nothing above the status is refreshed: {g:?}");
    }
    let covers = |target: Rect| {
        app.last.iter().any(|r| {
            let g = r.region.unwrap();
            g.x <= target.x && g.y <= target.y && g.x + g.w >= target.right() && g.y + g.h >= target.bottom()
        })
    };
    assert!(covers(footer), "the footer's old place is refreshed");
    assert!(covers(moved_footer), "the footer's new place is refreshed");

    // The frame is what a fresh render of the new state gives.
    let fresh = kaia_ui::render(
        {
            let t = &app.theme;
            let status = Paragraph::new(long, t.font.title.clone(), Tone::Ink);
            let col = Column::new(vec![
                WidgetPod::new(SizedBox::new(None).height(300)),
                WidgetPod::new(SizedBox::new(Some(WidgetPod::new(status))).width(500)),
                WidgetPod::new(Label::new("Footer", t.font.title.clone(), Tone::Ink)),
            ])
            .cross_align(CrossAlign::Start);
            Frame::new(WidgetPod::new(col)).fill(Tone::Surface)
        },
        SCREEN,
        &app.theme,
        &app.assets,
    );
    assert!(app.frame == fresh, "the partial repaint matches a full render");
}
