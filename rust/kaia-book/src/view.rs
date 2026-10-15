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
    /// Fit the first detected text column.
    FitColumns,
    /// A rectangle selected on the page.
    Region,
    /// A percentage of the fit-page size, for example 150.
    Manual { percent: u32 },
}

impl ZoomMode {
    /// The scale that shows `page` in `viewport` under this mode.
    pub(crate) fn scale(self, page: PageSize, viewport: Size) -> f32 {
        let sx = viewport.w as f32 / page.width.max(f32::MIN_POSITIVE);
        let sy = viewport.h as f32 / page.height.max(f32::MIN_POSITIVE);
        match self {
            ZoomMode::FitPage | ZoomMode::FitColumns | ZoomMode::Region => sx.min(sy),
            ZoomMode::FitWidth => sx,
            ZoomMode::Manual { percent } => sx.min(sy) * percent.clamp(100, 400) as f32 / 100.0,
        }
    }
}

/// View settings sent with each engine request.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedView {
    pub mode: ZoomMode,
    pub scale: Option<f32>,
    pub pan: Point,
}

impl FixedView {
    pub fn key(self) -> (u32, i32, i32) {
        (self.scale.map(f32::to_bits).unwrap_or(0), self.pan.x, self.pan.y)
    }

    pub fn transform(self, page: PageSize, viewport: Size) -> PageTransform {
        let mut t = PageTransform::new(page, viewport, Some(self.mode));
        let fit = ZoomMode::FitPage.scale(page, viewport);
        t.scale = self.scale.map_or(t.scale, |scale| fit * scale.clamp(1.0, 4.0));
        t.scale = t.scale.clamp(fit, fit * 4.0);
        t.full_image = Size::new(
            (page.width * t.scale).round().max(1.0) as u32,
            (page.height * t.scale).round().max(1.0) as u32,
        );
        t.image = Size::new(t.full_image.w.min(viewport.w), t.full_image.h.min(viewport.h));
        t.offset = Point::new(
            self.pan
                .x
                .clamp(0, t.full_image.w.saturating_sub(viewport.w) as i32),
            self.pan
                .y
                .clamp(0, t.full_image.h.saturating_sub(viewport.h) as i32),
        );
        t
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
    /// Size before clipping to the viewport.
    pub full_image: Size,
    /// Top left of the visible image in scaled page pixels.
    pub offset: Point,
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
        Self {
            scale,
            page,
            image,
            full_image: image,
            offset: Point::new(0, 0),
        }
    }

    /// Screen pixel to page point.
    pub fn to_page(&self, p: Point) -> PointF {
        PointF {
            x: (p.x + self.offset.x) as f32 / self.scale,
            y: (p.y + self.offset.y) as f32 / self.scale,
        }
    }

    /// Page point to screen pixel.
    pub fn to_screen_point(&self, p: PointF) -> Point {
        Point {
            x: (p.x * self.scale).round() as i32 - self.offset.x,
            y: (p.y * self.scale).round() as i32 - self.offset.y,
        }
    }

    /// Page box to the smallest screen rectangle that contains it.
    pub fn to_screen(&self, r: RectF) -> Rect {
        let x0 = (r.x0 * self.scale).floor() as i32 - self.offset.x;
        let y0 = (r.y0 * self.scale).floor() as i32 - self.offset.y;
        let x1 = (r.x1 * self.scale).ceil() as i32 - self.offset.x;
        let y1 = (r.y1 * self.scale).ceil() as i32 - self.offset.y;
        Rect {
            x: x0,
            y: y0,
            w: (x1 - x0).max(0) as u32,
            h: (y1 - y0).max(0) as u32,
        }
    }
}

/// Groups text blocks by horizontal overlap. Wide headings do not join columns.
pub(crate) fn text_columns(blocks: &[RectF]) -> Vec<RectF> {
    let blocks: Vec<_> = blocks
        .iter()
        .copied()
        .filter(|b| {
            b.x0.is_finite()
                && b.x1.is_finite()
                && b.y0.is_finite()
                && b.y1.is_finite()
                && b.width() > 0.0
                && b.height() > 0.0
        })
        .collect();
    let left = blocks.iter().map(|b| b.x0).fold(f32::INFINITY, f32::min);
    let right = blocks.iter().map(|b| b.x1).fold(f32::NEG_INFINITY, f32::max);
    let mut narrow: Vec<_> = blocks
        .iter()
        .copied()
        .filter(|b| b.width() < (right - left) * 0.7)
        .collect();
    narrow.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let mut columns: Vec<RectF> = Vec::new();
    for block in narrow {
        if let Some(column) = columns
            .iter_mut()
            .find(|c| c.x1.min(block.x1) - c.x0.max(block.x0) > c.width().min(block.width()) * 0.25)
        {
            column.x0 = column.x0.min(block.x0);
            column.x1 = column.x1.max(block.x1);
            column.y0 = column.y0.min(block.y0);
            column.y1 = column.y1.max(block.y1);
        } else {
            columns.push(block);
        }
    }
    if columns.len() < 2 {
        return Vec::new();
    }
    columns
}

/// Finds columns separated by white gutters on pages without extractable text.
pub(crate) fn image_columns(image: &kaia_gfx::Pixmap, scale: f32) -> Vec<RectF> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let ink: Vec<_> = (0..width)
        .map(|x| {
            (0..height)
                .filter(|y| image.data()[y * image.stride() + x] < 245)
                .count()
                > height / 100
        })
        .collect();
    let mut bands = Vec::new();
    let mut start = None;
    for (x, filled) in ink.into_iter().chain(std::iter::once(false)).enumerate() {
        if filled {
            start.get_or_insert(x);
        } else if let Some(left) = start.take() {
            if x - left >= width / 7 {
                let top = (0..height)
                    .find(|y| (left..x).any(|col| image.data()[y * image.stride() + col] < 245))
                    .unwrap_or(0);
                let bottom = (0..height)
                    .rfind(|y| (left..x).any(|col| image.data()[y * image.stride() + col] < 245))
                    .unwrap_or(height - 1);
                bands.push(RectF::new(
                    left as f32 / scale,
                    top as f32 / scale,
                    x as f32 / scale,
                    (bottom + 1) as f32 / scale,
                ));
            }
        }
    }
    if bands.len() < 2 {
        Vec::new()
    } else {
        bands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_blocks_find_columns_below_a_spanning_heading() {
        let blocks = [
            RectF::new(10.0, 0.0, 590.0, 20.0),
            RectF::new(10.0, 30.0, 280.0, 200.0),
            RectF::new(12.0, 210.0, 280.0, 400.0),
            RectF::new(320.0, 30.0, 590.0, 400.0),
        ];
        let columns = text_columns(&blocks);
        assert_eq!(
            columns,
            vec![
                RectF::new(10.0, 30.0, 280.0, 400.0),
                RectF::new(320.0, 30.0, 590.0, 400.0)
            ]
        );
        assert!(text_columns(&blocks[..1]).is_empty());
    }

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
