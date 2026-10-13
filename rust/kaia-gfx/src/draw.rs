//! Shapes, borders, blits and tone changes on [`Pixmap`]s.
//!
//! Curved edges are anti-aliased. Each pixel gets a coverage value from the
//! signed distance between its center and the edge. Rectangles with whole-pixel
//! edges get no partial pixels. Coordinates are pixels, origin top left.

use crate::geom::{Point, Rect};
use crate::pixmap::{Color, Pixmap};
use crate::rgba::AlphaPixmap;

/// Blends `color` over the pixel at (x, y). `alpha` 0 leaves the pixel alone and
/// 255 replaces it. Pixels outside the pixmap are skipped.
pub(crate) fn blend_pixel(pm: &mut Pixmap, x: i32, y: i32, color: Color, alpha: u8) {
    if alpha == 0 {
        return;
    }
    if alpha == 255 {
        pm.put(x, y, color);
        return;
    }
    let Some(dst) = pm.get(x, y) else {
        return;
    };
    let a = alpha as u32;
    let mix = |d: u8, s: u8| ((d as u32 * (255 - a) + s as u32 * a + 127) / 255) as u8;
    pm.put(
        x,
        y,
        Color::rgb(mix(dst.r, color.r), mix(dst.g, color.g), mix(dst.b, color.b)),
    );
}

fn to_alpha(coverage: f32) -> u8 {
    (coverage * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Coverage of a pixel given its signed distance to an edge (negative inside).
fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// A rounded rectangle in floating point, centered at (cx, cy).
#[derive(Clone, Copy)]
struct RoundRect {
    cx: f32,
    cy: f32,
    hx: f32,
    hy: f32,
    r: f32,
}

impl RoundRect {
    fn new(rect: Rect, radius: f32) -> Self {
        let hx = rect.w as f32 / 2.0;
        let hy = rect.h as f32 / 2.0;
        Self {
            cx: rect.x as f32 + hx,
            cy: rect.y as f32 + hy,
            hx,
            hy,
            r: radius.clamp(0.0, hx.min(hy)),
        }
    }

    /// Signed distance from (px, py) to the outline. Negative inside.
    fn distance(&self, px: f32, py: f32) -> f32 {
        let qx = (px - self.cx).abs() - (self.hx - self.r);
        let qy = (py - self.cy).abs() - (self.hy - self.r);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        let inside = qx.max(qy).min(0.0);
        outside + inside - self.r
    }

    /// The shape shrunk by `t` on every side. The corner radius shrinks too, so
    /// the stroke keeps a constant width around the corners. None when nothing
    /// is left inside.
    fn inset(&self, t: f32) -> Option<Self> {
        let hx = self.hx - t;
        let hy = self.hy - t;
        if hx <= 0.0 || hy <= 0.0 {
            return None;
        }
        Some(Self {
            cx: self.cx,
            cy: self.cy,
            hx,
            hy,
            r: (self.r - t).max(0.0).min(hx.min(hy)),
        })
    }
}

/// Fills a rectangle with rounded corners. `radius` is clamped to half the
/// smaller side. A radius of 0 is a plain rectangle.
pub fn fill_rrect(pm: &mut Pixmap, rect: Rect, radius: u32, color: Color) {
    if rect.is_empty() {
        return;
    }
    if radius == 0 {
        pm.fill_rect(rect, color);
        return;
    }
    let shape = RoundRect::new(rect, radius as f32);
    let area = rect.intersect(&pm.bounds());
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let c = coverage(shape.distance(x as f32 + 0.5, y as f32 + 0.5));
            blend_pixel(pm, x, y, color, to_alpha(c));
        }
    }
}

/// Draws a border inside `rect`: a stroke of `thickness` pixels along the inner
/// edge of a rectangle with corner `radius`. The stroke never grows outside the
/// rectangle, so a border does not change the layout of its neighbors.
pub fn border(pm: &mut Pixmap, rect: Rect, radius: u32, thickness: u32, color: Color) {
    if rect.is_empty() || thickness == 0 {
        return;
    }
    let outer = RoundRect::new(rect, radius as f32);
    let inner = outer.inset(thickness as f32);
    let area = rect.intersect(&pm.bounds());
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let inside_inner = inner.map_or(0.0, |s| coverage(s.distance(px, py)));
            let c = (coverage(outer.distance(px, py)) - inside_inner).max(0.0);
            blend_pixel(pm, x, y, color, to_alpha(c));
        }
    }
}

/// Strokes the inner edge of a square-cornered rectangle. Same as [`border`]
/// with radius 0.
pub fn stroke_rect(pm: &mut Pixmap, rect: Rect, thickness: u32, color: Color) {
    border(pm, rect, 0, thickness, color);
}

