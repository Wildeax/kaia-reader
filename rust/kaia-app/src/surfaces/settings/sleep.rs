//! The sleep screen: the cover of the current book, centred on the screen, with a
//! small "Kaia, sleeping" label at the bottom. Or a blank screen when the
//! setting `device.sleep_screen` is `blank`, or when the book has no cover.
//!
//! The app shows the widget as a full-screen window before the device sleeps
//! (`Window::new(sleep_screen(...), theme, assets, screen)`). The app owns the
//! sleep itself; this module only builds the screen.

use kaia_gfx::image::{scale_pixmap, FitMode};
use kaia_gfx::{Pixmap, Size};
use kaia_ui::components::{Align, Frame, Image, Label, Padding, Spacer, Stack};
use kaia_ui::geometry::Insets;
use kaia_ui::{Alignment, Tone, WidgetPod};

use super::keys;
use crate::services::Services;
use crate::surfaces::common;

/// The label drawn on the sleep screen.
pub const SLEEP_LABEL: &str = "Kaia, sleeping";

/// Whether the sleep screen shows a cover: the setting is `cover` and the book has one.
pub fn shows_cover(services: &Services) -> bool {
    keys::text_or(&services.settings, keys::DEVICE_SLEEP_SCREEN, "cover") != "blank"
}

/// The cover scaled to fit `screen` and placed in its middle. The white margins
/// of a page-rendered cover are cut first, so the picture fills the screen.
fn cover_widget(cover: &Pixmap, screen: Size) -> WidgetPod {
    let cover = kaia_ui::composite::trim_blank_margins(cover);
    let src = cover.size();
    let k =
        (f64::from(screen.w) / f64::from(src.w.max(1))).min(f64::from(screen.h) / f64::from(src.h.max(1)));
    let target = Size::new(
        (f64::from(src.w) * k).round() as u32,
        (f64::from(src.h) * k).round() as u32,
    );
    let scaled = scale_pixmap(&cover, target);
    let insets = Insets {
        left: screen.w.saturating_sub(target.w) / 2,
        top: screen.h.saturating_sub(target.h) / 2,
        right: 0,
        bottom: 0,
    };
    WidgetPod::new(Padding::new(
        WidgetPod::new(Image::new(scaled, FitMode::Contain)),
        insets,
    ))
}

/// The sleep screen for `book_id`, the book being read. None means no book:
/// the screen is blank.
pub fn sleep_screen(services: &Services, screen: Size, book_id: Option<i64>) -> Stack {
    let t = &services.theme;
    let cover = match book_id {
        Some(id) if shows_cover(services) => common::cover(&services.library, id),
        _ => None,
    };
    let background = match cover {
        // The library keeps covers in a 480 by 480 box, so the cover is scaled
        // to the largest size that fits the screen and placed in the middle.
        Some(pixmap) => cover_widget(&pixmap, screen),
        None => WidgetPod::new(Frame::new(WidgetPod::new(Spacer::new(1))).fill(Tone::Surface)),
    };
    let label = WidgetPod::new(Padding::new(
        WidgetPod::new(Label::new(
            SLEEP_LABEL,
            t.font.caption.clone(),
            Tone::InkSecondary,
        )),
        Insets::all(t.space.md),
    ));
    let label = WidgetPod::new(Align::new(label, Alignment::Center, Alignment::End));
    Stack::new(vec![background, label])
}
