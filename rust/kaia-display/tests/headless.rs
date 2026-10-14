//! Tests for the headless display and the scripted input source.

use std::time::Duration;

use kaia_display::{
    Display, DisplayError, HeadlessDisplay, InputEvent, InputSource, PanelSpec, Refresh, Rotation,
    ScriptedInput, Waveform,
};
use kaia_gfx::{Color, PixelFormat, Pixmap, Point, Rect, Size};
use kaia_input::{Key, KeyEvent, Phase, TouchEvent};

fn small_panel() -> PanelSpec {
    PanelSpec {
        size: Size::new(200, 100),
        dpi: 300,
        format: PixelFormat::Gray8,
    }
}

fn full_screen(size: Size) -> Rect {
    Rect::from_size(size)
}

#[test]
fn presents_two_frames_and_records_both_refreshes() {
    let mut display = HeadlessDisplay::new(small_panel());
    assert_eq!(display.size(), Size::new(200, 100));
    assert_eq!(display.format(), PixelFormat::Gray8);
    assert_eq!(display.dpi(), 300);
    assert!(display.refreshes().is_empty());

    let mut first = Pixmap::new(200, 100, PixelFormat::Gray8);
    first.fill_rect(Rect::new(0, 0, 200, 100), Color::gray(200));
    let full = Refresh::new(full_screen(display.size()), Waveform::Gc16, true);
    display.present(&first, &full).unwrap();
    assert_eq!(display.snapshot(), &first);

    let mut second = first.clone();
    second.fill_rect(Rect::new(10, 20, 30, 40), Color::BLACK);
    let ui = Refresh::new(Rect::new(10, 20, 30, 40), Waveform::Du, false);
    display.present(&second, &ui).unwrap();
    assert_eq!(display.snapshot(), &second);
    assert_eq!(display.refreshes(), &[full, ui]);
}

#[test]
fn records_the_dither_flag_of_each_refresh() {
    let mut display = HeadlessDisplay::new(small_panel());
    let frame = Pixmap::new(200, 100, PixelFormat::Gray8);
    let plain = Refresh::new(full_screen(display.size()), Waveform::Gc16, true);
    let dithered = plain.with_dither(true);
    display.present(&frame, &dithered).unwrap();
    display.present(&frame, &plain).unwrap();
    assert_eq!(display.refreshes(), &[dithered, plain]);
    assert!(display.refreshes()[0].dither);
    assert!(!display.refreshes()[1].dither);
}

#[test]
fn wait_idle_and_night_mode_are_no_ops_on_the_headless_display() {
    // They are default methods of the trait, so they are reachable through a box.
    let mut display: Box<dyn Display> = Box::new(HeadlessDisplay::new(small_panel()));
    display.wait_idle();
    display.set_night_mode(true);
    display.set_night_mode(false);
    assert_eq!(display.size(), Size::new(200, 100));
}

#[test]
fn rejected_frames_change_nothing() {
    let mut display = HeadlessDisplay::new(small_panel());
    let before = display.snapshot().clone();

    let wrong_size = Pixmap::new(100, 100, PixelFormat::Gray8);
    let refresh = Refresh::new(Rect::new(0, 0, 10, 10), Waveform::A2, false);
    assert!(matches!(
        display.present(&wrong_size, &refresh),
        Err(DisplayError::FrameSize { .. })
    ));

    let right_size = Pixmap::new(200, 100, PixelFormat::Gray8);
    let outside = Refresh::new(Rect::new(190, 90, 20, 20), Waveform::A2, false);
    assert!(matches!(
        display.present(&right_size, &outside),
        Err(DisplayError::RegionOutside { .. })
    ));

    assert_eq!(display.snapshot(), &before);
    assert!(display.refreshes().is_empty());
}

#[test]
fn rotation_swaps_the_logical_size_and_resets_the_snapshot() {
    let mut display = HeadlessDisplay::new(small_panel());
    display.set_rotation(Rotation::Deg90).unwrap();
    assert_eq!(display.rotation(), Rotation::Deg90);
    assert_eq!(display.size(), Size::new(100, 200));
    assert_eq!(display.snapshot().size(), Size::new(100, 200));

    let frame = Pixmap::new(100, 200, PixelFormat::Gray8);
    display
        .present(
            &frame,
            &Refresh::new(full_screen(display.size()), Waveform::Gc16, true),
        )
        .unwrap();
    assert_eq!(display.refreshes().len(), 1);
}

