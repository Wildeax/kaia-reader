//! Metadata and cover extraction through `kaia_doc`.
//!
//! A cover comes from the book's own image when the book carries one (EPUB
//! and CBZ). Other books, and books whose image is missing or does not
//! decode, get a render of the first page.
//!
//! Every call into the document engine runs on the thread that calls
//! [`Library::scan`](crate::Library::scan). The document is opened, read and
//! dropped inside one call, so no document crosses a thread boundary.

use std::fs::File;
use std::io::{Cursor, Read};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::{Duration, Instant};

use image::codecs::jpeg::{JpegDecoder, JpegEncoder};
use image::metadata::Orientation;
use image::{ExtendedColorType, ImageDecoder, ImageEncoder};
use kaia_gfx::image::{decode_rgb, fit_pixmap, DecodeLimits, FitMode};
use kaia_gfx::{PixelFormat, Pixmap, Size};
use zip::ZipArchive;

/// MIME type of every cover this build writes.
pub(crate) const COVER_MIME: &str = "image/jpeg";

/// JPEG quality of small covers and of covers re-encoded from PNG.
pub(crate) const COVER_JPEG_QUALITY: u8 = 85;

/// JPEG quality of large covers. A large cover is the biggest stored image, so
/// it takes the lower quality. Quality 80 is about 10% smaller than 85 on a
/// sample of ten books.
pub(crate) const LARGE_JPEG_QUALITY: u8 = 80;

/// Box for the large cover, the size of the sleep screen. A cover is scaled
/// down to fit and never enlarged. The page render fits the page into this box.
pub(crate) const LARGE_BOX: Size = Size { w: 1264, h: 1680 };
/// Box for the small cover, scaled from the large one. Never enlarged either.
pub(crate) const SMALL_BOX: Size = Size { w: 160, h: 160 };

/// Limits for a cover image that a book carries. The app decodes covers with
/// the kaia-gfx default, which allows 16.7 Mpx. The scan uses 6 Mpx instead.
/// The decoder peaks at about 11 bytes a pixel for an RGBA PNG, so an image at
/// the limit needs about 66 MB. A larger image is refused, and the cover falls
/// back to the page render.
pub(crate) const COVER_SOURCE_LIMITS: DecodeLimits = DecodeLimits {
    max_width: 4096,
    max_height: 4096,
    max_pixels: 6_000_000,
};

/// Largest encoded cover image read from a book. An entry over the cap gives
/// no cover.
const MAX_COVER_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) struct Cover {
    /// Size of the source image before scaling. The source is the book's cover
    /// image or the page render.
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
    // The image the book carries is sharper than a page render, so it comes first.
    let cover = book_cover_image(path)
        .and_then(|source| make_cover(source).ok())
        .or_else(|| doc.cover(LARGE_BOX).and_then(|page| make_cover(page).ok()));
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

/// Reads the series from the EPUB's package document.
fn epub_series(path: &Path) -> Option<(Option<String>, Option<f64>)> {
    let mut archive = ZipArchive::new(File::open(path).ok()?).ok()?;
    let (_, opf) = epub_package(&mut archive)?;
    series_from_opf(&opf)
}

/// Reads the EPUB's package document. Returns its path in the archive and its
/// text. The path comes from `container.xml`. It names an entry from the
/// archive root, and the path is used as written.
fn epub_package(archive: &mut ZipArchive<File>) -> Option<(String, String)> {
    let container = zip_entry(archive, "META-INF/container.xml", MAX_METADATA_BYTES)?;
    let container = roxmltree::Document::parse(std::str::from_utf8(&container).ok()?).ok()?;
    let opf_path = container
        .descendants()
        .find(|node| node.has_tag_name("rootfile"))?
        .attribute("full-path")?
        .to_string();
    let opf = zip_entry(archive, &opf_path, MAX_METADATA_BYTES)?;
    Some((opf_path, String::from_utf8(opf).ok()?))
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
    // The number is the `group-position` refinement of the collection, as EPUB 3
    // defines it.
    let refines = collection_id.map(|id| format!("#{id}"));
    let index = metas.iter().find_map(|meta| {
        (meta.attribute("property") == Some("group-position")
            && refines
                .as_deref()
                .is_some_and(|r| meta.attribute("refines") == Some(r)))
        .then(|| meta.text().and_then(parse_series_index))
        .flatten()
    });
    Some((Some(series), index))
}

fn parse_series_index(value: &str) -> Option<f64> {
    let index = value.trim().parse::<f64>().ok()?;
    index.is_finite().then_some(index)
}

/// Largest decompressed size of a metadata file read from an EPUB. Metadata
/// files are far smaller, so a bigger entry counts as corrupt.
const MAX_METADATA_BYTES: u64 = 16 * 1024 * 1024;

/// Reads one entry as bytes. The read stops one byte past `cap`, so an entry
/// over the cap is found without reading all of it. Returns None when the entry
/// is missing, corrupt (a CRC mismatch counts) or over the cap.
fn zip_entry(archive: &mut ZipArchive<File>, name: &str, cap: u64) -> Option<Vec<u8>> {
    let entry = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    entry.take(cap + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= cap).then_some(bytes)
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The cover image that the book carries, decoded. None when the book carries
/// none, the image is missing, it is over the limits or it is not PNG or JPEG.
fn book_cover_image(path: &Path) -> Option<Pixmap> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "epub" => epub_cover_image(path),
        "cbz" => cbz_cover_image(path),
        _ => None,
    }
}

