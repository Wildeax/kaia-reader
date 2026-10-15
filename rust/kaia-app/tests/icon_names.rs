//! Every icon name the app passes to an icon component resolves to a Phosphor SVG.
//! Unmapped names still use the mdlight fallback. The list below is the set of names
//! that `rust/kaia-app/src` and `rust/kaia-ui/src` pass to icons.

use std::fs;
use std::path::{Path, PathBuf};

use kaia_app::services::icon_dirs;
use kaia_gfx::{Color, Size};
use kaia_ui::assets::Assets;

const APP_ICON_NAMES: &[&str] = &[
    "align.justify",
    "align.left",
    "appbar.menu",
    "appbar.navigation",
    "appbar.pagefit",
    "appbar.rotation",
    "appbar.search",
    "appbar.settings",
    "appbar.tools",
    "appbar.typeset",
    "book.opened",
    "bookmark",
    "check",
    "chevron.down",
    "chevron.first",
    "chevron.last",
    "chevron.left",
    "chevron.right",
    "chevron.up",
    "close",
    "edit",
    "home",
    "light.moon",
    "light.sun",
    "more",
    "move",
    "notice-question",
    "settings.back",
    "settings.back.rtl",
    "settings.down",
    "star.empty",
    "star.full",
    "sun",
    "wifi",
    "wifi.open.100",
    "wifi.open.25",
    "wifi.open.50",
    "wifi.secure.100",
    "wifi.secure.25",
    "wifi.secure.50",
];

fn resources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources")
}

/// The text of every `.rs` file under the app and UI source folders.
fn app_source() -> String {
    fn collect(dir: &Path, out: &mut String) {
        for entry in fs::read_dir(dir).expect("source folder") {
            let path = entry.expect("source entry").path();
            if path.is_dir() {
                collect(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push_str(&fs::read_to_string(&path).expect("source file"));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut text = String::new();
    collect(&root.join("kaia-app/src"), &mut text);
    collect(&root.join("kaia-ui/src"), &mut text);
    text
}

#[test]
fn every_app_icon_name_resolves() {
    let resources = resources();
    let dirs = icon_dirs(&resources);
    let mut assets = Assets::load_icons(&resources.join("fonts/noto"), &dirs)
        .unwrap_or_else(|e| panic!("cannot load assets: {e}"));
    for name in APP_ICON_NAMES {
        let file = format!("{name}.svg");
        assert!(dirs[0].join(&file).is_file(), "{name} has no Phosphor icon");
        assets
            .icons
            .get(name, Size::new(64, 64), Color::BLACK)
            .unwrap_or_else(|e| panic!("{name} does not render: {e}"));
    }
}

#[test]
fn every_app_icon_name_is_used_by_the_app() {
    let source = app_source();
    for name in APP_ICON_NAMES {
        assert!(
            source.contains(&format!("\"{name}\"")),
            "{name} is in the list but no app or UI source passes it to an icon"
        );
    }
}

#[test]
fn every_kaia_icon_file_is_an_app_icon_name() {
    let folder = resources().join("kaia-icons/phosphor-light");
    for entry in fs::read_dir(&folder).expect("Kaia icon folder") {
        let file = entry
            .expect("entry")
            .file_name()
            .into_string()
            .expect("utf-8 name");
        let Some(name) = file.strip_suffix(".svg") else {
            continue;
        };
        assert!(
            APP_ICON_NAMES.contains(&name),
            "{file} is not used by the app; remove it or add the name to the list"
        );
    }
}

#[test]
fn an_unmapped_name_falls_back_to_mdlight() {
    let resources = resources();
    let dirs = icon_dirs(&resources);
    let kaia = dirs[0].join("wifi.open.0.svg");
    assert!(!kaia.exists(), "wifi.open.0 should not be in the Kaia folder");
    assert!(
        dirs[1].join("wifi.open.0.svg").is_file(),
        "the fallback has wifi.open.0"
    );
    let mut assets = Assets::load_icons(&resources.join("fonts/noto"), &dirs)
        .unwrap_or_else(|e| panic!("cannot load assets: {e}"));
    assets
        .icons
        .get("wifi.open.0", Size::new(64, 64), Color::BLACK)
        .unwrap_or_else(|e| panic!("the fallback does not render: {e}"));
}
