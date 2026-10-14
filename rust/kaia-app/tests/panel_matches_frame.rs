//! The panel matches the frame after each step of the main flows.
//!
//! The headless display keeps what the panel shows. A partial present copies only
//! its refresh region into the panel, as the e-ink panel does. The app's frame holds
//! everything the app painted. A widget that paints a region without marking it dirty
//! leaves stale pixels on the panel while the frame stays correct. The golden tests
//! compare the frame, so they miss this.
//!
//! The test runs every flow in one app and compares the panel with the frame after
//! each step. A failing step does not stop the run. Its frame and panel images go to
//! `/tmp/kaia-panel-matches-frame`, and the test lists every failure at the end.

mod common;

use std::fs;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use kaia_app::surfaces::settings::commands;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_display::InputEvent;
use kaia_gfx::{png, PixelFormat, Pixmap, Point, Rect};
use kaia_input::{Key, Phase, TouchEvent};
use kaia_platform::{device::Model, platform::Kind};

/// Where the images of failing steps go.
const OUT_DIR: &str = "/tmp/kaia-panel-matches-frame";

/// Steps run and the failures found. A failure is a panel that differs from the
/// frame, or an action that panicked.
#[derive(Default)]
struct Run {
    steps: usize,
    failures: Vec<String>,
    skipped: Vec<String>,
}

impl Run {
    /// Runs `action`, lets the app settle, then compares the panel with the frame.
    /// A panic in `action` is recorded as a failure of this step.
    fn step(
        &mut self,
        app: &mut App<HeadlessBackend>,
        name: &str,
        action: impl FnOnce(&mut App<HeadlessBackend>),
    ) {
        self.steps += 1;
        let number = self.steps;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            action(app);
            settle(app);
        }));
        if let Err(payload) = outcome {
            let screen = format!("{:?}", app.top_route());
            self.failures.push(format!(
                "step {number} `{name}` (screen {screen}): the step panicked: {}",
                panic_text(payload.as_ref())
            ));
            return;
        }
        self.check(app, number, name);
    }

    fn skip(&mut self, name: &str, why: &str) {
        self.skipped.push(format!("{name}: {why}"));
    }

    fn check(&mut self, app: &App<HeadlessBackend>, number: usize, name: &str) {
        let screen = format!("{:?}", app.top_route());
        let expected = expected_for_panel(app);
        let panel = app.backend().display().snapshot();
        if expected.size() != panel.size() {
            self.failures.push(format!(
                "step {number} `{name}` (screen {screen}): panel is {}x{}, frame is {}x{}",
                panel.width(),
                panel.height(),
                expected.width(),
                expected.height()
            ));
            return;
        }
        let Some((rect, count)) = differing(&expected, panel) else {
            return;
        };
        let (frame_png, panel_png) = save_pair(number, name, &expected, panel);
        self.failures.push(format!(
            "step {number} `{name}` (screen {screen}): {count} pixels differ in x={} y={} w={} h={}. \
             Frame: {}. Panel: {}.",
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            frame_png.display(),
            panel_png.display()
        ));
    }
}

/// Lets the loop finish the work the last step started: the library scan, covers,
/// the reader engine, and the repaint that follows them.
fn settle(app: &mut App<HeadlessBackend>) {
    wait_for(app, "the scan, the covers and the reader to finish", |a| {
        !a.scan_running() && !a.reader_busy() && !kaia_app::covers::busy()
    });
    app.process(Vec::new()).unwrap();
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "no message".to_string())
}

/// The frame as the panel should show it. The headless display receives a luma copy
/// when the color is off, so this applies the same conversion as the app's repaint.
fn expected_for_panel(app: &App<HeadlessBackend>) -> Pixmap {
    let frame = app.frame().clone();
    let services = app.services();
    let color = services.theme.color_panel
        && services
            .settings
            .read("display.color_rendering")
            .as_bool()
            .unwrap_or(true);
    if color || frame.format() != PixelFormat::Rgb24 {
        return frame;
    }
    let mut gray = frame;
    for pixel in gray.data_mut().as_chunks_mut::<3>().0 {
        let luma =
            ((u32::from(pixel[0]) * 77 + u32::from(pixel[1]) * 150 + u32::from(pixel[2]) * 29) >> 8) as u8;
        pixel.fill(luma);
    }
    gray
}