/// The cover image that the EPUB's package declares. EPUB 3 marks the image
/// with `properties="cover-image"`. EPUB 2 names it in `<meta name="cover">`.
/// When both are present, the EPUB 3 mark wins.
fn epub_cover_image(path: &Path) -> Option<Pixmap> {
    let mut archive = ZipArchive::new(File::open(path).ok()?).ok()?;
    let (opf_path, opf) = epub_package(&mut archive)?;
    let href = declared_cover_href(&opf)?;
    let name = archive_path(&opf_path, &href);
    decode_cover(&zip_entry(&mut archive, &name, MAX_COVER_BYTES)?)
}

/// The href of the cover image in the package's manifest. See
/// [`epub_cover_image`] for how the package declares it.
fn declared_cover_href(opf_xml: &str) -> Option<String> {
    let opf = roxmltree::Document::parse(opf_xml).ok()?;
    let items: Vec<_> = opf
        .descendants()
        .filter(|node| node.has_tag_name("item") && node.attribute("href").is_some())
        .collect();
    let marked = items.iter().find(|item| {
        item.attribute("properties")
            .is_some_and(|props| props.split_ascii_whitespace().any(|prop| prop == "cover-image"))
    });
    let by_meta = || {
        let id = opf
            .descendants()
            .find(|node| node.has_tag_name("meta") && node.attribute("name") == Some("cover"))?
            .attribute("content")?;
        items.iter().find(|item| item.attribute("id") == Some(id))
    };
    marked.or_else(by_meta)?.attribute("href").map(str::to_string)
}

/// The archive path of a manifest href. An href is relative to the folder of
/// the package document, and it is percent-encoded. A leading slash counts
/// from the archive root. Query strings and fragments are dropped.
fn archive_path(opf_path: &str, href: &str) -> String {
    let href = percent_decode(href.split(['#', '?']).next().unwrap_or_default());
    let dir = match opf_path.rsplit_once('/') {
        Some((dir, _)) if !href.starts_with('/') => dir,
        _ => "",
    };
    let mut segments: Vec<&str> = Vec::new();
    for part in dir.split('/').chain(href.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            part => segments.push(part),
        }
    }
    segments.join("/")
}

/// Decodes `%XX` escapes. An escape that is not two hex digits stays as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(byte) = bytes.get(i + 1..i + 3).and_then(hex_byte) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The byte that two hex digits name, or None for any other two bytes.
fn hex_byte(pair: &[u8]) -> Option<u8> {
    if pair.iter().all(u8::is_ascii_hexdigit) {
        u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()
    } else {
        None
    }
}

/// The first page of a CBZ, in name order. See [`is_cbz_page`].
fn cbz_cover_image(path: &Path) -> Option<Pixmap> {
    let mut archive = ZipArchive::new(File::open(path).ok()?).ok()?;
    let mut pages: Vec<String> = archive
        .file_names()
        .filter_map(|name| name.ok())
        .filter(|name| is_cbz_page(name))
        .map(|name| name.into_owned())
        .collect();
    pages.sort();
    let first = pages.first()?;
    decode_cover(&zip_entry(&mut archive, first, MAX_COVER_BYTES)?)
}

/// True for an entry that is a page of a CBZ: a PNG or JPEG file that is not
/// in a hidden name or a macOS resource fork.
fn is_cbz_page(name: &str) -> bool {
    if name.ends_with('/')
        || name
            .split('/')
            .any(|part| part.starts_with('.') || part == "__MACOSX")
    {
        return false;
    }
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png"))
}

