//! Reads PNG and JPEG type and dimensions from the header, without decoding.

use crate::error::StoreError;
use kaia_core::limits::ASSET_MAX_DIM;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Type and size of an accepted image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ImageInfo {
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

/// File extension used for an accepted MIME type.
pub(crate) fn ext_for_mime(mime: &str) -> &'static str {
    if mime == "image/png" {
        "png"
    } else {
        "jpg"
    }
}

/// Detects the type from magic bytes and reads the dimensions from the header.
/// Rejects anything that is not PNG or JPEG, or is larger than 2048 x 2048.
pub(crate) fn inspect(bytes: &[u8]) -> Result<ImageInfo, StoreError> {
    let (mime, dims) = if bytes.starts_with(PNG_SIGNATURE) {
        ("image/png", png_dims(bytes))
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ("image/jpeg", jpeg_dims(bytes))
    } else {
        return Err(StoreError::AssetRejected("not a PNG or JPEG file".into()));
    };
    let (width, height) =
        dims.ok_or_else(|| StoreError::AssetRejected(format!("cannot read {mime} dimensions from header")))?;
    if width == 0 || height == 0 || width > ASSET_MAX_DIM || height > ASSET_MAX_DIM {
        return Err(StoreError::AssetRejected(format!(
            "image is {width}x{height}, limit is {ASSET_MAX_DIM}x{ASSET_MAX_DIM}"
        )));
    }
    Ok(ImageInfo { mime, width, height })
}

/// The IHDR chunk must come first, right after the signature.
fn png_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(b[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(b[20..24].try_into().ok()?);
    Some((width, height))
}

/// Walks the marker segments up to the first SOF segment.
fn jpeg_dims(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2; // past SOI
    loop {
        // Skip to the next marker. Fill bytes (0xFF) may precede the marker code.
        while i < b.len() && b[i] != 0xFF {
            i += 1;
        }
        while i < b.len() && b[i] == 0xFF {
            i += 1;
        }
        let marker = *b.get(i)?;
        i += 1;
        match marker {
            // Standalone markers with no length field.
            0x00 | 0x01 | 0xD0..=0xD8 => continue,
            // EOI or SOS: the frame header must have come before this.
            0xD9 | 0xDA => return None,
            _ => {}
        }
        let seg_len = u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize;
        if seg_len < 2 {
            return None;
        }
        // SOF0..SOF15, excluding DHT (C4), JPG (C8) and DAC (CC).
        let is_sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            // Segment body: precision (1), height (2), width (2).
            let height = u16::from_be_bytes(b.get(i + 3..i + 5)?.try_into().ok()?);
            let width = u16::from_be_bytes(b.get(i + 5..i + 7)?.try_into().ok()?);
            return Some((u32::from(width), u32::from(height)));
        }
        i += seg_len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_magic() {
        let err = inspect(b"GIF89a\x01\x00\x01\x00").unwrap_err();
        assert!(matches!(err, StoreError::AssetRejected(_)));
        assert!(inspect(&[]).is_err());
    }

    #[test]
    fn png_header_dimensions() {
        let mut png = Vec::from(PNG_SIGNATURE);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&300u32.to_be_bytes());
        png.extend_from_slice(&200u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        let info = inspect(&png).unwrap();
        assert_eq!((info.mime, info.width, info.height), ("image/png", 300, 200));
    }

    #[test]
    fn png_without_ihdr_is_rejected() {
        let mut png = Vec::from(PNG_SIGNATURE);
        png.extend_from_slice(&[0u8; 20]);
        assert!(inspect(&png).is_err());
    }

    #[test]
    fn jpeg_sof_after_app0_segment() {
        let jpeg = [
            0xFF, 0xD8, // SOI
            0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, // APP0, length 4
            0xFF, 0xFF, 0xC4, 0x00, 0x02, // fill byte, then DHT with empty body
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x01, 0x90, 0x02, 0x58, 0x01, 0x01, 0x11,
            0x00, // SOF0, 600x400
        ];
        let info = inspect(&jpeg).unwrap();
        assert_eq!((info.mime, info.width, info.height), ("image/jpeg", 600, 400));
    }

    #[test]
    fn jpeg_without_frame_header_is_rejected() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02];
        assert!(inspect(&jpeg).is_err());
        let truncated = [0xFF, 0xD8, 0xFF, 0xC0, 0x00];
        assert!(inspect(&truncated).is_err());
    }

    #[test]
    fn dimension_limit_is_inclusive() {
        let mut png = Vec::from(PNG_SIGNATURE);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&2048u32.to_be_bytes());
        png.extend_from_slice(&2049u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert!(matches!(inspect(&png), Err(StoreError::AssetRejected(_))));
    }
}