/// The bounds of the pixels that differ, and how many pixels differ. The panel and the
/// frame have the same size and format.
fn differing(expected: &Pixmap, actual: &Pixmap) -> Option<(Rect, usize)> {
    let bpp = expected.format().bytes_per_pixel();
    let stride = expected.stride();
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0usize, 0usize);
    let mut count = 0usize;
    let rows = expected.data().chunks(stride).zip(actual.data().chunks(stride));
    for (y, (row_e, row_a)) in rows.enumerate() {
        for (x, (pe, pa)) in row_e.chunks(bpp).zip(row_a.chunks(bpp)).enumerate() {
            if pe != pa {
                count += 1;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }
    (count > 0).then(|| {
        let rect = Rect::new(
            left as i32,
            top as i32,
            (right - left) as u32,
            (bottom - top) as u32,
        );
        (rect, count)
    })
}

/// Writes the frame and the panel of a failing step. Returns the two file paths.
fn save_pair(number: usize, name: &str, frame: &Pixmap, panel: &Pixmap) -> (PathBuf, PathBuf) {
    fs::create_dir_all(OUT_DIR).unwrap();
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let base = format!("{number:02}-{slug}");
    let frame_path = Path::new(OUT_DIR).join(format!("{base}-frame.png"));
    let panel_path = Path::new(OUT_DIR).join(format!("{base}-panel.png"));
    png::write_pixmap(frame, &frame_path).unwrap();
    png::write_pixmap(panel, &panel_path).unwrap();
    (frame_path, panel_path)
}

/// The centre of the screen, where a tap opens or closes the reader overlay.
fn centre(app: &App<HeadlessBackend>) -> Point {
    Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2)
}

/// Opens the overlay with a centre tap.
fn open_overlay(app: &mut App<HeadlessBackend>) {
    let p = centre(app);
    app.tap(p).unwrap();
}

/// A point on a word of the text page that `turn_to_text` shows.
fn word_point() -> Point {
    Point::new(200, 120)
}

/// A hold at `p`. The touch goes down, the hold timer runs out in real time, and the
/// touch lifts. The gesture recognizer reads the hold from the clock, so the test
/// sleeps.
fn hold_at(app: &mut App<HeadlessBackend>, p: Point) {
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: 1,
    })])
    .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    app.iterate(Some(Duration::from_millis(1))).unwrap();
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: 2_000_000,
    })])
    .unwrap();
    wait_reader(app);
}

/// Turns pages until the text page of juliet, which starts after the cover.
fn turn_to_text(app: &mut App<HeadlessBackend>) {
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        wait_reader(app);
    }
}

/// Leaves the reader and the library until the app is on Home.
fn back_to_home(app: &mut App<HeadlessBackend>) {
    for _ in 0..8 {
        if app.top_route() == Some(&Route::Home) {
            return;
        }
        app.key(Key::Back).unwrap();
        wait_reader(app);
    }
    panic!("Back did not reach Home, on {:?}", app.top_route());
}

/// Taps the Previous or Next button of the Settings root list. The pager draws the
/// buttons in a strip above the nav bar, on the two edges, and gives them no widget key.
fn settings_pager_tap(app: &mut App<HeadlessBackend>, next: bool) {
    let nav = app.find_rect("nav-bar").expect("the navigation bar");
    let (touch, side) = {
        let t = &app.services().theme;
        (t.touch_target as i32, t.space.sm as i32)
    };
    let x = if next {
        nav.x + nav.w as i32 - touch / 2
    } else {
        nav.x + touch / 2
    };
    app.tap(Point::new(x, nav.y - side - touch / 2)).unwrap();
}

/// Opens the row `key` on the Settings root list. The list has two pages on the
/// screen, so the row is on page 1 or page 2. Previous goes to page 1 first.
fn settings_open_row(app: &mut App<HeadlessBackend>, key: &str) {
    for _ in 0..3 {
        settings_pager_tap(app, false);
    }
    if app.find_rect(key).is_none() {
        settings_pager_tap(app, true);
    }
    tap_key(app, key);
}

/// Runs `action` and checks that the boolean setting `key` flips.
fn toggle_changes(app: &mut App<HeadlessBackend>, key: &str, action: impl FnOnce(&mut App<HeadlessBackend>)) {
    let before = app.services().settings.read(key).as_bool();
    action(app);
    wait_reader(app);
    let after = app.services().settings.read(key).as_bool();
    assert!(
        after.is_some() && after != before,
        "{key} did not flip: {before:?} then {after:?}"
    );
}

