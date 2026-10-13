#![cfg(feature = "cre")]

mod common;

use kaia_doc::cre::{CreDocument, CreOptions};
use kaia_doc::Document;

#[test]
fn runtime_resources_override_build_paths_and_failed_open_can_retry() {
    let install = common::out_dir().join(format!("runtime-resources-{}", std::process::id()));
    std::fs::create_dir_all(install.join("fonts/noto")).unwrap();
    std::fs::create_dir_all(install.join("data/hyph")).unwrap();
    let options = CreOptions::from_install_dir(&install);
    assert_eq!(options.data_dir, install.join("data"));
    assert_eq!(options.font_dirs, vec![install.join("fonts")]);
    let book = common::fixture("juliet.epub");
    let error = CreDocument::open_with_options(&book, &options).err().unwrap();
    assert!(error.to_string().contains("no font files supplied"));
    for name in ["NotoSerif-Regular.ttf", "NotoSerif-Bold.ttf"] {
        std::fs::copy(
            std::path::Path::new(kaia_cre_sys::FONTS_DIR)
                .join("noto")
                .join(name),
            install.join("fonts/noto").join(name),
        )
        .unwrap();
    }
    for name in ["epub.css", "fb2.css", "hyph/English_US.pattern"] {
        std::fs::copy(
            std::path::Path::new(kaia_cre_sys::DATA_DIR).join(name),
            install.join("data").join(name),
        )
        .unwrap();
    }
    let doc = CreDocument::open_with_options(&book, &options).unwrap();
    assert!(doc.page_count() > 1);
    assert!(doc
        .render(10, 1.0, kaia_gfx::PixelFormat::Gray8)
        .unwrap()
        .data()
        .iter()
        .any(|p| *p < 128));
    drop(doc);
    std::fs::remove_dir_all(install).unwrap();
}
