#![cfg(feature = "cre")]

//! The user font folder as Kaia sets it at start. A copy of a reading face in
//! the folder lists once and renders the page. A file that is not a font
//! is ignored.

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use kaia_doc::cre::{rescan_fonts, set_user_font_dir, user_font_faces, CreDocument};
use kaia_doc::types::Layout;
use kaia_doc::Document;
use kaia_gfx::PixelFormat;

fn kaia_fonts() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/kaia-fonts")
}

/// The hash of page 10 with `face` set, or with the book's own font when `face` is None.
fn page_hash(face: Option<&str>) -> u64 {
    let mut doc = CreDocument::open(&common::fixture("juliet.epub")).unwrap();
    let mut layout = Layout::new(600.0, 800.0, 28.0);
    layout.font_family = face.map(str::to_string);
    doc.set_layout(&layout).unwrap();
    let pixels = doc.render(10, 1.0, PixelFormat::Gray8).unwrap();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(pixels.data(), &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

#[test]
fn user_folder_copy_of_a_reading_face_lists_once_and_renders() {
    let data = common::out_dir().join(format!("kaia-user-fonts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&data);
    let user = data.join("fonts");
    fs::create_dir_all(user.join("reading")).unwrap();
    fs::copy(
        kaia_fonts().join("atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf"),
        user.join("reading/AtkinsonHyperlegible-Regular.ttf"),
    )
    .unwrap();
    fs::write(user.join("notes.txt"), "not a font").unwrap();
    fs::write(user.join("broken.ttf"), b"not a font either").unwrap();

    set_user_font_dir(Some(user.clone()));
    rescan_fonts().expect("rescan with a user folder that holds junk");

    let faces = user_font_faces();
    let unique: BTreeSet<&String> = faces.iter().collect();
    assert_eq!(unique.len(), faces.len(), "a family is listed twice: {faces:?}");

    let default = page_hash(None);
    let atkinson = page_hash(Some("Atkinson Hyperlegible"));
    assert_ne!(
        atkinson, default,
        "the reading face renders like the default page"
    );

    set_user_font_dir(None);
    let _ = fs::remove_dir_all(&data);
}
