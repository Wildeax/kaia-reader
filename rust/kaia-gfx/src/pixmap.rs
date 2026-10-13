//! Owned pixel buffers. Gray8 is the native format for E Ink; Rgb24 is used
//! on color panels and for color content.

use crate::geom::{Rect, Size};

/// Pixel layout of a [`Pixmap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// One byte per pixel, 0 is black and 255 is white.
    Gray8,
    /// Three bytes per pixel, R G B.
    Rgb24,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Gray8 => 1,
            PixelFormat::Rgb24 => 3,
        }
    }
}

/// An opaque color. Gray pixmaps store the luminance of an RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const BLACK: Color = Color::gray(0);
    pub const WHITE: Color = Color::gray(255);

    pub const fn gray(v: u8) -> Self {
        Self { r: v, g: v, b: v }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Luminance using the Rec. 601 weights, rounded.
    pub fn luma(self) -> u8 {
        ((self.r as u32 * 299 + self.g as u32 * 587 + self.b as u32 * 114 + 500) / 1000) as u8
    }
}

/// An owned, row-major pixel buffer with no padding between rows.
#[derive(Clone, PartialEq, Eq)]
pub struct Pixmap {
    width: u32,
    height: u32,
    format: PixelFormat,
    data: Vec<u8>,
}

impl std::fmt::Debug for Pixmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pixmap")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .finish()
    }
}

impl Pixmap {
    /// A pixmap filled with white.
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let len = width as usize * height as usize * format.bytes_per_pixel();
        Self {
            width,
            height,
            format,
            data: vec![255; len],
        }
    }

    /// Wraps existing bytes. Returns None when the length does not match.
    pub fn from_data(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Option<Self> {
        let len = width as usize * height as usize * format.bytes_per_pixel();
        (data.len() == len).then_some(Self {
            width,
            height,
            format,
            data,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.size())
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Bytes per row.
    pub fn stride(&self) -> usize {
        self.width as usize * self.format.bytes_per_pixel()
    }

    /// Sets one pixel. Out-of-bounds writes are ignored.
    pub fn put(&mut self, x: i32, y: i32, c: Color) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let bpp = self.format.bytes_per_pixel();
        let i = y as usize * self.stride() + x as usize * bpp;
        match self.format {
            PixelFormat::Gray8 => self.data[i] = c.luma(),
            PixelFormat::Rgb24 => {
                self.data[i] = c.r;
                self.data[i + 1] = c.g;
                self.data[i + 2] = c.b;
            }
        }
    }

    /// Reads one pixel, or None outside the pixmap.
    pub fn get(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        let bpp = self.format.bytes_per_pixel();
        let i = y as usize * self.stride() + x as usize * bpp;
        Some(match self.format {
            PixelFormat::Gray8 => Color::gray(self.data[i]),
            PixelFormat::Rgb24 => Color::rgb(self.data[i], self.data[i + 1], self.data[i + 2]),
        })
    }

    /// Fills a rectangle, clipped to the pixmap.
    pub fn fill_rect(&mut self, rect: Rect, c: Color) {
        let r = rect.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        let bpp = self.format.bytes_per_pixel();
        let stride = self.stride();
        for y in r.y..r.bottom() {
            let row = y as usize * stride;
            let start = row + r.x as usize * bpp;
            let end = start + r.w as usize * bpp;
            match self.format {
                PixelFormat::Gray8 => self.data[start..end].fill(c.luma()),
                PixelFormat::Rgb24 => {
                    for px in self.data[start..end].as_chunks_mut::<3>().0 {
                        px[0] = c.r;
                        px[1] = c.g;
                        px[2] = c.b;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_is_clipped() {
        let mut p = Pixmap::new(4, 4, PixelFormat::Gray8);
        p.fill_rect(Rect::new(-2, -2, 4, 4), Color::BLACK);
        assert_eq!(p.get(0, 0), Some(Color::BLACK));
        assert_eq!(p.get(2, 2), Some(Color::WHITE));
    }

    #[test]
    fn rgb_put_get_round_trip() {
        let mut p = Pixmap::new(2, 1, PixelFormat::Rgb24);
        p.put(1, 0, Color::rgb(10, 20, 30));
        assert_eq!(p.get(1, 0), Some(Color::rgb(10, 20, 30)));
        assert_eq!(p.get(2, 0), None);
    }
}
