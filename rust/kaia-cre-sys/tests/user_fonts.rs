//! Fonts in the user folder reach CREngine. This file starts the engine with
//! the user folder alone, so every family listed comes from that folder. One
//! test function runs the steps in order, because the engine is shared by the
//! whole process.

use std::fs;
use std::path::{Path, PathBuf};

use kaia_cre_sys::{init_with_options, registered_fonts, RuntimeOptions, DATA_DIR};

fn kaia_fonts() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/kaia-fonts")
}

/// The families whose regular face sits under `user`, with their file paths.
fn families_under(user: &Path) -> Vec<(String, PathBuf)> {
    registered_fonts()
        .expect("registered fonts")
        .into_iter()
        .filter(|f| f.file.starts_with(user))
        .map(|f| (f.family, f.file))
        .collect()
}

#[test]
fn user_folder_fonts_register_and_later_files_appear_on_rescan() {
    let root = std::env::temp_dir().join(format!("kaia-cre-user-fonts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let user = root.join("fonts");
    let options = RuntimeOptions {
        data_dir: DATA_DIR.into(),
        font_dirs: vec![user.clone()],
    };

    // A missing folder is fine: the engine has no fonts, so it cannot start yet.
    assert!(
        init_with_options(&options).is_err(),
        "no fonts must not start the engine"
    );

    // A real font, plus files that are not fonts. Only the real one is accepted.
    fs::create_dir_all(user.join("atkinson")).unwrap();
    fs::copy(
        kaia_fonts().join("atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf"),
        user.join("atkinson/AtkinsonHyperlegible-Regular.ttf"),
    )
    .unwrap();
    fs::write(user.join("notes.txt"), "not a font").unwrap();
    fs::write(user.join("broken.ttf"), b"not a font either").unwrap();
    let accepted = init_with_options(&options).expect("a folder with junk still starts the engine");
    assert_eq!(accepted, 1, "only the real font is accepted");
    let found = families_under(&user);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "Atkinson Hyperlegible");
    assert!(found[0].1.ends_with("AtkinsonHyperlegible-Regular.ttf"));

    // A font added while the engine runs appears after a second init with the same folder.
    fs::create_dir_all(user.join("dyslexic")).unwrap();
    fs::copy(
        kaia_fonts().join("opendyslexic/OpenDyslexic-Regular.otf"),
        user.join("dyslexic/OpenDyslexic-Regular.otf"),
    )
    .unwrap();
    init_with_options(&options).expect("rescan");
    let names: Vec<String> = families_under(&user).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["Atkinson Hyperlegible", "OpenDyslexic"]);

    let _ = fs::remove_dir_all(&root);
}
