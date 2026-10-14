//! Generated books shared by the document, session and reader tests.
#![allow(dead_code)]
use std::path::Path;

/// Writes a ZIP with stored entries. EPUB fixtures need no compression library.
pub fn zip(path: &Path, files: &[(&str, &[u8])]) {
    fn u16(out: &mut Vec<u8>, n: u16) {
        out.extend(n.to_le_bytes());
    }
    fn u32(out: &mut Vec<u8>, n: u32) {
        out.extend(n.to_le_bytes());
    }
    fn crc(bytes: &[u8]) -> u32 {
        let mut n = !0u32;
        for b in bytes {
            n ^= *b as u32;
            for _ in 0..8 {
                n = (n >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(n & 1)));
            }
        }
        !n
    }
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for (name, bytes) in files {
        let offset = data.len() as u32;
        u32(&mut data, 0x04034b50);
        for n in [20, 0, 0, 0, 0] {
            u16(&mut data, n);
        }
        u32(&mut data, crc(bytes));
        u32(&mut data, bytes.len() as u32);
        u32(&mut data, bytes.len() as u32);
        u16(&mut data, name.len() as u16);
        u16(&mut data, 0);
        data.extend(name.as_bytes());
        data.extend(*bytes);
        u32(&mut directory, 0x02014b50);
        for n in [20, 20, 0, 0, 0, 0] {
            u16(&mut directory, n);
        }
        u32(&mut directory, crc(bytes));
        u32(&mut directory, bytes.len() as u32);
        u32(&mut directory, bytes.len() as u32);
        for n in [name.len() as u16, 0, 0, 0, 0] {
            u16(&mut directory, n);
        }
        u32(&mut directory, 0);
        u32(&mut directory, offset);
        directory.extend(name.as_bytes());
    }
    let offset = data.len() as u32;
    let size = directory.len() as u32;
    data.extend(directory);
    u32(&mut data, 0x06054b50);
    for n in [0, 0, files.len() as u16, files.len() as u16] {
        u16(&mut data, n);
    }
    u32(&mut data, size);
    u32(&mut data, offset);
    u16(&mut data, 0);
    std::fs::write(path, data).unwrap();
}

pub fn epub(path: &Path) {
    let content = format!(
        r##"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Reader links</title></head><body>
<h1>Reader links</h1>
<p>Read a note <a id="ref" epub:type="noteref" href="#note">1</a> here.</p>
<p>Read an aside <a href="#aside">Aside</a> here.</p>
<p>Read a short note <a href="#short">3</a> here.</p>
<p><a href="#chapter">Next chapter</a></p>
<p><a href="https://example.org/offline">External address</a></p>
<p><a href="#missing">Broken destination</a></p>
{}
<h1 id="chapter">Second chapter</h1><p>Chapter text.</p>
<aside id="note" epub:type="footnote"><p>First note paragraph.</p><p>Second note paragraph.</p><a href="#ref">Return</a></aside>
<aside id="aside"><p>An aside without a type.</p></aside>
<p id="short">A short numbered note.</p>
<h2>The end</h2><p>Last paragraph.</p>
</body></html>"##,
        "<p>Ordinary text fills this book so that its links reach another page.</p>".repeat(65)
    );
    epub_content(path, &content);
}

pub fn epub_content(path: &Path, content: &str) {
    zip(path, &[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
        ("book.opf", br#"<?xml version="1.0"?><package version="3.0" unique-identifier="id" xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">reader-links-test</dc:identifier><dc:title>Reader links</dc:title><dc:language>en</dc:language></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#),
        ("chapter.xhtml", content.as_bytes()),
    ]);
}

pub fn pdf(path: &Path) {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Annots [5 0 R 6 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] >>",
        "<< /Type /Annot /Subtype /Link /Rect [20 210 150 240] /Dest [4 0 R /Fit] >>",
        "<< /Type /Annot /Subtype /Link /Rect [20 150 150 180] /A << /S /URI /URI (https://example.org/offline) >> >>",
    ];
    let mut data = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data += &format!("{} 0 obj\n{object}\nendobj\n", i + 1);
    }
    let xref = data.len();
    data += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        data += &format!("{offset:010} 00000 n \n");
    }
    data += &format!(
        "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    std::fs::write(path, data).unwrap();
}