/// A horizontal line from (x, y) going right for `len` pixels, `thickness` tall.
pub fn hline(pm: &mut Pixmap, x: i32, y: i32, len: u32, thickness: u32, color: Color) {
    pm.fill_rect(Rect::new(x, y, len, thickness), color);
}

/// A vertical line from (x, y) going down for `len` pixels, `thickness` wide.
pub fn vline(pm: &mut Pixmap, x: i32, y: i32, len: u32, thickness: u32, color: Color) {
    pm.fill_rect(Rect::new(x, y, thickness, len), color);
}

/// Fills a disc. The center and radius are in pixels and may be fractional.
pub fn fill_circle(pm: &mut Pixmap, cx: f32, cy: f32, radius: f32, color: Color) {
    if radius <= 0.0 {
        return;
    }
    let area = circle_area(pm, cx, cy, radius);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let d = dist(x, y, cx, cy) - radius;
            blend_pixel(pm, x, y, color, to_alpha(coverage(d)));
        }
    }
}

/// Draws a ring whose outer edge has `radius` and whose stroke is `thickness`
/// pixels wide, drawn inside the radius.
pub fn stroke_circle(pm: &mut Pixmap, cx: f32, cy: f32, radius: f32, thickness: f32, color: Color) {
    if radius <= 0.0 || thickness <= 0.0 {
        return;
    }
    let inner_r = radius - thickness;
    let area = circle_area(pm, cx, cy, radius);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let d = dist(x, y, cx, cy);
            let outer = coverage(d - radius);
            let inner = if inner_r > 0.0 { coverage(d - inner_r) } else { 0.0 };
            blend_pixel(pm, x, y, color, to_alpha((outer - inner).max(0.0)));
        }
    }
}

fn dist(x: i32, y: i32, cx: f32, cy: f32) -> f32 {
    let dx = x as f32 + 0.5 - cx;
    let dy = y as f32 + 0.5 - cy;
    (dx * dx + dy * dy).sqrt()
}

/// The pixels a disc can touch, clipped to the pixmap.
fn circle_area(pm: &Pixmap, cx: f32, cy: f32, radius: f32) -> Rect {
    let x0 = (cx - radius).floor() as i32 - 1;
    let y0 = (cy - radius).floor() as i32 - 1;
    let x1 = (cx + radius).ceil() as i32 + 1;
    let y1 = (cy + radius).ceil() as i32 + 1;
    Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32).intersect(&pm.bounds())
}

/// The part of `src_rect` that lands inside `dst`, and the offset from source
/// to destination. Source pixel (sx, sy) goes to (sx + ox, sy + oy).
fn copy_region(src_bounds: Rect, dst_bounds: Rect, src_rect: Rect, at: Point) -> Option<(Rect, i32, i32)> {
    let ox = at.x - src_rect.x;
    let oy = at.y - src_rect.y;
    let region = src_rect
        .intersect(&src_bounds)
        .intersect(&dst_bounds.translate(-ox, -oy));
    (!region.is_empty()).then_some((region, ox, oy))
}

/// Copies `src_rect` of `src` into `dst` so that the rectangle's top-left
/// corner lands on `at`. Parts outside either pixmap are clipped. The source
/// and destination formats may differ: Rgb24 converts to luma for Gray8, and
/// Gray8 is copied to all three channels for Rgb24.
pub fn blit(dst: &mut Pixmap, src: &Pixmap, src_rect: Rect, at: Point) {
    let Some((region, ox, oy)) = copy_region(src.bounds(), dst.bounds(), src_rect, at) else {
        return;
    };
    let w = region.w as usize;
    if src.format() == dst.format() {
        let bpp = src.format().bytes_per_pixel();
        let (src_stride, dst_stride) = (src.stride(), dst.stride());
        let dst_data = dst.data_mut();
        for sy in region.y..region.bottom() {
            let s = sy as usize * src_stride + region.x as usize * bpp;
            let dy = (sy + oy) as usize;
            let d = dy * dst_stride + (region.x + ox) as usize * bpp;
            dst_data[d..d + w * bpp].copy_from_slice(&src.data()[s..s + w * bpp]);
        }
        return;
    }
    for sy in region.y..region.bottom() {
        for sx in region.x..region.right() {
            if let Some(c) = src.get(sx, sy) {
                dst.put(sx + ox, sy + oy, c);
            }
        }
    }
}

