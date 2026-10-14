//! Document trait and engine backends.
//!
//! [`open`] picks a backend for a file and returns it behind the [`Document`]
//! trait. The reader talks only to the trait. The backends are:
//!
//! - CREngine (feature `cre`, default on): KOReader's typography for EPUB,
//!   FB2, HTML, TXT, RTF, DOC and CHM. See [`cre`].
//! - DjVuLibre (feature `djvu`, default on): DjVu. See [`djvu`].
//! - libarchive: RAR and RAR5 comics with PNG or JPEG pages. See [`cbr`].
//! - MuPDF: PDF, CBZ, XPS, MOBI, SVG, and EPUB or FB2 when asked for with
//!   [`open_with`] and [`Engine::Mupdf`].
//!
//! This crate has no `unsafe` code. The C bindings live in `kaia-mupdf-sys`,
//! `kaia-cre-sys`, `kaia-djvu-sys`, and `kaia-archive-sys`.

pub mod cbr;
#[cfg(feature = "djvu")]
pub mod djvu;
pub mod document;
pub mod error;
pub mod format;
pub mod mupdf;
pub mod types;

#[cfg(feature = "cre")]
pub mod cre;

use std::path::Path;

pub use cbr::CbrDocument;
#[cfg(feature = "djvu")]
pub use djvu::DjvuDocument;
pub use document::Document;
pub use error::{DocError, Result};
pub use format::Format;
pub use mupdf::MupdfDocument;
pub use types::{
    Layout, Link, LinkTarget, Location, Margins, Metadata, PageSize, PageText, PointF, Quad, RectF, TextChar,
    TextLine, TocEntry,
};

/// Which engine opens a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// CREngine for the formats it handles, MuPDF for the rest.
    Auto,
    /// CREngine. Fails when the feature is off or the format is not one it handles.
    Cre,
    /// MuPDF. Fails for formats it does not handle.
    Mupdf,
}

/// Extensions that only CREngine opens. MuPDF has no format for them.
const CRE_ONLY_EXTENSIONS: &[&str] = &["txt", "rtf", "doc", "chm"];

/// Opens a document with the engine that suits its format. See [`Engine::Auto`].
pub fn open(path: &Path) -> Result<Box<dyn Document>> {
    open_with(path, Engine::Auto)
}

/// Opens a document with the given engine.
pub fn open_with(path: &Path, engine: Engine) -> Result<Box<dyn Document>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if CRE_ONLY_EXTENSIONS.contains(&ext.as_str()) {
        return match engine {
            Engine::Mupdf => Err(DocError::UnknownFormat(path.to_path_buf())),
            _ => open_cre(path, None),
        };
    }
    let format = format::detect(path)?;
    if format == Format::Cbr {
        if engine != Engine::Auto {
            return Err(DocError::UnknownFormat(path.to_path_buf()));
        }
        return Ok(Box::new(CbrDocument::open(path)?));
    }
    if format == Format::Djvu {
        return open_djvu(path, engine);
    }
    let use_cre = match engine {
        Engine::Cre => true,
        Engine::Mupdf => false,
        Engine::Auto => cre_kind_for(format).is_some(),
    };
    if use_cre {
        open_cre(path, cre_kind_for(format))
    } else {
        Ok(Box::new(MupdfDocument::open(path, format)?))
    }
}

/// The CREngine kind for a format that CREngine opens by default. None means
/// MuPDF.
#[cfg(feature = "cre")]
fn cre_kind_for(format: Format) -> Option<cre::Kind> {
    match format {
        Format::Epub => Some(cre::Kind::Epub),
        Format::Fb2 => Some(cre::Kind::Fb2),
        Format::Html => Some(cre::Kind::Html),
        _ => None,
    }
}

#[cfg(not(feature = "cre"))]
fn cre_kind_for(_format: Format) -> Option<()> {
    None
}

/// Opens with CREngine. The kind comes from the detected format when there is
/// one, and from the extension otherwise.
#[cfg(feature = "cre")]
fn open_cre(path: &Path, kind: Option<cre::Kind>) -> Result<Box<dyn Document>> {
    let doc = match kind {
        Some(kind) => cre::CreDocument::open_as(path, kind)?,
        None => cre::CreDocument::open(path)?,
    };
    Ok(Box::new(doc))
}

#[cfg(not(feature = "cre"))]
fn open_cre(path: &Path, _kind: Option<()>) -> Result<Box<dyn Document>> {
    Err(DocError::InvalidArgument(format!(
        "CREngine is not built in (feature cre): {}",
        path.display()
    )))
}

#[cfg(feature = "djvu")]
fn open_djvu(path: &Path, engine: Engine) -> Result<Box<dyn Document>> {
    if engine != Engine::Auto {
        return Err(DocError::UnknownFormat(path.to_path_buf()));
    }
    Ok(Box::new(DjvuDocument::open(path)?))
}

#[cfg(not(feature = "djvu"))]
fn open_djvu(path: &Path, _engine: Engine) -> Result<Box<dyn Document>> {
    Err(DocError::UnknownFormat(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_file(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
    }

    /// MuPDF reads its fallback faces from FONTDIR. The repository's
    /// resources/fonts has every face it asks for.
    fn point_mupdf_at_fonts() {
        if let Some(dir) = option_env!("KAIA_DOC_FONT_DIR") {
            std::env::set_var("FONTDIR", dir);
        }
    }

    #[test]
    fn epub_opens_with_cre_by_default_and_with_mupdf_on_request() {
        point_mupdf_at_fonts();
        let epub = repo_file("test/juliet.epub");
        assert!(open(&epub).expect("auto").page_count() > 1);
        assert!(open_with(&epub, Engine::Cre).expect("cre").page_count() > 1);
        let mupdf = open_with(&epub, Engine::Mupdf).expect("mupdf");
        assert!(mupdf.page_count() > 1);
    }

    #[test]
    fn text_files_need_cre() {
        let txt = repo_file("test/sample.txt");
        assert!(open(&txt).expect("auto").page_count() >= 1);
        assert!(open_with(&txt, Engine::Mupdf).is_err());
    }

    #[test]
    fn pdf_stays_on_mupdf() {
        point_mupdf_at_fonts();
        let pdf = repo_file("test/sample.pdf");
        assert!(open(&pdf).expect("auto").page_count() >= 1);
        assert!(open_with(&pdf, Engine::Cre).is_err());
    }
}
