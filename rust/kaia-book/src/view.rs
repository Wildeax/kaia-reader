//! Viewports, zoom and the mapping between page points and screen pixels.
//!
//! The screen image of a page starts at the top left of the viewport. Its
//! scale is the page-to-pixel factor from [`PageTransform::scale`]. Reflowable
//! pages are laid out at the viewport size, so their scale is 1.

use kaia_doc::{PageSize, PointF, RectF};
use kaia_gfx::{Point, Rect, Size};

/// How a fixed-layout page fits the viewport. Reflowable books ignore it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ZoomMode {
    /// The whole page fits the viewport.
    #[default]
    FitPage,
    /// The page width fits the viewport width.
    FitWidth,
    /// A percentage of the fit-page size, for example 150.
    Manual { percent: u32 },
}

impl ZoomMode {
    /// The scale that shows `page` in `viewport` under this mode.
    pub(crate) fn scale(self, page: PageSize, viewport: Size) -> f32 {
        let sx = viewport.w as f32 / page.width.max(f32::MIN_POSITIVE);
        let sy = viewport.h as f32 / page.height.max(f32::MIN_POSITIVE);
        match self {
            ZoomMode::FitPage => sx.min(sy),
            ZoomMode::FitWidth => sx,
            ZoomMode::Manual { percent } => sx.min(sy) * percent.max(1) as f32 / 100.0,
        }
    }
}

/// The mapping between page points and screen pixels for one page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageTransform {
    /// Pixels per page point.
    pub scale: f32,
    /// Size of the page in points.
    pub page: PageSize,
    /// Size of the rendered page image in pixels.
    pub image: Size,
}

impl PageTransform {
    /// Builds the transform for a page. Reflowable pages use scale 1.
    pub(crate) fn new(page: PageSize, viewport: Size, zoom: Option<ZoomMode>) -> Self {
        let scale = match zoom {
            Some(z) => z.scale(page, viewport),
            None => 1.0,
        };
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let image = Size {
            w: ((page.width * scale).round() as u32).max(1),
            h: ((page.height * scale).round() as u32).max(1),
        };
        Self { scale, page, image }
    }

    /// Screen pixel to page point.
    pub fn to_page(&self, p: Point) -> PointF {
        PointF {
            x: p.x as f32 / self.scale,
            y: p.y as f32 / self.scale,
        }
    }

    /// Page point to screen pixel.
    pub fn to_screen_point(&self, p: PointF) -> Point {
        Point {
            x: (p.x * self.scale).round() as i32,
            y: (p.y * self.scale).round() as i32,
        }
    }

    /// Page box to the smallest screen rectangle that contains it.
    pub fn to_screen(&self, r: RectF) -> Rect {
        let x0 = (r.x0 * self.scale).floor() as i32;
        let y0 = (r.y0 * self.scale).floor() as i32;
        let x1 = (r.x1 * self.scale).ceil() as i32;
        let y1 = (r.y1 * self.scale).ceil() as i32;
        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0) as u32,
            h: (y1 - y0).max(0) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_modes() {
        let page = PageSize {
            width: 600.0,
            height: 800.0,
        };
        let vp = Size { w: 300, h: 300 };
        assert_eq!(ZoomMode::FitPage.scale(page, vp), 0.375);
        assert_eq!(ZoomMode::FitWidth.scale(page, vp), 0.5);
        assert_eq!(ZoomMode::Manual { percent: 200 }.scale(page, vp), 0.75);
    }

    #[test]
    fn screen_and_page_round_trip() {
        let t = PageTransform::new(
            PageSize {
                width: 600.0,
                height: 800.0,
            },
            Size { w: 300, h: 400 },
            Some(ZoomMode::FitPage),
        );
        assert_eq!(t.image, Size { w: 300, h: 400 });
        let p = t.to_page(Point { x: 150, y: 200 });
        assert_eq!(t.to_screen_point(p), Point { x: 150, y: 200 });
        let r = t.to_screen(RectF::new(10.0, 10.0, 20.0, 30.0));
        assert_eq!(
            r,
            Rect {
                x: 5,
                y: 5,
                w: 5,
                h: 10
            }
        );
    }
}
