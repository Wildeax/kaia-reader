//! The document that the engine thread owns. It opens a file with the same
//! engine rules as `kaia_doc::open_with`, but keeps the concrete type, so the
//! CREngine calls that the `Document` trait lacks (xpointers, character boxes,
//! reflow options) stay reachable.
//!
//! The handles are not `Send`, so a value of this type never leaves the
//! engine thread.

use std::path::Path;

use kaia_doc::{format, DocError, Document, Engine, Format, MupdfDocument, PointF, RectF};

#[cfg(feature = "cre")]
use kaia_doc::cre::{CreDocument, Kind};

/// Extensions that only CREngine opens. MuPDF has no format for them.
const CRE_ONLY_EXTENSIONS: &[&str] = &["txt", "rtf", "doc", "chm"];

/// One character box from CREngine: the character, its box and its line number.
pub(crate) type CharBox = (char, RectF, i32);

pub(crate) enum EngineDoc {
    #[cfg(feature = "cre")]
    Cre(CreDocument),
    Mupdf(MupdfDocument),
}

impl EngineDoc {
    /// Opens a file with the given engine policy. Mirrors `kaia_doc::open_with`.
    pub fn open(path: &Path, engine: Engine) -> Result<Self, DocError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if CRE_ONLY_EXTENSIONS.contains(&ext.as_str()) {
            return match engine {
                Engine::Mupdf => Err(DocError::UnknownFormat(path.to_path_buf())),
                _ => Self::open_cre(path, None),
            };
        }
        let format = format::detect(path)?;
        let use_cre = match engine {
            Engine::Cre => true,
            Engine::Mupdf => false,
            Engine::Auto => cre_kind_for(format).is_some(),
        };
        if use_cre {
            Self::open_cre(path, cre_kind_for(format))
        } else {
            Ok(EngineDoc::Mupdf(MupdfDocument::open(path, format)?))
        }
    }

    #[cfg(feature = "cre")]
    fn open_cre(path: &Path, kind: Option<Kind>) -> Result<Self, DocError> {
        let doc = match kind {
            Some(kind) => CreDocument::open_as(path, kind)?,
            None => CreDocument::open(path)?,
        };
        Ok(EngineDoc::Cre(doc))
    }

    #[cfg(not(feature = "cre"))]
    fn open_cre(path: &Path, _kind: Option<()>) -> Result<Self, DocError> {
        Err(DocError::InvalidArgument(format!(
            "CREngine is not built in (feature cre): {}",
            path.display()
        )))
    }

    /// The document through the common trait.
    pub fn doc(&self) -> &dyn Document {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => d,
            EngineDoc::Mupdf(d) => d,
        }
    }

    /// The document through the common trait, for layout changes.
    pub fn doc_mut(&mut self) -> &mut dyn Document {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => d,
            EngineDoc::Mupdf(d) => d,
        }
    }

    /// True when the document is CREngine's.
    pub fn is_cre(&self) -> bool {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(_) => true,
            EngineDoc::Mupdf(_) => false,
        }
    }

    /// The page that holds an xpointer. None for MuPDF documents or when the
    /// xpointer is not in the document.
    #[cfg_attr(not(feature = "cre"), allow(unused_variables))]
    pub fn xpointer_page(&self, xp: &str) -> Result<Option<u32>, DocError> {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => d.xpointer_page(xp),
            EngineDoc::Mupdf(_) => Ok(None),
        }
    }

    /// The xpointer at the top of a page. None for MuPDF documents.
    #[cfg_attr(not(feature = "cre"), allow(unused_variables))]
    pub fn page_xpointer(&self, page: u32) -> Result<Option<String>, DocError> {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => d.page_xpointer(page).map(Some),
            EngineDoc::Mupdf(_) => Ok(None),
        }
    }

    /// The xpointer of the text at a point of a page. None when there is no text there, or for MuPDF.
    #[cfg_attr(not(feature = "cre"), allow(unused_variables))]
    pub fn xpointer_at(&self, page: u32, p: PointF) -> Result<Option<String>, DocError> {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => d.xpointer_at(page, p.x.round() as i32, p.y.round() as i32),
            EngineDoc::Mupdf(_) => Ok(None),
        }
    }

    /// Character boxes from `start` to `end`, under the engine's range rules. None
    /// for MuPDF documents.
    #[cfg_attr(not(feature = "cre"), allow(unused_variables))]
    pub fn char_boxes(
        &self,
        page: u32,
        start: &str,
        end: Option<&str>,
    ) -> Result<Option<Vec<CharBox>>, DocError> {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => Ok(Some(
                d.char_boxes(page, start, end)?
                    .into_iter()
                    .map(|b| {
                        (
                            b.ch,
                            RectF::new(b.rect[0], b.rect[1], b.rect[2], b.rect[3]),
                            b.line,
                        )
                    })
                    .collect(),
            )),
            EngineDoc::Mupdf(_) => Ok(None),
        }
    }

    /// The first visible character at or after `start` on the page, under the
    /// engine's range rules. None when no visible character follows it, or for MuPDF.
    #[cfg_attr(not(feature = "cre"), allow(unused_variables))]
    pub fn first_char_box(&self, page: u32, start: &str) -> Result<Option<CharBox>, DocError> {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => Ok(d.first_char_box(page, start)?.map(|b| {
                (
                    b.ch,
                    RectF::new(b.rect[0], b.rect[1], b.rect[2], b.rect[3]),
                    b.line,
                )
            })),
            EngineDoc::Mupdf(_) => Ok(None),
        }
    }

    /// Sets the CREngine options that the layout does not carry. A no-op for MuPDF.
    #[cfg_attr(not(feature = "cre"), allow(unused_variables))]
    pub fn set_cre_options(&mut self, hyphenation: bool, embedded_styles: bool) -> Result<(), DocError> {
        match self {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => {
                let dict = hyphenation.then_some(crate::layout::HYPHENATION_DICT);
                d.set_hyphenation(dict)?;
                d.set_embedded_styles(embedded_styles)
            }
            EngineDoc::Mupdf(_) => Ok(()),
        }
    }
}

/// The CREngine kind for a format that CREngine opens by default. None means MuPDF.
#[cfg(feature = "cre")]
fn cre_kind_for(format: Format) -> Option<Kind> {
    match format {
        Format::Epub => Some(Kind::Epub),
        Format::Fb2 => Some(Kind::Fb2),
        Format::Html => Some(Kind::Html),
        _ => None,
    }
}

#[cfg(not(feature = "cre"))]
fn cre_kind_for(_format: Format) -> Option<()> {
    None
}
