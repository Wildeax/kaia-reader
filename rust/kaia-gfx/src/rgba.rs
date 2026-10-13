//! RGBA8 pixel buffers with straight (not premultiplied) alpha. Icons, decoded
//! images with transparency and anything composited over a [`Pixmap`] use this
//! type. It is not a [`PixelFormat`] variant, so the E Ink formats stay small.

use std::fmt;

use crate::geom::{Rect, Size};

/// A row-major RGBA8 buffer with no padding between rows. Alpha 0 is fully
/// transparent and 255 is fully opaque.
#[derive(Clone, PartialEq, Eq)]
pub struct AlphaPixmap {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl fmt::Debug for AlphaPixmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AlphaPixmap")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl AlphaPixmap {
    /// A fully transparent pixmap.
    pub fn new(width: u32, height: u32) -> Self {
        let len = width as usize * height as usize * 4;
        Self {
            width,
            height,
            data: vec![0; len],
        }
    }

    /// Wraps existing bytes. Returns None when the length is not width * height * 4.
    pub fn from_data(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        let len = width as usize * height as usize * 4;
        (data.len() == len).then_some(Self { width, height, data })
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

    /// Bytes per row.
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Reads one pixel as `[r, g, b, a]`, or None outside the pixmap.
    pub fn get(&self, x: i32, y: i32) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        let i = y as usize * self.stride() + x as usize * 4;
        self.data[i..i + 4].try_into().ok()
    }

    /// Writes one pixel. Out-of-bounds writes are ignored.
    pub fn put(&mut self, x: i32, y: i32, rgba: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = y as usize * self.stride() + x as usize * 4;
        self.data[i..i + 4].copy_from_slice(&rgba);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_transparent() {
        let p = AlphaPixmap::new(2, 2);
        assert_eq!(p.get(1, 1), Some([0, 0, 0, 0]));
        assert_eq!(p.data().len(), 16);
    }

    #[test]
    fn put_get_and_bounds() {
        let mut p = AlphaPixmap::new(3, 1);
        p.put(2, 0, [1, 2, 3, 4]);
        p.put(3, 0, [9, 9, 9, 9]);
        assert_eq!(p.get(2, 0), Some([1, 2, 3, 4]));
        assert_eq!(p.get(3, 0), None);
        assert_eq!(p.get(-1, 0), None);
    }

    #[test]
    fn from_data_checks_length() {
        assert!(AlphaPixmap::from_data(2, 2, vec![0; 15]).is_none());
        assert!(AlphaPixmap::from_data(2, 2, vec![0; 16]).is_some());
    }
}
