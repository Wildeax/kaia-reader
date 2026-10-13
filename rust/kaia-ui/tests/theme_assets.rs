//! Theme scaling, palette rules, and the icon cache.

mod common;

use std::rc::Rc;

use common::*;
use kaia_gfx::{Color, PixelFormat, Size};
use kaia_ui::assets::{AssetError, Assets, IconError};
use kaia_ui::theme::{Scale, Theme, Tone};

#[test]
fn dp_follows_koreader_scale_by_size() {
    // KOReader: Screen:scaleBySize. Without an override the scale is min(w, h) / 600 on both factors.
    let plain = Scale {
        width: 1264,
        height: 1680,
        dpi: None,
    };
    assert_eq!(plain.dp(1), 3, "ceil(2.1066)");
    assert_eq!(plain.dp(50), 106, "the 9 mm touch target at 300 PPI in DESIGN.md");
    assert_eq!(plain.dp(0), 0);

    // With a 300 DPI override the DPI factor is 300 / 160.
    let dpi = Scale {
        width: 1264,
        height: 1680,
        dpi: Some(300),
    };
    assert_eq!(dpi.dp(34), 68, "ceil(34 * (2.1066 + 1.875) / 2)");
    assert_eq!(dpi.dp(50), 100);
}

#[test]
fn theme_tokens_are_scaled_and_match_design_md() {
    let t = theme(false);
    assert_eq!(t.space.md, t.scale.dp(16));
    assert_eq!(t.radius.card, t.scale.dp(12));
    assert_eq!(t.line.progress, t.scale.dp(6));
    assert_eq!(t.font.display.family, "Noto Serif");
    assert_eq!(t.font.display.weight, 700);
    assert_eq!(t.font.body.family, "Noto Sans");
    assert_eq!(t.font.body.size, t.scale.dp(20) as f32);
}

#[test]
fn the_accent_is_ink_on_grayscale_and_green_on_color() {
    let gray = Theme::new(
        Scale {
            width: 1264,
            height: 1680,
            dpi: None,
        },
        false,
    );
    assert_eq!(gray.color(Tone::Accent), gray.color(Tone::Ink));
    assert_eq!(gray.pixel_format(), PixelFormat::Gray8);

    let color = Theme::new(
        Scale {
            width: 1264,
            height: 1680,
            dpi: None,
        },
        true,
    );
    assert_eq!(color.color(Tone::Accent), Color::rgb(0x1F, 0x6F, 0x5C));
    assert_eq!(color.pixel_format(), PixelFormat::Rgb24);
}

#[test]
fn grays_match_the_design_tokens() {
    let t = theme(false);
    assert_eq!(t.color(Tone::Ink), Color::gray(0x00));
    assert_eq!(t.color(Tone::InkSecondary), Color::gray(0x55));
    assert_eq!(t.color(Tone::InkMuted), Color::gray(0x88));
    assert_eq!(t.color(Tone::Divider), Color::gray(0xCC));
    assert_eq!(t.color(Tone::Surface), Color::gray(0xFF));
    assert_eq!(t.color(Tone::SurfaceAlt), Color::gray(0xEE));
}

#[test]
fn icons_are_cached_by_name_size_and_color() {
    let shared = assets();
    let mut a = shared.borrow_mut();
    let ink = Color::gray(0);
    let first = a.icons.get("home", Size::new(64, 64), ink).unwrap();
    let again = a.icons.get("home", Size::new(64, 64), ink).unwrap();
    assert!(Rc::ptr_eq(&first, &again), "same key, same mask");
    assert_eq!(a.icons.len(), 1);

    let bigger = a.icons.get("home", Size::new(96, 96), ink).unwrap();
    assert!(!Rc::ptr_eq(&first, &bigger));
    let tinted = a.icons.get("home", Size::new(64, 64), Color::gray(0x55)).unwrap();
    assert!(!Rc::ptr_eq(&first, &tinted));
    assert_eq!(a.icons.len(), 3);
    assert_eq!(first.width(), 64);
    assert_eq!(bigger.width(), 96);
}

#[test]
fn an_icon_mask_keeps_the_shape_and_takes_the_tint() {
    let shared = assets();
    let mut a = shared.borrow_mut();
    let mask = a
        .icons
        .get("home", Size::new(64, 64), Color::rgb(0x1F, 0x6F, 0x5C))
        .unwrap();
    let mut drawn = 0;
    for y in 0..64 {
        for x in 0..64 {
            if let Some([r, g, b, alpha]) = mask.get(x, y) {
                if alpha > 0 {
                    drawn += 1;
                    assert_eq!([r, g, b], [0x1F, 0x6F, 0x5C]);
                }
            }
        }
    }
    assert!(drawn > 100, "the home glyph covers many pixels, got {drawn}");
}

#[test]
fn bad_icon_names_and_missing_files_are_errors() {
    let shared = assets();
    let mut a = shared.borrow_mut();
    assert!(matches!(
        a.icons.get("../etc/passwd", Size::new(8, 8), Color::BLACK),
        Err(IconError::BadName(_))
    ));
    assert!(matches!(
        a.icons.get("no.such.icon", Size::new(8, 8), Color::BLACK),
        Err(IconError::Read { .. })
    ));
}

#[test]
fn missing_font_and_icon_directories_are_reported() {
    let missing = std::path::Path::new("/nonexistent/kaia/fonts");
    let err = Assets::load(missing, ICON_DIR.as_ref())
        .err()
        .expect("font dir is missing");
    assert!(matches!(err, AssetError::Fonts(_)), "{err}");

    let err = Assets::load(FONT_DIR.as_ref(), missing)
        .err()
        .expect("icon dir is missing");
    assert!(matches!(err, AssetError::IconDir { .. }), "{err}");
}