/// Composites `src_rect` of an RGBA source over `dst` with its top-left corner
/// at `at`. Transparent pixels leave the destination unchanged.
pub fn blit_alpha(dst: &mut Pixmap, src: &AlphaPixmap, src_rect: Rect, at: Point) {
    let Some((region, ox, oy)) = copy_region(src.bounds(), dst.bounds(), src_rect, at) else {
        return;
    };
    for sy in region.y..region.bottom() {
        for sx in region.x..region.right() {
            if let Some([r, g, b, a]) = src.get(sx, sy) {
                blend_pixel(dst, sx + ox, sy + oy, Color::rgb(r, g, b), a);
            }
        }
    }
}

/// Inverts every channel inside `rect`. Used for selections and highlights.
pub fn invert_rect(pm: &mut Pixmap, rect: Rect) {
    map_rect_bytes(pm, rect, |b| 255 - b);
}

/// Moves every channel inside `rect` toward white. `amount` 0 changes nothing
/// and 255 makes the area white. Used to dim or lighten a region.
pub fn lighten_rect(pm: &mut Pixmap, rect: Rect, amount: u8) {
    let a = amount as u32;
    map_rect_bytes(pm, rect, |b| {
        (b as u32 + ((255 - b as u32) * a + 127) / 255) as u8
    });
}

