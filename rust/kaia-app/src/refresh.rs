//! Turns the refresh requests of `kaia_events` into panel refreshes.
//!
//! The waveform for each KOReader mode comes from the table on
//! [`kaia_display::Waveform`]. This module only applies it: a request without a
//! region covers the whole screen, and a request outside the screen is clipped.

use kaia_display::{Refresh as PanelRefresh, Waveform};
use kaia_events::{Refresh, RefreshMode};
use kaia_gfx::{Rect, Size};

/// The waveform and the flash flag for a KOReader refresh mode.
///
/// `dither` only changes `full` on the color panel, where the driver uses `Gcc16`
/// for dithered content (see the table on `Waveform`).
pub fn waveform_for(mode: RefreshMode, dither: bool, color_panel: bool) -> (Waveform, bool) {
    match mode {
        RefreshMode::Full if dither && color_panel => (Waveform::Gcc16, true),
        RefreshMode::Full => (Waveform::Gc16, true),
        RefreshMode::FlashPartial => (Waveform::Glr16, true),
        RefreshMode::Partial | RefreshMode::NoMergePartial => (Waveform::Glr16, false),
        RefreshMode::FlashUi => (Waveform::Auto, true),
        RefreshMode::Ui | RefreshMode::NoMergeUi => (Waveform::Auto, false),
        RefreshMode::Fast => (Waveform::Du, false),
        RefreshMode::A2 => (Waveform::A2, false),
    }
}

/// The panel refresh for `refresh` on a screen of `screen` pixels. Returns None
/// when the region is empty after clipping to the screen.
pub fn to_panel(
    refresh: &Refresh,
    screen: Size,
    color_panel: bool,
    content_dither: bool,
) -> Option<PanelRefresh> {
    let screen_rect = Rect::from_size(screen);
    let region = match refresh.region {
        None => screen_rect,
        Some(r) => Rect::new(r.x, r.y, r.w.max(0) as u32, r.h.max(0) as u32),
    };
    let region = region.intersect(&screen_rect);
    if region.is_empty() {
        return None;
    }
    let (waveform, flash) = waveform_for(refresh.mode, refresh.dither, color_panel);
    Some(PanelRefresh::new(region, waveform, flash).with_dither(refresh.dither || content_dither))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_events::Rect as EventRect;

    #[test]
    fn koreader_modes_map_to_their_waveforms() {
        assert_eq!(
            waveform_for(RefreshMode::Full, false, true),
            (Waveform::Gc16, true)
        );
        assert_eq!(
            waveform_for(RefreshMode::Full, true, true),
            (Waveform::Gcc16, true)
        );
        assert_eq!(
            waveform_for(RefreshMode::Full, true, false),
            (Waveform::Gc16, true)
        );
        assert_eq!(
            waveform_for(RefreshMode::Partial, false, true),
            (Waveform::Glr16, false)
        );
        assert_eq!(
            waveform_for(RefreshMode::FlashPartial, false, true),
            (Waveform::Glr16, true)
        );
        assert_eq!(
            waveform_for(RefreshMode::Ui, false, true),
            (Waveform::Auto, false)
        );
        assert_eq!(
            waveform_for(RefreshMode::FlashUi, false, true),
            (Waveform::Auto, true)
        );
        assert_eq!(
            waveform_for(RefreshMode::Fast, false, true),
            (Waveform::Du, false)
        );
        assert_eq!(waveform_for(RefreshMode::A2, false, true), (Waveform::A2, false));
    }

    #[test]
    fn no_region_means_the_whole_screen() {
        let r = to_panel(&Refresh::new(RefreshMode::Ui), Size::new(100, 80), true, false).unwrap();
        assert_eq!(r.region, Rect::new(0, 0, 100, 80));
    }

    #[test]
    fn picture_content_is_dithered_and_plain_ui_is_not() {
        let screen = Size::new(100, 80);
        let plain = to_panel(&Refresh::new(RefreshMode::Full), screen, true, false).unwrap();
        assert!(!plain.dither);
        let pictures = to_panel(&Refresh::new(RefreshMode::Full), screen, true, true).unwrap();
        assert!(pictures.dither);
    }

    #[test]
    fn regions_are_clipped_and_empty_ones_dropped() {
        let screen = Size::new(100, 80);
        let partly = Refresh {
            region: Some(EventRect::new(90, 70, 50, 50)),
            ..Refresh::new(RefreshMode::Partial)
        };
        assert_eq!(
            to_panel(&partly, screen, true, false).unwrap().region,
            Rect::new(90, 70, 10, 10)
        );
        let outside = Refresh {
            region: Some(EventRect::new(200, 200, 10, 10)),
            ..Refresh::new(RefreshMode::Partial)
        };
        assert!(to_panel(&outside, screen, true, false).is_none());
        let empty = Refresh {
            region: Some(EventRect::new(0, 0, 0, 10)),
            ..Refresh::new(RefreshMode::Ui)
        };
        assert!(to_panel(&empty, screen, true, false).is_none());
    }
}