#[test]
fn save_png_writes_the_snapshot() {
    let display = HeadlessDisplay::new(small_panel());
    let dir = std::env::temp_dir().join(format!("kaia-display-headless-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("snapshot.png");
    display.save_png(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scripted_touch_script_yields_events_in_order() {
    let script = "# a tap, then a drag\ndown 0 500 800 0\nmove 0 540 800 100\nup 0 540 800 120\npoll 900\n";
    let mut input = ScriptedInput::from_script(script).unwrap();
    assert!(!input.is_done());

    let events = input.poll(None);
    assert_eq!(
        events,
        vec![
            InputEvent::Touch(TouchEvent {
                slot: 0,
                phase: Phase::Down,
                pos: Point::new(500, 800),
                time_us: 0,
            }),
            InputEvent::Touch(TouchEvent {
                slot: 0,
                phase: Phase::Move,
                pos: Point::new(540, 800),
                time_us: 100_000,
            }),
            InputEvent::Touch(TouchEvent {
                slot: 0,
                phase: Phase::Up,
                pos: Point::new(540, 800),
                time_us: 120_000,
            }),
        ]
    );
    assert!(input.is_done());
    assert!(input.poll(Some(Duration::from_millis(5))).is_empty());
}

#[test]
fn scripted_events_from_a_list_keep_their_order() {
    let key = |key, pressed| {
        InputEvent::Key(KeyEvent {
            key,
            pressed,
            time_us: 0,
        })
    };
    let events = vec![
        key(Key::RPgFwd, true),
        key(Key::RPgFwd, false),
        InputEvent::Resize(Size::new(1680, 1264)),
        InputEvent::Quit,
    ];
    let mut input = ScriptedInput::new(events.clone());
    assert_eq!(input.poll(None), events);
    assert!(input.is_done());
}

#[test]
fn bad_script_reports_the_line() {
    let err = ScriptedInput::from_script("down 0 1 1 0\nsideways 0 1 1 0\n").unwrap_err();
    assert_eq!(err.line, 2);
}

#[test]
fn partial_present_changes_only_the_requested_region() {
    let mut display = HeadlessDisplay::new(small_panel());
    let mut base = Pixmap::new(200, 100, PixelFormat::Gray8);
    base.fill_rect(Rect::new(0, 0, 200, 100), Color::gray(200));
    display
        .present(
            &base,
            &Refresh::new(full_screen(display.size()), Waveform::Gc16, true),
        )
        .unwrap();

    // The app changes two areas but marks only the first one dirty. The panel must
    // keep the old pixels in the second area.
    let mut next = base.clone();
    next.fill_rect(Rect::new(10, 10, 20, 20), Color::BLACK);
    next.fill_rect(Rect::new(150, 60, 10, 10), Color::BLACK);
    let dirty = Rect::new(10, 10, 20, 20);
    display
        .present(&next, &Refresh::new(dirty, Waveform::Du, false))
        .unwrap();

    let mut expected = base.clone();
    expected.fill_rect(dirty, Color::BLACK);
    assert_eq!(display.snapshot(), &expected);
    assert_eq!(
        display.snapshot().data()[60 * 200 + 155],
        200,
        "the unmarked area keeps its pixels"
    );
    assert_eq!(display.refreshes().len(), 2);
}

#[test]
fn partial_present_copies_a_region_that_touches_the_edge() {
    let mut display = HeadlessDisplay::new(small_panel());
    let mut frame = Pixmap::new(200, 100, PixelFormat::Gray8);
    frame.fill_rect(Rect::new(0, 0, 200, 100), Color::gray(90));
    display
        .present(
            &frame,
            &Refresh::new(full_screen(display.size()), Waveform::Gc16, true),
        )
        .unwrap();

    // The region ends at the bottom right corner of the screen.
    let mut edge = frame.clone();
    edge.fill_rect(Rect::new(180, 80, 20, 20), Color::WHITE);
    display
        .present(
            &edge,
            &Refresh::new(Rect::new(180, 80, 20, 20), Waveform::Glr16, false),
        )
        .unwrap();
    assert_eq!(display.snapshot().data()[85 * 200 + 185], 255);
    assert_eq!(display.snapshot().data()[85 * 200 + 175], 90);
}

#[test]
fn full_present_replaces_the_whole_snapshot() {
    let mut display = HeadlessDisplay::new(small_panel());
    let mut first = Pixmap::new(200, 100, PixelFormat::Gray8);
    first.fill_rect(Rect::new(0, 0, 200, 100), Color::gray(40));
    display
        .present(
            &first,
            &Refresh::new(full_screen(display.size()), Waveform::Gc16, true),
        )
        .unwrap();

    // Every pixel changes, but the refresh asks for a partial region. The full-screen
    // present that follows must copy all of it.
    let mut second = Pixmap::new(200, 100, PixelFormat::Gray8);
    second.fill_rect(Rect::new(0, 0, 200, 100), Color::gray(220));
    display
        .present(
            &second,
            &Refresh::new(full_screen(display.size()), Waveform::Gc16, true),
        )
        .unwrap();
    assert_eq!(display.snapshot(), &second);
}

#[test]
fn empty_region_copies_nothing() {
    let mut display = HeadlessDisplay::new(small_panel());
    let before = display.snapshot().clone();
    let mut frame = Pixmap::new(200, 100, PixelFormat::Gray8);
    frame.fill_rect(Rect::new(0, 0, 200, 100), Color::BLACK);
    let empty = Refresh::new(Rect::new(5, 5, 0, 0), Waveform::Du, false);
    display.present(&frame, &empty).unwrap();
    assert_eq!(display.snapshot(), &before);
    assert_eq!(display.refreshes(), &[empty]);
}

#[test]
fn first_present_after_rotation_copies_the_whole_frame() {
    let mut display = HeadlessDisplay::new(small_panel());
    display.set_rotation(Rotation::Deg90).unwrap();

    // A small region after rotation still shows the whole new frame, because the
    // panel has not been drawn in the new orientation yet.
    let mut frame = Pixmap::new(100, 200, PixelFormat::Gray8);
    frame.fill_rect(Rect::new(0, 0, 100, 200), Color::gray(120));
    display
        .present(&frame, &Refresh::new(Rect::new(0, 0, 4, 4), Waveform::Du, false))
        .unwrap();
    assert_eq!(display.snapshot(), &frame);

    // After that present, a partial region updates only its own pixels again.
    let mut next = frame.clone();
    next.fill_rect(Rect::new(50, 100, 10, 10), Color::BLACK);
    next.fill_rect(Rect::new(90, 190, 10, 10), Color::BLACK);
    display
        .present(
            &next,
            &Refresh::new(Rect::new(50, 100, 10, 10), Waveform::Du, false),
        )
        .unwrap();
    assert_eq!(display.snapshot().data()[195 * 100 + 95], 120);
    assert_eq!(display.snapshot().data()[105 * 100 + 55], 0);
}

#[test]
fn rotating_back_and_forth_keeps_full_presents_working() {
    let mut display = HeadlessDisplay::new(small_panel());
    display.set_rotation(Rotation::Deg270).unwrap();
    let frame = Pixmap::new(100, 200, PixelFormat::Gray8);
    display
        .present(
            &frame,
            &Refresh::new(full_screen(display.size()), Waveform::Gc16, true),
        )
        .unwrap();
    display.set_rotation(Rotation::Deg0).unwrap();
    let back = Pixmap::new(200, 100, PixelFormat::Gray8);
    display
        .present(
            &back,
            &Refresh::new(full_screen(display.size()), Waveform::Gc16, true),
        )
        .unwrap();
    assert_eq!(display.rotation(), Rotation::Deg0);
    assert_eq!(display.snapshot(), &back);
    assert_eq!(display.refreshes().len(), 2);
}
