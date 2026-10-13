//! Metadata and cover extraction through `kaia_doc`.
//!
//! Every call into the document engine runs on the thread that calls
//! [`Library::scan`](crate::Library::scan). The document is opened, read and
//! dropped inside one call, so no document crosses a thread boundary.

use std::fs::File;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder};
use kaia_gfx::image::{decode_rgb, fit_pixmap, DecodeLimits, FitMode};
use kaia_gfx::{PixelFormat, Pixmap, Size};
use zip::ZipArchive;

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
    pub series: Option<String>,
    pub series_index: Option<f64>,
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

    let (series, series_index) = if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
    {
        epub_series(path).unwrap_or_default()
    } else {
        (None, None)
    };

    Ok(Extracted {
        title: non_empty(meta.title),
        authors: non_empty(meta.author),
        series,
        series_index,
        pages: u32::try_from(pages).unwrap_or(u32::MAX),
        cover,
    })
}

/// Reads the series from the EPUB's package document. The package path comes
/// from `container.xml`. It names an entry from the archive root, and the path
/// is used as written.
fn epub_series(path: &Path) -> Option<(Option<String>, Option<f64>)> {
    let mut archive = ZipArchive::new(File::open(path).ok()?).ok()?;
    let container = zip_entry(&mut archive, "META-INF/container.xml")?;
    let container = roxmltree::Document::parse(std::str::from_utf8(&container).ok()?).ok()?;
    let opf_path = container
        .descendants()
        .find(|node| node.has_tag_name("rootfile"))?
        .attribute("full-path")?;
    let opf = zip_entry(&mut archive, opf_path)?;
    series_from_opf(std::str::from_utf8(&opf).ok()?)
}

fn series_from_opf(xml: &str) -> Option<(Option<String>, Option<f64>)> {
    let opf = roxmltree::Document::parse(xml).ok()?;
    let metas: Vec<_> = opf
        .descendants()
        .filter(|node| node.has_tag_name("meta"))
        .collect();

    let calibre_series = metas.iter().find_map(|meta| {
        (meta.attribute("name")? == "calibre:series")
            .then(|| {
                meta.attribute("content")
                    .map(str::to_string)
                    .and_then(|value| non_empty(Some(value)))
            })
            .flatten()
    });
    let calibre_index = metas.iter().find_map(|meta| {
        (meta.attribute("name")? == "calibre:series_index")
            .then(|| meta.attribute("content").and_then(parse_series_index))
            .flatten()
    });
    if calibre_series.is_some() {
        return Some((calibre_series, calibre_index));
    }

    let collection = metas.iter().find(|meta| {
        meta.attribute("property") == Some("belongs-to-collection")
            && meta.text().is_some_and(|text| !text.trim().is_empty())
    })?;
    let collection_id = collection.attribute("id");
    let collection_is_series = collection_id.is_some_and(|id| {
        metas.iter().any(|meta| {
            meta.attribute("property") == Some("collection-type")
                && meta
                    .attribute("refines")
                    .is_some_and(|refines| refines == format!("#{id}"))
                && meta
                    .text()
                    .is_some_and(|text| text.trim().eq_ignore_ascii_case("series"))
        })
    });
    // EPUB 3 permits collection metadata without a type declaration. When a
    // type is present, only accept entries explicitly marked as a series.
    let any_collection_type = metas
        .iter()
        .any(|meta| meta.attribute("property") == Some("collection-type"));
    if any_collection_type && !collection_is_series {
        return None;
    }
    let series = collection
        .text()
        .map(str::to_string)
        .and_then(|value| non_empty(Some(value)))?;
    Some((Some(series), None))
}

fn parse_series_index(value: &str) -> Option<f64> {
    let index = value.trim().parse::<f64>().ok()?;
    index.is_finite().then_some(index)
}

/// Largest decompressed size of an entry read from an EPUB. Metadata files are
/// far smaller, so a bigger entry counts as corrupt.
const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;

/// Reads one entry as bytes. The read stops one byte past the cap, so an entry
/// over the cap is found without reading all of it. Returns None when the entry
/// is missing, corrupt (a CRC mismatch counts) or over the cap.
fn zip_entry(archive: &mut ZipArchive<File>, name: &str) -> Option<Vec<u8>> {
    let entry = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    entry.take(MAX_METADATA_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_METADATA_BYTES).then_some(bytes)
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

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::Path;

    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::{epub_series, series_from_opf};

    const CONTAINER_XML: &[u8] =
        br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;
    const OPF_XML: &[u8] = br#"<package><metadata><meta name="calibre:series" content="The Expanse"/><meta name="calibre:series_index" content="2"/></metadata></package>"#;

    #[test]
    fn reads_calibre_series_metadata() {
        let xml = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata>
            <meta name="calibre:series" content="The Expanse" />
            <meta name="calibre:series_index" content="2.5" />
        </metadata></package>"#;
        assert_eq!(
            series_from_opf(xml),
            Some((Some("The Expanse".into()), Some(2.5)))
        );
    }

    #[test]
    fn reads_epub_three_series_collection() {
        let xml = r##"<package xmlns="http://www.idpf.org/2007/opf"><metadata>
            <meta id="series" property="belongs-to-collection">Earthsea</meta>
            <meta refines="#series" property="collection-type">series</meta>
        </metadata></package>"##;
        assert_eq!(series_from_opf(xml), Some((Some("Earthsea".into()), None)));
    }

    #[test]
    fn reads_series_from_an_epub_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("series.epub");
        write_zip(
            &path,
            &[
                ("META-INF/container.xml", CONTAINER_XML, CompressionMethod::Stored),
                ("OEBPS/content.opf", OPF_XML, CompressionMethod::Deflated),
            ],
        );
        assert_eq!(epub_series(&path), Some((Some("The Expanse".into()), Some(2.0))));
    }

    #[test]
    fn corrupt_or_truncated_epub_gives_no_series() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.epub");
        write_zip(
            &good,
            &[
                ("META-INF/container.xml", CONTAINER_XML, CompressionMethod::Stored),
                ("OEBPS/content.opf", OPF_XML, CompressionMethod::Stored),
            ],
        );
        let bytes = fs::read(&good).unwrap();

        // Cut the file before the end-of-central-directory record.
        let truncated = dir.path().join("truncated.epub");
        fs::write(&truncated, &bytes[..bytes.len() / 2]).unwrap();

        // Change one byte of the stored OPF. The CRC no longer matches.
        let needle = b"The Expanse";
        let at = bytes.windows(needle.len()).position(|w| w == needle).unwrap();
        let mut damaged = bytes.clone();
        damaged[at] = b'X';
        let damaged_path = dir.path().join("damaged.epub");
        fs::write(&damaged_path, &damaged).unwrap();

        let not_zip = dir.path().join("not-a-zip.epub");
        fs::write(&not_zip, b"PK\x03\x04 this is not a zip archive").unwrap();

        for path in [&truncated, &damaged_path, &not_zip] {
            assert_eq!(epub_series(path), None, "{}", path.display());
        }
    }

    /// Writes a ZIP archive. Each entry is a name, its bytes and a compression method.
    fn write_zip(path: &Path, entries: &[(&str, &[u8], CompressionMethod)]) {
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        for (name, bytes, method) in entries {
            let options = SimpleFileOptions::default().compression_method(*method);
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }
}
