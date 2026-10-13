//! Pixel work for the desktop window: conversion to the window's 0x00RRGGBB buffer,
//! box-filter scaling, flash inversion and the coordinate mapping between the window
//! and the panel. Pure functions, so they are tested without a window.

use kaia_gfx::{PixelFormat, Pixmap, Point, Rect, Size};

/// Converts a frame to 0x00RRGGBB pixels, row by row. Gray is repeated into all three
/// channels.
pub fn to_xrgb(pm: &Pixmap) -> Vec<u32> {
    match pm.format() {
        PixelFormat::Gray8 => pm
            .data()
            .iter()
            .map(|&v| ((v as u32) << 16) | ((v as u32) << 8) | v as u32)
            .collect(),
        PixelFormat::Rgb24 => pm
            .data()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32)
            .collect(),
    }
}

/// The source range that output index `d` of `n` averages over, for a source of `s`
/// pixels. Every source pixel is used by exactly one output pixel when `n <= s`.
fn span(d: usize, s: usize, n: usize) -> (usize, usize) {
    let start = d * s / n;
    let end = ((d + 1) * s / n).max(start + 1).min(s);
    (start, end)
}

/// Scales `src` (`sw` by `sh` pixels) to `dw` by `dh` pixels by averaging each block.
/// Expects `dw <= sw` and `dh <= sh`, which holds for a window no larger than the panel.
pub fn downscale(src: &[u32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(dw * dh);
    for dy in 0..dh {
        let (y0, y1) = span(dy, sh, dh);
        for dx in 0..dw {
            let (x0, x1) = span(dx, sw, dw);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in y0..y1 {
                for &p in &src[y * sw + x0..y * sw + x1] {
                    r += (p >> 16) & 0xff;
                    g += (p >> 8) & 0xff;
                    b += p & 0xff;
                    n += 1;
                }
            }
            let n = n.max(1);
            out.push(((r / n) << 16) | ((g / n) << 8) | (b / n));
        }
    }
    out
}

/// Inverts the pixels of `rect` in a buffer `stride` pixels wide. The rectangle is
/// clipped to the buffer.
pub fn invert_region(buf: &mut [u32], stride: usize, rect: Rect) {
    if stride == 0 {
        return;
    }
    let rows = buf.len() / stride;
    let clip = rect.intersect(&Rect::new(0, 0, stride as u32, rows as u32));
    for y in clip.y..clip.bottom() {
        let row = y as usize * stride;
        for p in &mut buf[row + clip.x as usize..row + clip.right() as usize] {
            *p ^= 0x00ff_ffff;
        }
    }
}

/// The window rectangle that shows a panel rectangle. Edges round outward, so every
/// panel pixel in `rect` is covered.
pub fn panel_rect_to_window(rect: Rect, panel: Size, window: Size) -> Rect {
    let (pw, ph) = (panel.w as i64, panel.h as i64);
    let (ww, wh) = (window.w as i64, window.h as i64);
    let x0 = rect.x as i64 * ww / pw;
    let y0 = rect.y as i64 * wh / ph;
    let x1 = (rect.right() as i64 * ww + pw - 1) / pw;
    let y1 = (rect.bottom() as i64 * wh + ph - 1) / ph;
    Rect::new(
        x0 as i32,
        y0 as i32,
        (x1 - x0).max(0) as u32,
        (y1 - y0).max(0) as u32,
    )
}

/// The panel pixel under a window position. The position is clamped to the panel.
pub fn window_to_panel(x: f32, y: f32, panel: Size, window: Size) -> Point {
    let (pw, ph) = (panel.w as i64, panel.h as i64);
    let (ww, wh) = (window.w.max(1) as i64, window.h.max(1) as i64);
    let px = (x.max(0.0) as i64 * pw / ww).clamp(0, pw - 1);
    let py = (y.max(0.0) as i64 * ph / wh).clamp(0, ph - 1);
    Point::new(px as i32, py as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_gfx::Color;

    #[test]
    fn xrgb_from_gray_and_rgb() {
        let mut gray = Pixmap::new(2, 1, PixelFormat::Gray8);
        gray.put(0, 0, Color::gray(0x10));
        assert_eq!(to_xrgb(&gray), vec![0x0010_1010, 0x00ff_ffff]);

        let mut rgb = Pixmap::new(1, 1, PixelFormat::Rgb24);
        rgb.put(0, 0, Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(to_xrgb(&rgb), vec![0x0012_3456]);
    }

    #[test]
    fn downscale_averages_blocks() {
        // A 2 by 2 block of 0, 0xff, 0, 0xff in the red channel averages to 0x7f.
        let src = [0x00_00_00, 0xff_00_00, 0x00_00_00, 0xff_00_00];
        assert_eq!(downscale(&src, 2, 2, 1, 1), vec![0x7f_00_00]);
    }

    #[test]
    fn downscale_keeps_size_at_full_scale() {
        let src: Vec<u32> = (0..6).collect();
        assert_eq!(downscale(&src, 3, 2, 3, 2), src);
    }

    #[test]
    fn invert_is_clipped_to_the_buffer() {
        let mut buf = vec![0u32; 4 * 2];
        invert_region(&mut buf, 4, Rect::new(2, 1, 10, 10));
        assert_eq!(buf[..4], [0, 0, 0, 0]);
        assert_eq!(buf[4..], [0, 0, 0xff_ffff, 0xff_ffff]);
    }

    #[test]
    fn panel_rect_covers_its_window_pixels() {
        let panel = Size::new(1264, 1680);
        let window = Size::new(632, 840);
        // A 3 pixel wide panel rect at x=1 maps to a window rect that contains x=0 and x=1.
        let mapped = panel_rect_to_window(Rect::new(1, 0, 3, 1), panel, window);
        assert_eq!(mapped.x, 0);
        assert!(mapped.right() >= 2);
        assert!(mapped.h >= 1);
    }

    #[test]
    fn window_position_maps_to_panel_and_clamps() {
        let panel = Size::new(1264, 1680);
        let window = Size::new(632, 840);
        assert_eq!(window_to_panel(0.0, 0.0, panel, window), Point::new(0, 0));
        assert_eq!(window_to_panel(316.0, 420.0, panel, window), Point::new(632, 840));
        assert_eq!(window_to_panel(-5.0, 9000.0, panel, window), Point::new(0, 1679));
    }
}