/// Taps the plus end of the stepper with `key`.
fn tap_stepper_plus(app: &mut App<HeadlessBackend>, key: &str) {
    let rect = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no stepper {key:?} on the top window"));
    let touch = app.services().theme.touch_target as i32;
    let plus = Point::new(rect.x + rect.w as i32 - touch / 2, rect.y + rect.h as i32 / 2);
    app.tap(plus).unwrap();
}

#[test]
fn differing_reports_the_bounds_of_the_changed_pixels() {
    let mut a = Pixmap::new(10, 8, PixelFormat::Rgb24);
    let b = a.clone();
    assert_eq!(differing(&a, &b), None);
    a.put(2, 3, kaia_gfx::Color::BLACK);
    a.put(5, 6, kaia_gfx::Color::BLACK);
    assert_eq!(differing(&a, &b), Some((Rect::new(2, 3, 4, 4), 2)));
}

#[test]
fn panel_matches_frame_after_each_step() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _ = fs::remove_dir_all(OUT_DIR);
    let dir = scratch("panel-matches-frame");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    let mut run = Run::default();

    // Home and Library.
    run.step(&mut app, "home: start", |app| app.start().unwrap());
    run.step(&mut app, "library: Books in covers", |app| {
        tap_key(app, "library")
    });
    run.step(&mut app, "library: Books in list", |app| {
        tap_key(app, "library-view-list")
    });
    run.step(&mut app, "library: Books in covers again", |app| {
        tap_key(app, "library-view-covers")
    });
    for tab in ["folders", "collections", "series", "authors"] {
        run.step(&mut app, &format!("library: {tab} tab"), |app| {
            tap_key(app, &format!("library-view-{tab}"))
        });
    }
    run.step(&mut app, "library: Books tab again", |app| {
        tap_key(app, "library-view-books")
    });

    // The reader: pages, overlay, sheets.
    run.step(&mut app, "reader: open juliet.epub", |app| {
        open_from_library(app, "juliet.epub")
    });
    for n in 1..=8 {
        run.step(&mut app, &format!("reader: page forward {n}"), |app| {
            app.key(Key::RPgFwd).unwrap();
        });
    }
    run.step(&mut app, "reader: page back", |app| {
        app.key(Key::LPgBack).unwrap();
    });
    run.step(&mut app, "reader: overlay open", open_overlay);
    run.step(&mut app, "reader: overlay closed", open_overlay);
    run.step(&mut app, "reader: overlay open again", open_overlay);
    run.step(&mut app, "reader: Text and layout sheet", |app| {
        tap_key(app, "overlay-layout")
    });
    run.step(&mut app, "reader: font list", |app| tap_key(app, "layout-font"));
    run.step(&mut app, "reader: font list closed by Back", |app| {
        app.key(Key::Back).unwrap();
    });
    run.step(&mut app, "reader: font list opened again", |app| {
        tap_key(app, "layout-font")
    });
    run.step(&mut app, "reader: font list closed by its close button", |app| {
        tap_key(app, "panel-close")
    });
    run.step(&mut app, "reader: Text and layout sheet closed by Back", |app| {
        app.key(Key::Back).unwrap();
    });
    run.step(&mut app, "reader: overlay closed after the sheet", open_overlay);
    run.step(&mut app, "reader: overlay open for More", open_overlay);
    run.step(&mut app, "reader: More sheet", |app| tap_key(app, "overlay-more"));
    run.step(&mut app, "reader: Header and footer sheet", |app| {
        tap_key(app, "more-footer")
    });
    run.step(&mut app, "reader: Header and footer sheet closed", |app| {
        tap_key(app, "panel-close")
    });
    run.step(&mut app, "reader: closed the sheets and the overlay", |app| {
        if app.find_rect("overlay-more").is_some() {
            app.key(Key::Back).unwrap();
        }
        if matches!(app.top_route(), Some(Route::Overlay)) {
            open_overlay(app);
        }
    });

    // Selection, color, More and shade.
    run.step(&mut app, "reader: turn to text", turn_to_text);
    run.step(&mut app, "reader: word held, selection panel", |app| {
        hold_at(app, word_point())
    });
    run.step(&mut app, "reader: yellow picked", |app| {
        tap_key(app, "color-yellow")
    });
    run.step(&mut app, "reader: word held after the highlight", |app| {
        hold_at(app, word_point())
    });
    run.step(&mut app, "reader: More from the selection panel", |app| {
        tap_key(app, "sel-more")
    });
    run.step(&mut app, "reader: shade stepper plus", |app| {
        let before = app.services().settings.read("reading.highlight_shade").as_f64();
        tap_stepper_plus(app, "sel-shade");
        let after = app.services().settings.read("reading.highlight_shade").as_f64();
        assert_ne!(after, before, "the plus button changes the shade");
    });
    run.step(&mut app, "reader: More closed", |app| tap_key(app, "panel-close"));

    // Dictionary.
    run.step(&mut app, "reader: word held for the dictionary", |app| {
        hold_at(app, word_point())
    });
    run.step(&mut app, "reader: dictionary sheet", |app| {
        tap_key(app, "sel-dict")
    });
    run.step(&mut app, "reader: dictionary sheet closed", |app| {
        tap_key(app, "panel-close")
    });
    run.step(&mut app, "reader: back to Home", back_to_home);

    // Settings.
    run.step(&mut app, "settings: root", |app| tap_key(app, "nav-settings"));
    for name in commands::SECTIONS {
        run.step(&mut app, &format!("settings: {name} section"), |app| {
            settings_open_row(app, &format!("settings-{name}"))
        });
        run.step(&mut app, &format!("settings: back from {name}"), |app| {
            app.key(Key::Back).unwrap();
        });
    }
    run.step(&mut app, "settings: display section", |app| {
        settings_open_row(app, "settings-display")
    });
    run.step(&mut app, "settings: night mode on", |app| {
        toggle_changes(app, "display.night_mode", |app| {
            tap_key(app, "set-display.night_mode")
        })
    });
    run.step(&mut app, "settings: night mode off", |app| {
        toggle_changes(app, "display.night_mode", |app| {
            tap_key(app, "set-display.night_mode")
        })
    });
    run.step(&mut app, "settings: display back", |app| {
        app.key(Key::Back).unwrap();
    });
    run.step(&mut app, "settings: reading section", |app| {
        settings_open_row(app, "settings-reading")
    });
    run.step(&mut app, "settings: font size up", |app| {
        let before = app.services().settings.read("reading.font_size").as_f64();
        tap_stepper_plus(app, "set-reading.font_size");
        let after = app.services().settings.read("reading.font_size").as_f64();
        assert_ne!(after, before, "the plus button changes the font size");
    });
    run.step(&mut app, "settings: reading back", |app| {
        app.key(Key::Back).unwrap();
    });

    // Quick settings.
    // The light sliders work on a Kobo only, so the test platform reports one.
    run.step(&mut app, "quick settings: open", |app| {
        app.services_mut().platform.kind = Kind::Kobo(Model::LibraColour);
        app.command("settings:quick:open", &[]);
    });
    run.step(&mut app, "quick settings: warmth", |app| {
        let before = app.services().platform.frontlight.warmth();
        let rect = app.find_rect("quick-warmth").expect("the warmth slider");
        let p = Point::new(rect.x + rect.w as i32 * 3 / 4, rect.y + rect.h as i32 / 2);
        app.tap(p).unwrap();
        let after = app.services().platform.frontlight.warmth();
        assert_ne!(after, before, "the slider changes the warmth");
    });
    run.step(&mut app, "quick settings: closed", |app| {
        app.key(Key::Back).unwrap();
    });

    // Import screens. The Kobo and KOReader screens need data this repository does
    // not carry as a fixture, so they do not run.
    run.skip(
        "settings: Import from Kobo",
        "no Kobo database fixture in tests/fixtures",
    );
    run.skip(
        "settings: Import from KOReader",
        "no KOReader folder fixture in tests/fixtures",
    );

    eprintln!(
        "panel_matches_frame: {} steps run, {} failed, {} skipped",
        run.steps,
        run.failures.len(),
        run.skipped.len()
    );
    for skip in &run.skipped {
        eprintln!("skipped: {skip}");
    }
    assert!(
        run.failures.is_empty(),
        "{} of {} steps left the panel different from the frame or failed:\n{}",
        run.failures.len(),
        run.steps,
        run.failures.join("\n")
    );
}
