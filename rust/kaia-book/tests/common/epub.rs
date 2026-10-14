//! Writes small EPUB files for the image tests. Each file has one XHTML page
//! with the given body, one grayscale PNG and a table of contents. The zip
//! entries are stored, not compressed, so the writer needs no compression code.
//!
//! The app's reader tests include this file by path, so it uses no
//! dependencies and no items from the test crate.

#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// The name of the picture in the book, as the body refers to it.
pub const PICTURE: &str = "pic.png";

/// Writes an EPUB to `path`. `body` is the XHTML inside `<body>`. `chapters`
/// lists the table of contents as (title, anchor in the body) pairs. The book
/// language is `en`.
pub fn write_epub(path: &Path, body: &str, chapters: &[(&str, &str)]) {
    write_epub_in(path, "en", body, chapters);
}

/// Like [`write_epub`], with `dc:language` set to `language`.
pub fn write_epub_in(path: &Path, language: &str, body: &str, chapters: &[(&str, &str)]) {
    let entries: Vec<(&str, Vec<u8>)> = vec![
        ("mimetype", b"application/epub+zip".to_vec()),
        ("META-INF/container.xml", CONTAINER.as_bytes().to_vec()),
        ("OEBPS/content.opf", opf(language).into_bytes()),
        ("OEBPS/toc.ncx", ncx(chapters).into_bytes()),
        ("OEBPS/book.xhtml", xhtml(body).into_bytes()),
        ("OEBPS/pic.png", png_gray(64, 64)),
    ];
    fs::write(path, zip(&entries)).expect("write epub");
}

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
"#;

fn opf(language: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Image test</dc:title>
    <dc:identifier id="id">urn:kaia:image-test</dc:identifier>
    <dc:language>{language}</dc:language>
    <dc:creator>Kaia</dc:creator>
  </metadata>
  <manifest>
    <item id="book" href="book.xhtml" media-type="application/xhtml+xml"/>
    <item id="pic" href="pic.png" media-type="image/png"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
  </manifest>
  <spine toc="ncx"><itemref idref="book"/></spine>
</package>
"#
    )
}

fn ncx(chapters: &[(&str, &str)]) -> String {
    let mut points = String::new();
    for (i, (title, anchor)) in chapters.iter().enumerate() {
        points.push_str(&format!(
            "<navPoint id=\"n{n}\" playOrder=\"{n}\"><navLabel><text>{title}</text></navLabel>\
             <content src=\"book.xhtml#{anchor}\"/></navPoint>\n",
            n = i + 1
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="urn:kaia:image-test"/></head>
  <docTitle><text>Image test</text></docTitle>
  <navMap>
{points}  </navMap>
</ncx>
"#
    )
}

fn xhtml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Image test</title></head>
<body>
{body}
</body>
</html>
"#
    )
}

/// A grayscale PNG with a checker pattern, so it has ink everywhere.
pub fn png_gray(width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    for y in 0..height {
        raw.push(0); // filter type 0 for each row
        for x in 0..width {
            raw.push(if (x / 8 + y / 8) % 2 == 0 { 0 } else { 255 });
        }
    }
    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n'];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8 bits, gray, deflate, filter 0, no interlace
    png_chunk(&mut out, b"IHDR", &ihdr);
    png_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    png_chunk(&mut out, b"IEND", &[]);
    out
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

/// A zlib stream made of stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(65535).peekable();
    if blocks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = blocks.next() {
        let last = blocks.peek().is_none();
        let len = block.len() as u16;
        out.push(u8::from(last));
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Builds a stored zip archive from (name, bytes) entries.
fn zip(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let size = data.len() as u32;
        let name_bytes = name.as_bytes();
        // Local file header.
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);
        // Central directory entry.
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&0u16.to_le_bytes()); // method
        central.extend_from_slice(&0u16.to_le_bytes()); // time
        central.extend_from_slice(&0x21u16.to_le_bytes()); // date
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra length
        central.extend_from_slice(&0u16.to_le_bytes()); // comment length
        central.extend_from_slice(&0u16.to_le_bytes()); // disk number
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    // End of central directory.
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