/// Decodes a cover image within [`COVER_SOURCE_LIMITS`]. The decoder reads the
/// size from the header and refuses an image over the limits before it
/// allocates pixels. A JPEG whose EXIF orientation is not upright is turned
/// upright.
fn decode_cover(bytes: &[u8]) -> Option<Pixmap> {
    let pm = decode_rgb(bytes, COVER_SOURCE_LIMITS).ok()?;
    match jpeg_orientation(bytes) {
        Orientation::NoTransforms => Some(pm),
        orientation => apply_orientation(pm, orientation),
    }
}

/// The EXIF orientation of a JPEG. Other formats, and JPEGs whose tag cannot be
/// read, count as upright. The decoder copies its input, so other formats skip
/// it. A JPEG starts with the start of image marker, FF D8.
fn jpeg_orientation(bytes: &[u8]) -> Orientation {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return Orientation::NoTransforms;
    }
    JpegDecoder::new(Cursor::new(bytes))
        .and_then(|mut decoder| decoder.orientation())
        .unwrap_or(Orientation::NoTransforms)
}

/// Turns a pixmap as its EXIF orientation says. None when the pixmap does not
/// hold a valid RGB image.
fn apply_orientation(pm: Pixmap, orientation: Orientation) -> Option<Pixmap> {
    let rgb = image::RgbImage::from_raw(pm.width(), pm.height(), pm.data().to_vec())?;
    let mut img = image::DynamicImage::ImageRgb8(rgb);
    img.apply_orientation(orientation);
    let turned = img.into_rgb8();
    let (w, h) = turned.dimensions();
    Pixmap::from_data(w, h, PixelFormat::Rgb24, turned.into_raw())
}

/// True when the pixmap fits inside `bound` in both directions.
fn fits(pm: &Pixmap, bound: Size) -> bool {
    pm.width() <= bound.w && pm.height() <= bound.h
}

/// The pixmap scaled down to fit `bound`. A pixmap that already fits is
/// returned at its own size.
fn fit_no_enlarge(pm: &Pixmap, bound: Size) -> Pixmap {
    if fits(pm, bound) {
        pm.clone()
    } else {
        fit_pixmap(pm, bound, FitMode::Contain)
    }
}

/// Fits the cover image into both boxes and encodes each as JPEG. A source
/// that is smaller than a box keeps its own size.
fn make_cover(source: Pixmap) -> Result<Cover, String> {
    let (source_w, source_h) = (source.width(), source.height());
    let large = fit_no_enlarge(&source, LARGE_BOX);
    let small = fit_no_enlarge(&large, SMALL_BOX);
    Ok(Cover {
        source_w,
        source_h,
        small: encode_jpeg(&small, COVER_JPEG_QUALITY)?,
        large: encode_jpeg(&large, LARGE_JPEG_QUALITY)?,
    })
}

