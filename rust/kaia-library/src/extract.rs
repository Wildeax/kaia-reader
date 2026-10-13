//! Metadata and cover extraction through `kaia_doc`.
//!
//! Every call into the document engine runs on the thread that calls
//! [`Library::scan`](crate::Library::scan). The document is opened, read and
//! dropped inside one call, so no document crosses a thread boundary.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};
use kaia_gfx::image::{decode_rgb, fit_pixmap, DecodeLimits, FitMode};
use kaia_gfx::{PixelFormat, Pixmap, Size};

/// MIME type of every cover this build writes.
pub(crate) const COVER_MIME: &str = "image/jpeg";

/// JPEG quality of stored covers.
pub(crate) const COVER_JPEG_QUALITY: u8 = 85;

/// Box for the large cover. The engine renders the cover to this box, so the
/// decoded raster stays at or below it.
pub(crate) const LARGE_BOX: Size = Size { w: 480, h: 480 };
/// Box for the small cover, scaled from the large one.
pub(crate) const SMALL_BOX: Size = Size { w: 160, h: 160 };

pub(crate) struct Cover {
    /// Size of the rendered cover before thumbnailing.
    pub source_w: u32,
    pub source_h: u32,
    /// JPEG bytes.
    pub small: Vec<u8>,
    /// JPEG bytes.
    pub large: Vec<u8>,
}

pub(crate) struct Extracted {
    pub title: Option<String>,
    pub authors: Option<String>,
    pub pages: u32,
    pub cover: Option<Cover>,
}

/// Runs [`extract`] and turns a panic in the engine into a failure message, so
/// one bad file does not stop the scan. Returns the reason on failure.
pub(crate) fn extract_guarded(path: &Path, budget: Duration) -> Result<Extracted, String> {
    catch_unwind(AssertUnwindSafe(|| extract(path, budget)))
        .unwrap_or_else(|_| Err("the document engine failed unexpectedly".to_string()))
}

/// Opens the document and reads its title, author, page count and cover.
///
/// The budget is checked between engine calls. A single engine call cannot be
/// interrupted, so a call that runs long finishes before the check fails it.
fn extract(path: &Path, budget: Duration) -> Result<Extracted, String> {
    let start = Instant::now();
    let check = || {
        if start.elapsed() >= budget {
            Err(format!(
                "extraction took longer than {:.1} s",
                budget.as_secs_f64()
            ))
        } else {
            Ok(())
        }
    };

    let doc = kaia_doc::open(path).map_err(|e| e.to_string())?;
    check()?;
    let meta = doc.metadata();
    let pages = doc.page_count();
    check()?;
    if pages == 0 {
        return Err("the document has no pages".to_string());
    }
    let cover = doc.cover(LARGE_BOX).and_then(|pm| make_cover(&pm).ok());
    check()?;

    Ok(Extracted {
        title: non_empty(meta.title),
        authors: non_empty(meta.author),
        pages: u32::try_from(pages).unwrap_or(u32::MAX),
        cover,
    })
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Scales the rendered cover into both boxes and encodes each as JPEG.
fn make_cover(pm: &Pixmap) -> Result<Cover, String> {
    let large = fit_pixmap(pm, LARGE_BOX, FitMode::Contain);
    let small = fit_pixmap(&large, SMALL_BOX, FitMode::Contain);
    Ok(Cover {
        source_w: pm.width(),
        source_h: pm.height(),
        small: encode_jpeg(&small)?,
        large: encode_jpeg(&large)?,
    })
}

/// Encodes an Rgb24 pixmap as a JPEG at [`COVER_JPEG_QUALITY`]. Color is kept.
pub(crate) fn encode_jpeg(pm: &Pixmap) -> Result<Vec<u8>, String> {
    if pm.format() != PixelFormat::Rgb24 {
        return Err("covers are encoded from Rgb24 pixmaps".to_string());
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, COVER_JPEG_QUALITY)
        .write_image(pm.data(), pm.width(), pm.height(), ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Re-encodes a stored PNG cover as JPEG at the same size. None when the bytes
/// do not decode.
pub(crate) fn reencode_png(png: &[u8]) -> Option<Vec<u8>> {
    let pm = decode_rgb(png, DecodeLimits::default()).ok()?;
    encode_jpeg(&pm).ok()
}
