//! Design tokens from `docs/kaia/specs/DESIGN.md`, scaled for the panel.
//!
//! Every size is passed through [`Scale::dp`], which is KOReader's
//! `Screen:scaleBySize` (`base/ffi/framebuffer.lua`). Widgets read tokens from
//! the theme and never hardcode sizes or grays.

use kaia_gfx::text::TextStyle;
use kaia_gfx::{Color, PixelFormat};

/// Screen size and the optional DPI override that `scaleBySize` uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub width: u32,
    pub height: u32,
    /// Set when the user or the device overrides the DPI. Without it, the
    /// scale follows the shorter screen side only.
    pub dpi: Option<u32>,
}

impl Scale {
    /// `ceil(value * (size_scale + dpi_scale) / 2)`, where `size_scale` is the
    /// shorter side over 600 and `dpi_scale` is `dpi / 160` with an override,
    /// or `size_scale` without one. This is the formula in
    /// `framebuffer.lua`, including the round-up.
    pub fn dp(&self, value: u32) -> u32 {
        let size_scale = self.width.min(self.height) as f64 / 600.0;
        let dpi_scale = match self.dpi {
            Some(dpi) => dpi as f64 / 160.0,
            None => size_scale,
        };
        (value as f64 * (size_scale + dpi_scale) / 2.0).ceil() as u32
    }
}

/// A font face from the bundled Noto families. `size` is in pixels, already scaled.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub family: &'static str,
    pub size: f32,
    pub weight: u16,
}

