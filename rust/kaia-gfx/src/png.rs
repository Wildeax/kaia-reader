//! PNG encoding for headless screenshots and golden files.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use ::image::{ExtendedColorType, ImageEncoder};

use crate::pixmap::{PixelFormat, Pixmap};
use crate::rgba::AlphaPixmap;

/// Errors from writing PNG data.
#[derive(Debug)]
pub enum PngError {
    Io(io::Error),
    Encode(::image::ImageError),
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PngError::Io(e) => write!(f, "I/O error: {e}"),
            PngError::Encode(e) => write!(f, "PNG encoding failed: {e}"),
        }
    }
}

impl std::error::Error for PngError {}

impl From<io::Error> for PngError {
    fn from(e: io::Error) -> Self {
        PngError::Io(e)
    }
}

/// Encodes a Gray8 or Rgb24 pixmap as PNG bytes. Gray8 is written as 8-bit luma
/// and Rgb24 as 8-bit RGB.
pub fn encode_pixmap(pm: &Pixmap) -> Result<Vec<u8>, PngError> {
    let color = match pm.format() {
        PixelFormat::Gray8 => ExtendedColorType::L8,
        PixelFormat::Rgb24 => ExtendedColorType::Rgb8,
    };
    encode(pm.data(), pm.width(), pm.height(), color)
}

/// Encodes an RGBA pixmap as 8-bit RGBA PNG bytes.
pub fn encode_rgba(pm: &AlphaPixmap) -> Result<Vec<u8>, PngError> {
    encode(pm.data(), pm.width(), pm.height(), ExtendedColorType::Rgba8)
}

/// Writes a Gray8 or Rgb24 pixmap to `path`, creating or replacing the file.
pub fn write_pixmap(pm: &Pixmap, path: &Path) -> Result<(), PngError> {
    fs::write(path, encode_pixmap(pm)?)?;
    Ok(())
}

/// Writes an RGBA pixmap to `path`, creating or replacing the file.
pub fn write_rgba(pm: &AlphaPixmap, path: &Path) -> Result<(), PngError> {
    fs::write(path, encode_rgba(pm)?)?;
    Ok(())
}

fn encode(data: &[u8], width: u32, height: u32, color: ExtendedColorType) -> Result<Vec<u8>, PngError> {
    let mut out = Vec::new();
    ::image::codecs::png::PngEncoder::new(&mut out)
        .write_image(data, width, height, color)
        .map_err(PngError::Encode)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixmap::Color;

    fn sample_gray() -> Pixmap {
        let mut pm = Pixmap::new(5, 3, PixelFormat::Gray8);
        for y in 0..3 {
            for x in 0..5 {
                pm.put(x, y, Color::gray((x * 40 + y) as u8));
            }
        }
        pm
    }

    #[test]
    fn gray_round_trip() {
        let pm = sample_gray();
        let bytes = encode_pixmap(&pm).unwrap();
        let back = ::image::load_from_memory(&bytes).unwrap().to_luma8();
        assert_eq!(back.dimensions(), (5, 3));
        assert_eq!(back.as_raw(), pm.data());
    }

    #[test]
    fn rgb_and_rgba_round_trip() {
        let mut rgb = Pixmap::new(2, 2, PixelFormat::Rgb24);
        rgb.put(1, 1, Color::rgb(10, 200, 30));
        let back = ::image::load_from_memory(&encode_pixmap(&rgb).unwrap())
            .unwrap()
            .to_rgb8();
        assert_eq!(back.as_raw(), rgb.data());

        let mut rgba = AlphaPixmap::new(1, 1);
        rgba.put(0, 0, [1, 2, 3, 4]);
        let back = ::image::load_from_memory(&encode_rgba(&rgba).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(back.as_raw(), rgba.data());
    }

    #[test]
    fn write_creates_a_file() {
        let dir = std::env::temp_dir().join(format!("kaia-gfx-png-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gray.png");
        write_pixmap(&sample_gray(), &path).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
