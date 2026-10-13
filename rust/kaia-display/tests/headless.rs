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
