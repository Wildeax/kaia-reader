//! Opens the desktop window at Libra Colour size and draws a screen with a tap counter.
//!
//! Run with `cargo run -p kaia-display --example hello_window`. Tap or click to add to the
//! counter. Escape or closing the window quits.
//!
//! Environment variables:
//!
//! - `KAIA_EXIT_AFTER_MS`: quit after this many milliseconds, for scripted runs.
//! - `KAIA_EINK_SIM`: show the screen as an E Ink panel (16 gray levels, flashes invert).

use std::error::Error;
use std::time::{Duration, Instant};

use kaia_display::{
    DesktopDisplay, DesktopOptions, Display, InputEvent, InputSource, PanelSpec, Refresh, Waveform,
};
use kaia_gfx::draw;
use kaia_gfx::text::{FontSystem, TextStyle, NOTO_FONT_PREFIXES};
use kaia_gfx::{Color, Pixmap, Point, Rect};
use kaia_input::{Key, Phase};

const FONT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/fonts/noto");
const MARGIN: i32 = 96;
const CONTENT_W: u32 = 1072;
const COUNTER_RECT: Rect = Rect::new(MARGIN, 900, CONTENT_W, 120);

fn draw_static(fonts: &mut FontSystem, pm: &mut Pixmap) {
    let title = TextStyle {
        weight: 700,
        ..TextStyle::new(72.0)
    };
    let layout = fonts.layout("Kaia display", &title, CONTENT_W, Some(1), true);
    fonts.draw_layout(pm, &layout, Point::new(MARGIN, 160));

    draw::hline(pm, MARGIN, 280, CONTENT_W, 2, Color::BLACK);

    let body = TextStyle::new(40.0);
    let text = "This is the desktop display backend. Tap or click to count. \
                Press Escape or close the window to quit.";
    let layout = fonts.layout(text, &body, CONTENT_W, Some(6), true);
    fonts.draw_layout(pm, &layout, Point::new(MARGIN, 320));
}

fn draw_counter(fonts: &mut FontSystem, pm: &mut Pixmap, taps: u32) {
    pm.fill_rect(COUNTER_RECT, Color::WHITE);
    let style = TextStyle {
        weight: 700,
        ..TextStyle::new(64.0)
    };
    let layout = fonts.layout(&format!("Taps: {taps}"), &style, CONTENT_W, Some(1), false);
    fonts.draw_layout(pm, &layout, Point::new(COUNTER_RECT.x, COUNTER_RECT.y));
}

fn main() -> Result<(), Box<dyn Error>> {
    let deadline = match std::env::var("KAIA_EXIT_AFTER_MS") {
        Ok(ms) => Some(Instant::now() + Duration::from_millis(ms.parse()?)),
        Err(_) => None,
    };
    let options = DesktopOptions {
        eink_sim: std::env::var_os("KAIA_EINK_SIM").is_some(),
        ..DesktopOptions::default()
    };

    let mut display = DesktopDisplay::open(PanelSpec::LIBRA_COLOUR, options)?;
    let mut fonts = FontSystem::load(&[FONT_DIR], NOTO_FONT_PREFIXES)?;
    let screen = display.size();
    let mut frame = Pixmap::new(screen.w, screen.h, display.format());
    let mut taps = 0;

    draw_static(&mut fonts, &mut frame);
    draw_counter(&mut fonts, &mut frame, taps);
    display.present(
        &frame,
        &Refresh::new(Rect::from_size(screen), Waveform::Gc16, true),
    )?;

    loop {
        let timeout = match deadline {
            Some(at) => match at.checked_duration_since(Instant::now()) {
                Some(left) => Some(left),
                None => break,
            },
            None => None,
        };
        for event in display.poll(timeout) {
            match event {
                InputEvent::Touch(touch) if touch.phase == Phase::Down => {
                    taps += 1;
                    draw_counter(&mut fonts, &mut frame, taps);
                    display.present(&frame, &Refresh::new(COUNTER_RECT, Waveform::Du, false))?;
                }
                InputEvent::Key(key) if key.pressed && key.key == Key::Back => return Ok(()),
                InputEvent::Quit => return Ok(()),
                _ => {}
            }
        }
    }
    Ok(())
}
