//! The display trait, the panel description and the errors a display returns.

use std::error::Error;
use std::fmt;

use kaia_gfx::{PixelFormat, Pixmap, Rect, Size};

use crate::rotation::Rotation;
use crate::waveform::Refresh;

/// A screen that shows frames. Backends are the desktop window, the headless
/// recorder and, later, the Kobo panel through FBInk.
///
/// Sizes and frames use the logical orientation: [`Display::size`] already includes
/// the current rotation, and [`Display::present`] takes a frame of that size. The
/// backend rotates the frame into panel order if it needs to.
pub trait Display {
    /// Logical size of the screen at the current rotation.
    fn size(&self) -> Size;

    /// The pixel format the screen prefers. A frame of either format is accepted.
    fn format(&self) -> PixelFormat;

    /// Pixels per inch of the panel.
    fn dpi(&self) -> u32;

    /// Shows `frame` and updates `refresh.region` with `refresh.waveform`.
    ///
    /// `frame` must have [`Display::size`]. `refresh.region` must lie inside the screen
    /// unless it is empty.
    fn present(&mut self, frame: &Pixmap, refresh: &Refresh) -> Result<(), DisplayError>;

    /// Changes the orientation. Later frames use the new logical size.
    fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DisplayError>;

    /// Waits until the last refresh has finished on the panel. Call it before the device
    /// suspends, so that no update is still running. Backends without a panel that
    /// refreshes asynchronously have nothing to wait for, so the default does nothing.
    fn wait_idle(&mut self) {}

    /// Turns the night mode on or off. Night mode inverts the panel and changes the
    /// waveforms of later refreshes. The default does nothing, for backends without a
    /// night mode.
    fn set_night_mode(&mut self, _on: bool) {}
}

/// The physical panel: its size at rotation 0, its density, and the frame format it
/// prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelSpec {
    pub size: Size,
    pub dpi: u32,
    pub format: PixelFormat,
}

impl PanelSpec {
    /// Kobo Libra Colour: 1264 by 1680 pixels, 300 dpi, color panel.
    pub const LIBRA_COLOUR: PanelSpec = PanelSpec {
        size: Size::new(1264, 1680),
        dpi: 300,
        format: PixelFormat::Rgb24,
    };
}

/// Why a display call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayError {
    /// The frame does not have the logical screen size.
    FrameSize { expected: Size, actual: Size },
    /// The refresh region reaches outside the screen.
    RegionOutside { region: Rect, screen: Size },
    /// A backend option has a value the backend does not accept.
    InvalidOption(String),
    /// The backend failed (window system, panel driver or file output).
    Backend(String),
}

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DisplayError::FrameSize { expected, actual } => write!(
                f,
                "frame is {}x{}, the screen is {}x{}",
                actual.w, actual.h, expected.w, expected.h
            ),
            DisplayError::RegionOutside { region, screen } => write!(
                f,
                "refresh region {}x{} at ({}, {}) is outside the {}x{} screen",
                region.w, region.h, region.x, region.y, screen.w, screen.h
            ),
            DisplayError::InvalidOption(msg) => write!(f, "invalid display option: {msg}"),
            DisplayError::Backend(msg) => write!(f, "display backend failed: {msg}"),
        }
    }
}

impl Error for DisplayError {}

/// Checks a `present` call against the logical screen size. Backends call it before
/// they touch their state, so a rejected call changes nothing.
pub fn check_present(screen: Size, frame: &Pixmap, refresh: &Refresh) -> Result<(), DisplayError> {
    if frame.size() != screen {
        return Err(DisplayError::FrameSize {
            expected: screen,
            actual: frame.size(),
        });
    }
    let region = refresh.region;
    if !region.is_empty() && region.intersect(&Rect::from_size(screen)) != region {
        return Err(DisplayError::RegionOutside { region, screen });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::waveform::Waveform;

    fn refresh(region: Rect) -> Refresh {
        Refresh::new(region, Waveform::Du, false)
    }

    #[test]
    fn accepts_a_frame_of_the_screen_size() {
        let frame = Pixmap::new(10, 8, PixelFormat::Gray8);
        assert!(check_present(Size::new(10, 8), &frame, &refresh(Rect::new(0, 0, 10, 8))).is_ok());
    }

    #[test]
    fn rejects_a_frame_of_another_size() {
        let frame = Pixmap::new(10, 7, PixelFormat::Gray8);
        assert_eq!(
            check_present(Size::new(10, 8), &frame, &refresh(Rect::new(0, 0, 1, 1))),
            Err(DisplayError::FrameSize {
                expected: Size::new(10, 8),
                actual: Size::new(10, 7),
            })
        );
    }

    #[test]
    fn rejects_a_region_that_leaves_the_screen() {
        let frame = Pixmap::new(10, 8, PixelFormat::Gray8);
        let outside = Rect::new(5, 5, 6, 1);
        assert!(matches!(
            check_present(Size::new(10, 8), &frame, &refresh(outside)),
            Err(DisplayError::RegionOutside { .. })
        ));
    }

    #[test]
    fn accepts_an_empty_region() {
        let frame = Pixmap::new(10, 8, PixelFormat::Gray8);
        assert!(check_present(Size::new(10, 8), &frame, &refresh(Rect::new(50, 50, 0, 0))).is_ok());
    }
}
