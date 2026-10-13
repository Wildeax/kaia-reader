//! Reduces grays to the few levels an E Ink panel shows.
//!
//! The panel has 16 gray levels. Ordered dithering (Bayer 8x8) gives a stable
//! pattern that does not move between refreshes. Floyd-Steinberg error
//! diffusion gives smoother photos but changes more pixels between frames.
//! All output is Gray8 with values taken from the level set.

use crate::pixmap::{Color, PixelFormat, Pixmap};

/// Levels on the 16-gray E Ink panel.
pub const EINK_LEVELS: u16 = 16;

/// Bayer 8x8 threshold matrix, values 0 to 63.
const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

/// Converts a pixmap to Gray8. Gray8 is copied, and Rgb24 uses Rec. 601 luma.
pub fn to_gray8(src: &Pixmap) -> Pixmap {
    if src.format() == PixelFormat::Gray8 {
        return src.clone();
    }
    let mut out = Pixmap::new(src.width(), src.height(), PixelFormat::Gray8);
    for (dst, px) in out.data_mut().iter_mut().zip(src.data().as_chunks::<3>().0) {
        *dst = Color::rgb(px[0], px[1], px[2]).luma();
    }
    out
}

/// The gray value of level `k` out of `levels`, evenly spaced from 0 to 255.
fn level_value(k: u16, levels: u16) -> u8 {
    ((k as u32 * 255 + (levels as u32 - 1) / 2) / (levels as u32 - 1)) as u8
}

/// Dithers with a Bayer 8x8 matrix. `levels` is clamped to 2 through 256.
/// The pattern depends only on pixel position, so a static screen stays stable.
pub fn ordered(src: &Pixmap, levels: u16) -> Pixmap {
    let levels = levels.clamp(2, 256);
    let gray = to_gray8(src);
    let mut out = Pixmap::new(gray.width(), gray.height(), PixelFormat::Gray8);
    let width = gray.width() as usize;
    let top = (levels - 1) as f32;
    for y in 0..gray.height() as usize {
        for x in 0..width {
            let v = gray.data()[y * width + x] as f32 / 255.0 * top;
            let base = v.floor();
            let threshold = (BAYER8[y % 8][x % 8] as f32 + 0.5) / 64.0;
            let k = (base + if v - base > threshold { 1.0 } else { 0.0 }).min(top) as u16;
            out.data_mut()[y * width + x] = level_value(k, levels);
        }
    }
    out
}

/// Dithers with Floyd-Steinberg error diffusion. `levels` is clamped to 2
/// through 256.
pub fn floyd_steinberg(src: &Pixmap, levels: u16) -> Pixmap {
    let levels = levels.clamp(2, 256);
    let gray = to_gray8(src);
    let (w, h) = (gray.width() as usize, gray.height() as usize);
    let mut work: Vec<f32> = gray.data().iter().map(|&v| v as f32).collect();
    let mut out = Pixmap::new(gray.width(), gray.height(), PixelFormat::Gray8);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let old = work[i].clamp(0.0, 255.0);
            let new = nearest_level(old, levels);
            out.data_mut()[i] = new;
            let err = old - new as f32;
            let mut spread = |dx: isize, dy: isize, weight: f32| {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
                    work[ny as usize * w + nx as usize] += err * weight;
                }
            };
            spread(1, 0, 7.0 / 16.0);
            spread(-1, 1, 3.0 / 16.0);
            spread(0, 1, 5.0 / 16.0);
            spread(1, 1, 1.0 / 16.0);
        }
    }
    out
}

fn nearest_level(v: f32, levels: u16) -> u8 {
    let top = (levels - 1) as f32;
    let k = (v / 255.0 * top).round().clamp(0.0, top) as u16;
    level_value(k, levels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;

    fn gradient() -> Pixmap {
        let mut pm = Pixmap::new(256, 64, PixelFormat::Gray8);
        for y in 0..64 {
            for x in 0..256 {
                pm.put(x, y, Color::gray(x as u8));
            }
        }
        pm
    }

    fn level_set(levels: u16) -> Vec<u8> {
        (0..levels).map(|k| level_value(k, levels)).collect()
    }

    fn mean(pm: &Pixmap) -> f32 {
        pm.data().iter().map(|&v| v as f32).sum::<f32>() / pm.data().len() as f32
    }

    #[test]
    fn eink_levels_are_multiples_of_17() {
        assert_eq!(level_set(16), (0..16).map(|k| k * 17).collect::<Vec<u8>>());
    }

    #[test]
    fn ordered_output_uses_only_levels() {
        let out = ordered(&gradient(), 16);
        let allowed = level_set(16);
        assert_eq!(out.format(), PixelFormat::Gray8);
        assert!(out.data().iter().all(|v| allowed.contains(v)));
    }

    #[test]
    fn ordered_keeps_mean_brightness() {
        let src = gradient();
        let out = ordered(&src, 16);
        assert!(
            (mean(&src) - mean(&out)).abs() < 1.5,
            "{} vs {}",
            mean(&src),
            mean(&out)
        );
    }

    #[test]
    fn floyd_steinberg_output_uses_only_levels_and_keeps_mean() {
        let mut flat = Pixmap::new(64, 64, PixelFormat::Gray8);
        flat.fill_rect(Rect::new(0, 0, 64, 64), Color::gray(100));
        let out = floyd_steinberg(&flat, 16);
        let allowed = level_set(16);
        assert!(out.data().iter().all(|v| allowed.contains(v)));
        assert!((mean(&out) - 100.0).abs() < 2.0, "mean {}", mean(&out));
    }

    #[test]
    fn two_levels_are_black_and_white() {
        let out = ordered(&gradient(), 2);
        assert!(out.data().iter().all(|&v| v == 0 || v == 255));
        let out = floyd_steinberg(&gradient(), 2);
        assert!(out.data().iter().all(|&v| v == 0 || v == 255));
    }

    #[test]
    fn extremes_stay_put() {
        let mut pm = Pixmap::new(2, 1, PixelFormat::Gray8);
        pm.put(0, 0, Color::BLACK);
        let out = ordered(&pm, 16);
        assert_eq!(out.get(0, 0), Some(Color::BLACK));
        assert_eq!(out.get(1, 0), Some(Color::WHITE));
    }

    #[test]
    fn rgb_is_converted_to_luma_first() {
        let mut pm = Pixmap::new(1, 1, PixelFormat::Rgb24);
        pm.put(0, 0, Color::rgb(0, 255, 0));
        let gray = to_gray8(&pm);
        assert_eq!(gray.format(), PixelFormat::Gray8);
        assert_eq!(
            gray.get(0, 0),
            Some(Color::gray(150)),
            "green luma is 0.587 * 255"
        );
    }
}
