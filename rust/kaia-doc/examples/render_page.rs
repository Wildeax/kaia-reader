//! Renders one page of a book to a binary PGM (Gray8) file.
//!
//! Usage: `render_page <book> <page> <out.pgm>`
//!
//! `page` is 0-based, as in the `Document` trait. Reflowable books (EPUB,
//! FB2, ...) are laid out at 1264x1680 pixels. Fixed layouts (PDF) keep their
//! aspect ratio and are scaled to fit the same box. The page is rendered with
//! the engine that `kaia_doc::open` picks for the file.
//!
//! Font and data locations: MuPDF reads its fallback faces from `FONTDIR`.
//! CREngine finds `fonts` and `data` beside the executable, with the desktop
//! build tree as a fallback. Use `CreDocument::open_with_options` to supply
//! other resource folders.

use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;

use kaia_doc::{Document, Layout};
use kaia_gfx::{PixelFormat, Pixmap};

const WIDTH: u32 = 1264;
const HEIGHT: u32 = 1680;
/// Font size in pixels, the same default CREngine uses in kaia-doc.
const EM: f32 = 24.0;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: render_page <book> <page> <out.pgm>");
        return ExitCode::from(2);
    }
    match run(Path::new(&args[1]), &args[2], Path::new(&args[3])) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("render_page: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(book: &Path, page_arg: &str, out: &Path) -> Result<(), String> {
    let page: u32 = page_arg
        .parse()
        .map_err(|_| format!("page must be a number, got {page_arg:?}"))?;
    let mut doc = kaia_doc::open(book).map_err(|e| format!("{}: {e}", book.display()))?;
    let reflowable = doc.is_reflowable();
    if reflowable {
        doc.set_layout(&Layout::new(WIDTH as f32, HEIGHT as f32, EM))
            .map_err(|e| format!("layout: {e}"))?;
    }
    let count = doc.page_count();
    if page as usize >= count {
        return Err(format!("page {page} is out of range, the book has {count} pages"));
    }
    let scale = if reflowable {
        1.0
    } else {
        fit_scale(doc.as_ref(), page)?
    };
    let pixmap = doc
        .render(page, scale, PixelFormat::Gray8)
        .map_err(|e| format!("render page {page}: {e}"))?;
    write_pgm(out, &pixmap).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{}: page {page} of {count}, {}x{} at scale {scale:.3}, reflowable: {reflowable}, wrote {}",
        book.display(),
        pixmap.width(),
        pixmap.height(),
        out.display()
    );
    Ok(())
}

/// Scale (1.0 is 72 dpi) that fits the page in the output box.
fn fit_scale(doc: &dyn Document, page: u32) -> Result<f32, String> {
    let size = doc.page_size(page).map_err(|e| format!("page size: {e}"))?;
    if size.width <= 0.0 || size.height <= 0.0 {
        return Err(format!("page {page} has no size"));
    }
    Ok((WIDTH as f32 / size.width).min(HEIGHT as f32 / size.height))
}

/// Writes a Gray8 pixmap as a binary PGM (P5, maxval 255).
fn write_pgm(path: &Path, pixmap: &Pixmap) -> std::io::Result<()> {
    if pixmap.format() != PixelFormat::Gray8 {
        return Err(std::io::Error::other("pixmap is not Gray8"));
    }
    let (w, h, stride) = (pixmap.width() as usize, pixmap.height() as usize, pixmap.stride());
    let mut out = BufWriter::new(File::create(path)?);
    write!(out, "P5\n{w} {h}\n255\n")?;
    for row in pixmap.data().chunks(stride).take(h) {
        out.write_all(&row[..w])?;
    }
    out.flush()
}
