//! Draws a sample E Ink screen and compares it with a golden PNG.
//!
//! Regenerate the golden file with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-gfx --test sample_screen`.

use std::path::Path;

use kaia_gfx::dither::{self, EINK_LEVELS};
use kaia_gfx::draw;
use kaia_gfx::geom::{Point, Rect, Size};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::image::render_svg;
use kaia_gfx::text::{FontSystem, TextLayout, TextStyle};
use kaia_gfx::{Color, PixelFormat, Pixmap};

const FONT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/fonts/noto");
const ICON_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/icons/mdlight");
const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/sample_screen.png");

const SCREEN: Size = Size::new(1264, 1680);
const MARGIN: i32 = 96;
const CONTENT_W: u32 = 1072;

fn draw_text(
    fs: &mut FontSystem,
    pm: &mut Pixmap,
    text: &str,
    style: &TextStyle,
    max_lines: Option<usize>,
    origin: Point,
) -> TextLayout {
    let layout = fs.layout(text, style, CONTENT_W, max_lines, true);
    fs.draw_layout(pm, &layout, origin);
    layout
}

fn serif(size: f32, weight: u16) -> TextStyle {
    TextStyle {
        family: "Noto Serif".to_string(),
        weight,
        ..TextStyle::new(size)
    }
}

fn sans(size: f32, weight: u16) -> TextStyle {
    TextStyle {
        weight,
        ..TextStyle::new(size)
    }
}

#[test]
fn sample_screen_matches_golden() {
    let mut fs = FontSystem::load(&[FONT_DIR], &["NotoSans-", "NotoSerif-", "NotoNaskhArabic-"])
        .expect("Noto fonts in resources/fonts/noto");
    let mut pm = Pixmap::new(SCREEN.w, SCREEN.h, PixelFormat::Gray8);
    let ink = Color::gray(0);

    // Title and rule.
    let title = draw_text(
        &mut fs,
        &mut pm,
        "Kaia Reader",
        &serif(88.0, 700),
        None,
        Point::new(MARGIN, 110),
    );
    assert_eq!(title.lines.len(), 1);
    draw::hline(&mut pm, MARGIN, 250, CONTENT_W, 3, ink);

    // Card with an icon and three lines of text.
    let card = Rect::new(MARGIN, 300, CONTENT_W, 300);
    draw::fill_rrect(&mut pm, card, 36, Color::gray(238));
    draw::border(&mut pm, card, 36, 3, Color::gray(40));
    let icon_size = Size::new(120, 120);
    let svg = std::fs::read(Path::new(ICON_DIR).join("book.opened.svg")).expect("icon file");
    let icon = render_svg(&svg, icon_size).expect("icon renders");
    draw::blit_alpha(
        &mut pm,
        &icon,
        icon.bounds(),
        Point::new(card.x + 60, card.y + 90),
    );
    let card_text_x = MARGIN + 240;
    draw_text(
        &mut fs,
        &mut pm,
        "Continue reading",
        &sans(48.0, 700),
        Some(1),
        Point::new(card_text_x, 340),
    );
    draw_text(
        &mut fs,
        &mut pm,
        "Chapter 3: The river crossing",
        &sans(36.0, 400),
        Some(1),
        Point::new(card_text_x, 410),
    );
    draw_text(
        &mut fs,
        &mut pm,
        "Page 42 of 311",
        &sans(32.0, 400),
        Some(1),
        Point::new(card_text_x, 468),
    );

    // Mixed English and Arabic paragraph, wrapped at the content width.
    let body = "Kaia keeps your place in every book, and notes stay with the text. \
        Arabic such as مرحبا بكم في كايا reads right to left inside the same paragraph. \
        هذه فقرة عربية تمتد على أكثر من سطر لاختبار الالتفاف عند حدود العرض المحدد للصفحة، \
        والسطر الأخير يبقى في مكانه.";
    let paragraph = TextStyle {
        line_height: 62.0,
        ..serif(40.0, 400)
    };
    let para = draw_text(&mut fs, &mut pm, body, &paragraph, None, Point::new(MARGIN, 680));
    assert!(
        para.lines.len() >= 4,
        "paragraph wraps to {} lines",
        para.lines.len()
    );
    assert!(para.lines.iter().all(|l| l.width <= CONTENT_W as f32 + 0.5));

    // Progress bar with a marker at 62 percent.
    draw_text(
        &mut fs,
        &mut pm,
        "Progress 62 %",
        &sans(36.0, 400),
        Some(1),
        Point::new(MARGIN, 1230),
    );
    let track = Rect::new(MARGIN, 1300, CONTENT_W, 36);
    draw::fill_rrect(&mut pm, track, 18, Color::gray(220));
    draw::border(&mut pm, track, 18, 2, Color::gray(60));
    let filled = (CONTENT_W as f32 * 0.62) as u32;
    draw::fill_rrect(&mut pm, Rect::new(MARGIN, 1300, filled, 36), 18, ink);
    let marker_x = (MARGIN + filled as i32) as f32;
    draw::fill_circle(&mut pm, marker_x, 1318.0, 28.0, Color::WHITE);
    draw::stroke_circle(&mut pm, marker_x, 1318.0, 28.0, 4.0, ink);

    // Dither a gray ramp to the 16 E Ink levels so the test covers that path too.
    let mut ramp = Pixmap::new(CONTENT_W, 60, PixelFormat::Gray8);
    for x in 0..CONTENT_W {
        let v = (x * 255 / (CONTENT_W - 1)) as u8;
        draw::vline(&mut ramp, x as i32, 0, 60, 1, Color::gray(v));
    }
    let ramp = dither::ordered(&ramp, EINK_LEVELS);
    draw::blit(&mut pm, &ramp, ramp.bounds(), Point::new(MARGIN, 1400));

    assert_matches_golden(&pm, Path::new(GOLDEN), 2);
}
