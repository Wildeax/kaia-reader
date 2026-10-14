#![cfg(feature = "cre")]

//! The Kaia reading fonts reach CREngine on the desktop tree and in an
//! installed package, and CREngine lays out pages with them.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kaia_doc::cre::{CreDocument, CreOptions};
use kaia_doc::types::Layout;
use kaia_doc::Document;
use kaia_gfx::PixelFormat;

/// The eight files of the two reading fonts, relative to `resources/kaia-fonts`.
const FONT_FILES: [&str; 8] = [
    "atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf",
    "atkinson-hyperlegible/AtkinsonHyperlegible-Bold.ttf",
    "atkinson-hyperlegible/AtkinsonHyperlegible-Italic.ttf",
    "atkinson-hyperlegible/AtkinsonHyperlegible-BoldItalic.ttf",
    "opendyslexic/OpenDyslexic-Regular.otf",
    "opendyslexic/OpenDyslexic-Bold.otf",
    "opendyslexic/OpenDyslexic-Italic.otf",
    "opendyslexic/OpenDyslexic-Bold-Italic.otf",
];

/// The family names CREngine reads from these files (FreeType `family_name`).
const FACES: [&str; 2] = ["Atkinson Hyperlegible", "OpenDyslexic"];

fn kaia_fonts() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/kaia-fonts")
}

fn file_names(paths: &[PathBuf]) -> BTreeSet<String> {
    paths
        .iter()
        .map(|p| {
            let parent = p.parent().and_then(Path::file_name).unwrap().to_string_lossy();
            format!("{parent}/{}", p.file_name().unwrap().to_string_lossy())
        })
        .collect()
}

#[test]
fn desktop_tree_lists_the_reading_fonts() {
    let listed = file_names(&CreOptions::default().font_files());
    for rel in FONT_FILES {
        assert!(listed.contains(rel), "{rel} is not in the font list");
    }
}

#[test]
fn installed_fonts_folder_lists_the_reading_fonts() {
    // The Kobo package copies both folders into fonts/ beside the binary.
    let install = common::out_dir().join(format!("kaia-reading-fonts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&install);
    for rel in FONT_FILES
        .iter()
        .chain(["atkinson-hyperlegible/OFL.txt", "opendyslexic/OFL.txt"].iter())
    {
        let target = install.join("fonts").join(rel);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(kaia_fonts().join(rel), target).unwrap();
    }
    let options = CreOptions::from_install_dir(&install);
    assert_eq!(options.font_dirs, vec![install.join("fonts")]);
    let listed = file_names(&options.font_files());
    for rel in FONT_FILES {
        assert!(listed.contains(rel), "{rel} is not in the installed font list");
    }
    std::fs::remove_dir_all(install).unwrap();
}

/// The hash of page 10 when the layout names `face`.
fn page_hash(face: &str) -> u64 {
    let mut doc = CreDocument::open(&common::fixture("juliet.epub")).unwrap();
    let mut layout = Layout::new(600.0, 800.0, 28.0);
    layout.font_family = Some(face.to_string());
    doc.set_layout(&layout).unwrap();
    let pixels = doc.render(10, 1.0, PixelFormat::Gray8).unwrap();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(pixels.data(), &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

#[test]
fn each_reading_face_changes_the_rendered_page() {
    // A name that no font has. CREngine falls back for it, so its page is the
    // reference for "the face was not found".
    let unknown = page_hash("Kaia No Such Face");
    let serif = page_hash("Noto Serif");
    let mut hashes = Vec::new();
    for face in FACES {
        let hash = page_hash(face);
        assert_ne!(hash, unknown, "{face} renders like an unknown face");
        assert_ne!(hash, serif, "{face} renders like Noto Serif");
        hashes.push(hash);
    }
    assert_ne!(hashes[0], hashes[1], "the two reading faces render the same page");
}