/// Applies `f` to each byte of the pixels inside `rect`. Gray8 and Rgb24 both
/// store one byte per channel, so the same loop works for either.
fn map_rect_bytes(pm: &mut Pixmap, rect: Rect, f: impl Fn(u8) -> u8) {
    let area = rect.intersect(&pm.bounds());
    if area.is_empty() {
        return;
    }
    let bpp = pm.format().bytes_per_pixel();
    let stride = pm.stride();
    let w = area.w as usize * bpp;
    let data = pm.data_mut();
    for y in area.y as usize..area.bottom() as usize {
        let start = y * stride + area.x as usize * bpp;
        for b in &mut data[start..start + w] {
            *b = f(*b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixmap::PixelFormat;

    const BLACK: Color = Color::BLACK;

    fn gray_at(pm: &Pixmap, x: i32, y: i32) -> u8 {
        pm.get(x, y).expect("in bounds").r
    }

    #[test]
    fn rounded_rect_corner_is_anti_aliased_in_both_formats() {
        for format in [PixelFormat::Gray8, PixelFormat::Rgb24] {
            let mut pm = Pixmap::new(80, 80, format);
            fill_rrect(&mut pm, Rect::new(0, 0, 80, 80), 20, BLACK);
            assert_eq!(pm.get(0, 0), Some(Color::WHITE), "{format:?} corner is outside");
            assert_eq!(pm.get(40, 40), Some(BLACK), "{format:?} center is filled");
            let partial = (0..20)
                .flat_map(|y| (0..20).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let v = pm.get(x, y).unwrap().r;
                    v > 0 && v < 255
                })
                .count();
            assert!(partial > 10, "{format:?} corner has {partial} partial pixels");
        }
    }

    #[test]
    fn square_rect_has_no_partial_pixels() {
        let mut pm = Pixmap::new(10, 10, PixelFormat::Gray8);
        fill_rrect(&mut pm, Rect::new(2, 3, 5, 4), 0, BLACK);
        assert_eq!(pm.get(2, 3), Some(BLACK));
        assert_eq!(pm.get(6, 6), Some(BLACK));
        assert_eq!(pm.get(7, 3), Some(Color::WHITE));
        assert_eq!(pm.get(2, 7), Some(Color::WHITE));
    }

    #[test]
    fn border_stays_inside_the_rect() {
        let mut pm = Pixmap::new(12, 12, PixelFormat::Gray8);
        border(&mut pm, Rect::new(1, 1, 10, 10), 0, 2, BLACK);
        assert_eq!(gray_at(&pm, 1, 1), 0);
        assert_eq!(gray_at(&pm, 2, 2), 0);
        assert_eq!(gray_at(&pm, 3, 3), 255, "interior stays white");
        assert_eq!(gray_at(&pm, 0, 0), 255, "outside stays white");
        assert_eq!(gray_at(&pm, 10, 10), 0);
        assert_eq!(gray_at(&pm, 11, 11), 255);
    }

    #[test]
    fn thick_border_covers_small_rect() {
        let mut pm = Pixmap::new(6, 6, PixelFormat::Gray8);
        border(&mut pm, Rect::new(0, 0, 4, 4), 0, 5, BLACK);
        assert_eq!(gray_at(&pm, 1, 1), 0);
        assert_eq!(gray_at(&pm, 4, 4), 255);
    }

    #[test]
    fn lines_have_length_and_thickness() {
        let mut pm = Pixmap::new(10, 10, PixelFormat::Gray8);
        hline(&mut pm, 1, 2, 5, 2, BLACK);
        vline(&mut pm, 8, 0, 4, 1, BLACK);
        assert_eq!(gray_at(&pm, 0, 2), 255);
        assert_eq!(gray_at(&pm, 1, 2), 0);
        assert_eq!(gray_at(&pm, 5, 3), 0);
        assert_eq!(gray_at(&pm, 6, 2), 255);
        assert_eq!(gray_at(&pm, 8, 3), 0);
        assert_eq!(gray_at(&pm, 8, 4), 255);
    }

    #[test]
    fn circle_fill_and_ring() {
        let mut pm = Pixmap::new(40, 40, PixelFormat::Gray8);
        fill_circle(&mut pm, 20.0, 20.0, 10.0, BLACK);
        assert_eq!(gray_at(&pm, 20, 20), 0);
        assert_eq!(gray_at(&pm, 0, 0), 255);
        let edge = (0..40).filter(|&x| {
            let v = gray_at(&pm, x, 20);
            v > 0 && v < 255
        });
        assert!(edge.count() >= 1, "disc edge is anti-aliased");

        let mut ring = Pixmap::new(40, 40, PixelFormat::Gray8);
        stroke_circle(&mut ring, 20.0, 20.0, 10.0, 2.0, BLACK);
        assert_eq!(gray_at(&ring, 20, 20), 255, "ring leaves the center open");
        assert_eq!(gray_at(&ring, 20, 11), 0, "ring is drawn at the stroke");
    }

    #[test]
    fn blit_copies_region_with_clipping() {
        let mut src = Pixmap::new(4, 4, PixelFormat::Gray8);
        for y in 0..4 {
            for x in 0..4 {
                src.put(x, y, Color::gray((x * 10 + y) as u8));
            }
        }
        let mut dst = Pixmap::new(4, 4, PixelFormat::Gray8);
        // Source (1..3, 1..3) with its top-left at (-1, -1) in dst. Only source (2, 2) lands on dst (0, 0).
        blit(&mut dst, &src, Rect::new(1, 1, 2, 2), Point::new(-1, -1));
        assert_eq!(gray_at(&dst, 0, 0), 22);
        assert_eq!(gray_at(&dst, 1, 0), 255);
        assert_eq!(gray_at(&dst, 0, 1), 255);
    }

    #[test]
    fn blit_converts_between_formats() {
        let mut src = Pixmap::new(2, 1, PixelFormat::Rgb24);
        src.put(0, 0, Color::rgb(0, 0, 0));
        src.put(1, 0, Color::rgb(255, 0, 0));
        let mut dst = Pixmap::new(2, 1, PixelFormat::Gray8);
        blit(&mut dst, &src, src.bounds(), Point::new(0, 0));
        assert_eq!(gray_at(&dst, 0, 0), 0);
        assert_eq!(gray_at(&dst, 1, 0), 76, "red luma is 0.299 * 255");

        let mut gray = Pixmap::new(1, 1, PixelFormat::Gray8);
        gray.put(0, 0, Color::gray(90));
        let mut rgb = Pixmap::new(2, 1, PixelFormat::Rgb24);
        blit(&mut rgb, &gray, gray.bounds(), Point::new(1, 0));
        assert_eq!(rgb.get(1, 0), Some(Color::gray(90)));
        assert_eq!(rgb.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn blit_alpha_composites() {
        let mut src = AlphaPixmap::new(2, 1);
        src.put(0, 0, [0, 0, 0, 255]);
        src.put(1, 0, [0, 0, 0, 128]);
        let mut dst = Pixmap::new(2, 1, PixelFormat::Gray8);
        blit_alpha(&mut dst, &src, src.bounds(), Point::new(0, 0));
        assert_eq!(gray_at(&dst, 0, 0), 0);
        let half = gray_at(&dst, 1, 0);
        assert!((126..=128).contains(&half), "half black over white is {half}");
    }

    #[test]
    fn invert_and_lighten_stay_in_rect() {
        let mut pm = Pixmap::new(4, 1, PixelFormat::Rgb24);
        pm.put(0, 0, Color::rgb(10, 20, 30));
        invert_rect(&mut pm, Rect::new(0, 0, 1, 1));
        assert_eq!(pm.get(0, 0), Some(Color::rgb(245, 235, 225)));
        assert_eq!(pm.get(1, 0), Some(Color::WHITE));

        let mut g = Pixmap::new(2, 1, PixelFormat::Gray8);
        g.put(0, 0, Color::gray(0));
        lighten_rect(&mut g, Rect::new(0, 0, 1, 1), 128);
        assert_eq!(gray_at(&g, 0, 0), 128);
        assert_eq!(gray_at(&g, 1, 0), 255);
        lighten_rect(&mut g, Rect::new(0, 0, 1, 1), 255);
        assert_eq!(gray_at(&g, 0, 0), 255);
    }
}
