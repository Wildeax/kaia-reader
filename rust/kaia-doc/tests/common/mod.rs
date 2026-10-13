//! Shared helpers for the corpus tests: fixture paths, font setup and a PNG writer.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The repository's `test/` directory with the PDF and EPUB fixtures.
pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
}

/// Public domain EPUBs that a developer keeps outside the repository. Empty
/// when the directory does not exist, so the tests that use them skip.
pub fn gutenberg_epubs() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let dir = PathBuf::from(home).join("kaia-test-library");
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "epub"))
        .collect();
    files.sort();
    files
}

/// Directory for artifacts that tests write: `rust/target/kaia-doc-test`.
pub fn out_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/kaia-doc-test");
    fs::create_dir_all(&dir).expect("create test output directory");
    dir
}

/// Fallback faces that MuPDF requests from FONTDIR, as relative paths. This
/// list follows `base/thirdparty/mupdf/external_fonts.patch`.
const FONT_FILES: &[&str] = &[
    "urw/NimbusRomNo9L-Reg.cff",
    "urw/NimbusRomNo9L-RegIta.cff",
    "urw/NimbusRomNo9L-Med.cff",
    "urw/NimbusRomNo9L-MedIta.cff",
    "urw/NimbusSanL-Reg.cff",
    "urw/NimbusSanL-RegIta.cff",
    "urw/NimbusSanL-Bol.cff",
    "urw/NimbusSanL-BolIta.cff",
    "urw/NimbusMono-Regular.cff",
    "urw/NimbusMono-Oblique.cff",
    "urw/NimbusMono-Bold.cff",
    "urw/NimbusMono-BoldOblique.cff",
    "urw/Dingbats.cff",
    "urw/StandardSymL.cff",
    "noto/NotoSans-Regular.ttf",
    "noto/NotoSans-Italic.ttf",
    "noto/NotoSans-Bold.ttf",
    "noto/NotoSans-BoldItalic.ttf",
    "noto/NotoSansCJKsc-Regular.otf",
    "freefont/FreeSerif.ttf",
];

/// Points MuPDF's FONTDIR at a test font directory and returns it.
///
/// The source is the repository's `resources/fonts`, which has every face
/// KOReader's MuPDF patch asks for. If a face is missing (for example with a
/// different KAIA_MUPDF_FONTS), it links to the nearest available one so a
/// page still renders; glyph shapes then differ from the device.
pub fn ensure_font_dir() -> PathBuf {
    static FONT_DIR: OnceLock<PathBuf> = OnceLock::new();
    FONT_DIR
        .get_or_init(|| {
            let dir = build_font_dir();
            // Edition 2021 allows set_var. get_or_init runs this once, and other
            // threads that call ensure_font_dir wait for it to finish.
            std::env::set_var("FONTDIR", &dir);
            dir
        })
        .clone()
}

fn build_font_dir() -> PathBuf {
    let root = out_dir().join("fonts");
    let source = option_env!("KAIA_DOC_FONT_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| panic!("no fallback font directory: build the KOReader desktop tree"));
    let available = |rel: &str| source.join(rel).exists();

    for rel in FONT_FILES {
        let target = root.join(rel);
        if target.exists() || target.is_symlink() {
            continue;
        }
        let from = if available(rel) {
            source.join(rel)
        } else {
            let fallback = if rel.contains("Mono") {
                "droid/DroidSansMono.ttf"
            } else if rel.contains("Med") || rel.contains("Bol") || rel.contains("Bold") {
                "urw/NimbusRomNo9L-Med.cff"
            } else {
                "urw/NimbusRomNo9L-Reg.cff"
            };
            source.join(fallback)
        };
        fs::create_dir_all(target.parent().unwrap()).expect("create font subdir");
        std::os::unix::fs::symlink(&from, &target).expect("link font");
    }
    root
}

/// Writes an 8-bit PNG. `gray` selects grayscale, otherwise RGB. `data` is
/// row-major with no padding.
pub fn write_png(path: &Path, width: u32, height: u32, gray: bool, data: &[u8]) {
    let channels = if gray { 1 } else { 3 };
    let row = width as usize * channels;
    assert_eq!(data.len(), row * height as usize, "pixel buffer size");

    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in data.chunks(row.max(1)) {
        raw.push(0); // filter type: none
        raw.extend_from_slice(line);
    }

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, if gray { 0 } else { 2 }, 0, 0, 0]);

    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    fs::write(path, out).expect("write PNG");
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// A zlib stream made of stored (uncompressed) deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    let mut blocks = data.chunks(65535).peekable();
    while let Some(block) = blocks.next() {
        let last = blocks.peek().is_none();
        out.push(last as u8);
        let len = block.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_and_adler_match_known_values() {
        // Check values from the PNG and zlib specifications.
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }
}
