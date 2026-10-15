//! EPUB display pages. Each spine item contributes ceil(compressed ZIP bytes / 1024).
//! The fraction of rendered screens through that item selects its logical page.
//! These labels never replace screen indices, saved percent or reading statistics.

use std::{fs::File, io::Read, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalProgress {
    /// One-based book page.
    pub page: u32,
    pub total: u32,
    /// One-based page within the current spine item.
    pub chapter_page: u32,
    pub chapter_total: u32,
}

pub(crate) struct SpinePages(Vec<u32>);

impl SpinePages {
    pub fn open(path: &Path) -> Option<Self> {
        let mut zip = zip::ZipArchive::new(File::open(path).ok()?).ok()?;
        let mut container = String::new();
        zip.by_name("META-INF/container.xml")
            .ok()?
            .read_to_string(&mut container)
            .ok()?;
        let xml = roxmltree::Document::parse(&container).ok()?;
        let package = xml
            .descendants()
            .find(|n| n.has_tag_name("rootfile"))?
            .attribute("full-path")?;
        let base = Path::new(package).parent()?;
        let mut opf = String::new();
        zip.by_name(package).ok()?.read_to_string(&mut opf).ok()?;
        let xml = roxmltree::Document::parse(&opf).ok()?;
        let mut pages = Vec::new();
        for item in xml.descendants().filter(|n| n.has_tag_name("itemref")) {
            let id = item.attribute("idref")?;
            let href = xml
                .descendants()
                .find(|n| n.has_tag_name("item") && n.attribute("id") == Some(id))?
                .attribute("href")?;
            let name = archive_path(base, href)?;
            let size = zip.by_name(name.to_str()?).ok()?.compressed_size();
            pages.push(u32::try_from(size.div_ceil(1024).max(1)).ok()?);
        }
        pages.iter().try_fold(0u32, |sum, page| sum.checked_add(*page))?;
        (!pages.is_empty()).then_some(Self(pages))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn progress(&self, item: usize, fraction: f64) -> Option<LogicalProgress> {
        let total = *self.0.get(item)?;
        let offset = ((fraction.clamp(0.0, 1.0) * f64::from(total)) as u32).min(total - 1);
        Some(LogicalProgress {
            page: self.0[..item].iter().sum::<u32>() + offset + 1,
            total: self.0.iter().sum(),
            chapter_page: offset + 1,
            chapter_total: total,
        })
    }
}

fn archive_path(base: &Path, href: &str) -> Option<std::path::PathBuf> {
    use std::path::Component;
    let href = href.split('#').next()?;
    let decoded = percent_encoding::percent_decode_str(href).decode_utf8().ok()?;
    let joined = base.join(decoded.as_ref());
    let mut path = std::path::PathBuf::new();
    for component in joined.components() {
        match component {
            Component::Normal(name) => path.push(name),
            Component::CurDir => {}
            Component::ParentDir if path.pop() => {}
            _ => return None,
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_links_resolve_relative_and_encoded_archive_names() {
        assert_eq!(
            archive_path(Path::new("OPS/package"), "../text/Chapter%201.xhtml#start").unwrap(),
            Path::new("OPS/text/Chapter 1.xhtml")
        );
        assert!(archive_path(Path::new("OPS"), "../../outside.xhtml").is_none());
        assert!(archive_path(Path::new("OPS"), "/absolute.xhtml").is_none());
    }
    #[test]
    fn juliet_has_96_pages_and_about_spans_3_and_4() {
        let spine = SpinePages::open(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test/juliet.epub"
        )))
        .unwrap();
        let start = spine.progress(2, 0.0).unwrap();
        assert_eq!((start.page, start.total, start.chapter_total), (3, 96, 2));
        assert_eq!(spine.progress(2, 0.499).unwrap().page, 3);
        assert_eq!(spine.progress(2, 0.5).unwrap().page, 4);
        assert_eq!(spine.progress(2, 1.0).unwrap().page, 4);
    }
}