/// A named color from the palette. Components refer to colors by tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ink,
    InkSecondary,
    InkMuted,
    Divider,
    Surface,
    SurfaceAlt,
    /// Green on color panels, ink on grayscale panels.
    Accent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub xs: u32,
    pub sm: u32,
    pub md: u32,
    pub lg: u32,
    pub xl: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Radius {
    pub card: u32,
    pub button: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lines {
    pub hairline: u32,
    pub divider: u32,
    pub focus: u32,
    pub progress: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fonts {
    pub display: FontSpec,
    pub title: FontSpec,
    pub book_title: FontSpec,
    pub body: FontSpec,
    pub body_bold: FontSpec,
    pub shelf_title: FontSpec,
    pub caption: FontSpec,
    pub label: FontSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub ink: Color,
    pub ink_secondary: Color,
    pub ink_muted: Color,
    pub divider: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub accent: Color,
}

/// The accent green from DESIGN.md.
const ACCENT_RGB: Color = Color::rgb(0x1F, 0x6F, 0x5C);

/// The smallest tappable size: 9 mm, which is `scaleBySize(50)` when the scale
/// follows the screen (106 px on the Libra Colour). A DPI override changes the
/// scale and can make the same 50 units smaller than 9 mm (100 px at 300 DPI),
/// so the 9 mm size at the override DPI is the floor.
pub(crate) fn touch_target(scale: &Scale) -> u32 {
    let mm_px = scale
        .dpi
        .map_or(0, |dpi| (9.0 * f64::from(dpi) / 25.4).ceil() as u32);
    scale.dp(50).max(mm_px)
}

/// All tokens for one panel.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub scale: Scale,
    /// True for the color panel. Decides the pixel format and the accent.
    pub color_panel: bool,
    pub space: Space,
    pub radius: Radius,
    pub line: Lines,
    pub font: Fonts,
    pub palette: Palette,
    /// Smallest size of a tappable control: 9 mm. See [`touch_target`].
    pub touch_target: u32,
    /// Icon size for controls: `scaleBySize(32)`.
    pub icon_size: u32,
    /// Height of the bottom nav bar: `scaleBySize(60)` from DESIGN.md.
    pub nav_bar: u32,
    /// Minimum height of a list row: about 14 mm at 300 PPI, from APP_UX.md.
    pub row_height: u32,
    /// True when the UI runs right to left. Composite components mirror
    /// their layout (back chevrons, action order) when this is set.
    pub rtl: bool,
}

impl Theme {
    /// The tokens of DESIGN.md at `scale`. `color_panel` selects the accent.
    pub fn new(scale: Scale, color_panel: bool) -> Self {
        let dp = |v| scale.dp(v);
        let font = |family, size, weight| FontSpec {
            family,
            size: dp(size) as f32,
            weight,
        };
        let ink = Color::gray(0x00);
        Self {
            scale,
            color_panel,
            space: Space {
                xs: dp(4),
                sm: dp(8),
                md: dp(16),
                lg: dp(24),
                xl: dp(40),
            },
            radius: Radius {
                card: dp(12),
                button: dp(10),
            },
            line: Lines {
                hairline: dp(1),
                divider: dp(1),
                focus: dp(3),
                progress: dp(6),
            },
            font: Fonts {
                display: font("Noto Serif", 34, 700),
                title: font("Noto Sans", 26, 700),
                book_title: font("Noto Serif", 22, 700),
                body: font("Noto Sans", 20, 400),
                body_bold: font("Noto Sans", 20, 700),
                shelf_title: font("Noto Sans", 16, 700),
                caption: font("Noto Sans", 16, 400),
                label: font("Noto Sans", 15, 700),
            },
            touch_target: touch_target(&scale),
            icon_size: dp(32),
            nav_bar: dp(60),
            row_height: dp(83),
            rtl: false,
            palette: Palette {
                ink,
                ink_secondary: Color::gray(0x55),
                ink_muted: Color::gray(0x88),
                divider: Color::gray(0xCC),
                surface: Color::gray(0xFF),
                surface_alt: Color::gray(0xEE),
                accent: if color_panel { ACCENT_RGB } else { ink },
            },
        }
    }

    /// The same tokens with the layout direction set. Returns the theme with
    /// `rtl` changed.
    pub fn with_rtl(mut self, rtl: bool) -> Self {
        self.rtl = rtl;
        self
    }

    /// The pixel format the panel frame uses.
    pub fn pixel_format(&self) -> PixelFormat {
        if self.color_panel {
            PixelFormat::Rgb24
        } else {
            PixelFormat::Gray8
        }
    }

    pub fn color(&self, tone: Tone) -> Color {
        let p = &self.palette;
        match tone {
            Tone::Ink => p.ink,
            Tone::InkSecondary => p.ink_secondary,
            Tone::InkMuted => p.ink_muted,
            Tone::Divider => p.divider,
            Tone::Surface => p.surface,
            Tone::SurfaceAlt => p.surface_alt,
            Tone::Accent => p.accent,
        }
    }

    /// The text style for a font token in a tone. Line height is 1.25 times the size.
    pub fn style(&self, font: &FontSpec, tone: Tone) -> TextStyle {
        TextStyle {
            family: font.family.to_string(),
            weight: font.weight,
            color: self.color(tone),
            ..TextStyle::new(font.size)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dp_rounds_up_and_zero_stays_zero() {
        let s = Scale {
            width: 600,
            height: 900,
            dpi: None,
        };
        assert_eq!(s.dp(0), 0);
        assert_eq!(s.dp(1), 1, "a scale of 1.0 keeps the value");
        let big = Scale {
            width: 1200,
            height: 1200,
            dpi: None,
        };
        assert_eq!(big.dp(3), 6, "scale 2.0 doubles");
        let fractional = Scale {
            width: 900,
            height: 900,
            dpi: None,
        };
        assert_eq!(fractional.dp(2), 3, "ceil(2 * 1.5)");
    }

    #[test]
    fn touch_target_is_nine_millimetres_at_the_panel_dpi() {
        let libra = Scale {
            width: 1264,
            height: 1680,
            dpi: Some(300),
        };
        // 9 mm at 300 DPI is 106.3 px, so the floor is 107 px. dp(50) with the
        // override would be 100 px, which is 8.5 mm.
        assert_eq!(touch_target(&libra), 107);
        let no_override = Scale { dpi: None, ..libra };
        assert_eq!(touch_target(&no_override), 106, "dp(50) without an override");
    }
}