/// Encodes an Rgb24 pixmap as a JPEG at `quality`. Color is kept.
pub(crate) fn encode_jpeg(pm: &Pixmap, quality: u8) -> Result<Vec<u8>, String> {
    if pm.format() != PixelFormat::Rgb24 {
        return Err("covers are encoded from Rgb24 pixmaps".to_string());
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .write_image(pm.data(), pm.width(), pm.height(), ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Re-encodes a stored PNG cover as JPEG at the same size. None when the bytes
/// do not decode.
pub(crate) fn reencode_png(png: &[u8]) -> Option<Vec<u8>> {
    let pm = decode_rgb(png, DecodeLimits::default()).ok()?;
    encode_jpeg(&pm, COVER_JPEG_QUALITY).ok()
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::Path;

    use kaia_gfx::image::{decode_rgb, DecodeLimits, ImageError};
    use kaia_gfx::{PixelFormat, Pixmap};
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::{
        archive_path, declared_cover_href, decode_cover, epub_series, is_cbz_page, make_cover,
        percent_decode, series_from_opf, COVER_SOURCE_LIMITS,
    };

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
    fn reads_epub_three_group_position_as_the_series_number() {
        let xml = r##"<package xmlns="http://www.idpf.org/2007/opf"><metadata>
            <meta id="series" property="belongs-to-collection">Earthsea</meta>
            <meta refines="#series" property="collection-type">series</meta>
            <meta refines="#series" property="group-position">2.5</meta>
            <meta refines="#other" property="group-position">9</meta>
        </metadata></package>"##;
        assert_eq!(series_from_opf(xml), Some((Some("Earthsea".into()), Some(2.5))));
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

    #[test]
    fn hrefs_resolve_against_the_package_folder() {
        assert_eq!(
            archive_path("OEBPS/content.opf", "images/cover.jpg"),
            "OEBPS/images/cover.jpg"
        );
        assert_eq!(archive_path("OEBPS/content.opf", "../cover.jpg"), "cover.jpg");
        assert_eq!(archive_path("content.opf", "cover.jpeg"), "cover.jpeg");
        assert_eq!(
            archive_path("OEBPS/content.opf", "/META-INF/cover.png"),
            "META-INF/cover.png"
        );
        assert_eq!(
            archive_path("OEBPS/content.opf", "./img/a.png?v=2#top"),
            "OEBPS/img/a.png"
        );
        assert_eq!(
            archive_path("OEBPS/content.opf", "img/front%20cover.png"),
            "OEBPS/img/front cover.png"
        );
    }

    #[test]
    fn malformed_percent_escapes_stay_as_written() {
        assert_eq!(percent_decode("a%20b%2Fc"), "a b/c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%+1"), "%+1");
    }

    #[test]
    fn epub_three_cover_mark_wins_over_the_epub_two_meta() {
        let both = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata>
            <meta name="cover" content="old" /></metadata><manifest>
            <item id="old" href="old.jpg" media-type="image/jpeg" />
            <item id="new" href="cover.png" media-type="image/png" properties="nav cover-image" />
            </manifest></package>"#;
        assert_eq!(declared_cover_href(both).as_deref(), Some("cover.png"));

        let meta_only = r#"<package xmlns="http://www.idpf.org/2007/opf"><metadata>
            <meta name="cover" content="front" /></metadata><manifest>
            <item id="front" href="images/front.jpg" media-type="image/jpeg" />
            </manifest></package>"#;
        assert_eq!(
            declared_cover_href(meta_only).as_deref(),
            Some("images/front.jpg")
        );

        let none = r#"<package xmlns="http://www.idpf.org/2007/opf"><manifest>
            <item id="text" href="text.xhtml" media-type="application/xhtml+xml" /></manifest></package>"#;
        assert_eq!(declared_cover_href(none), None);
    }

    #[test]
    fn cbz_pages_skip_folders_hidden_names_and_resource_forks() {
        assert!(is_cbz_page("001.jpg"));
        assert!(is_cbz_page("chapter 2/PAGE.PNG"));
        assert!(!is_cbz_page("chapter 2/"));
        assert!(!is_cbz_page(".001.jpg"));
        assert!(!is_cbz_page("__MACOSX/001.jpg"));
        assert!(!is_cbz_page("ComicInfo.xml"));
    }

    #[test]
    fn images_over_the_limits_are_refused_from_the_header() {
        // One image is too wide, the other has too many pixels in total.
        for (w, h) in [(4097, 2), (2600, 2600)] {
            let png = kaia_gfx::png::encode_pixmap(&Pixmap::new(w, h, PixelFormat::Gray8)).unwrap();
            assert!(
                matches!(
                    decode_rgb(&png, COVER_SOURCE_LIMITS),
                    Err(ImageError::TooLarge { .. })
                ),
                "{w}x{h} is refused"
            );
            assert!(decode_cover(&png).is_none(), "{w}x{h} gives no cover");
        }
        let small = kaia_gfx::png::encode_pixmap(&Pixmap::new(64, 96, PixelFormat::Rgb24)).unwrap();
        assert!(decode_cover(&small).is_some());
    }

    #[test]
    fn a_source_smaller_than_the_boxes_keeps_its_size() {
        let cover = make_cover(Pixmap::new(100, 150, PixelFormat::Rgb24)).unwrap();
        assert_eq!((cover.source_w, cover.source_h), (100, 150));
        assert_eq!(jpeg_size(&cover.large), (100, 150));
        assert_eq!(jpeg_size(&cover.small), (100, 150));
    }

    #[test]
    fn a_larger_source_fits_inside_both_boxes() {
        let cover = make_cover(Pixmap::new(1600, 2400, PixelFormat::Rgb24)).unwrap();
        assert_eq!((cover.source_w, cover.source_h), (1600, 2400));
        assert_eq!(jpeg_size(&cover.large), (1120, 1680));
        assert_eq!(jpeg_size(&cover.small), (107, 160));
    }

    fn jpeg_size(bytes: &[u8]) -> (u32, u32) {
        let pm = decode_rgb(bytes, DecodeLimits::default()).unwrap();
        (pm.width(), pm.height())
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
