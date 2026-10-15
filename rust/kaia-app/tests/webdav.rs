//! Settings and reader events queue WebDAV exports without using the network.
mod common;
use common::*;
use kaia_app::{
    surfaces::settings::{entries::Entry, sections},
    webdav,
};
use kaia_events::Value;
use std::fs;

#[test]
fn webdav_settings_store_password_separately_and_book_close_queues_saved_notes() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("webdav-settings-close");
    let library = library_with_books(&dir);
    let mut app = make_app(&dir, vec![library]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-network");
    assert!(!app.services().settings.is_true(webdav::ENABLED));
    let entries = sections::entries("network", app.services(), 0);
    assert!(entries
        .iter()
        .any(|e| matches!(e, Entry::Toggle { key, on: false, .. } if *key == webdav::ENABLED)));
    for (key, text) in [
        (webdav::SERVER, "https://example.invalid/dav"),
        (webdav::USERNAME, "reader"),
        (webdav::PASSWORD, "a private password"),
        (webdav::FOLDER, "notes"),
    ] {
        app.command(&format!("settings:set:{key}"), &[Value::from(text)]);
    }
    app.command("settings:set:webdav.enabled", &[Value::Bool(true)]);
    app.services_mut()
        .settings
        .save("network.airplane_mode", Value::Bool(true));
    let data = app.services().paths.data_dir.clone();
    assert_eq!(
        fs::read_to_string(data.join("webdav-password")).unwrap(),
        "a private password"
    );
    assert!(!app.services().settings.has(webdav::PASSWORD));
    app.command("back", &[]);
    assert!(!fs::read_to_string(data.join("settings.json"))
        .unwrap()
        .contains("a private password"));
    tap_key(&mut app, "settings-close");
    tap_key(&mut app, "nav-library");
    open_from_library(&mut app, "juliet.epub");
    wait_reader(&mut app);
    app.command("reader:bookmark", &[]);
    wait_reader(&mut app);
    app.command("nav:settings", &[]);
    wait_reader(&mut app);
    assert_eq!(app.top_route(), Some(&kaia_app::Route::Settings));
    let queue: serde_json::Value =
        serde_json::from_slice(&fs::read(data.join("webdav-queue.json")).unwrap()).unwrap();
    assert_eq!(queue["queue"].as_array().unwrap().len(), 0);
    // Settings covers the book. Export waits until the reader actually closes.
    tap_key(&mut app, "settings-close");
    app.command("back", &[]);
    wait_reader(&mut app);
    app.command("nav:settings", &[]);
    wait_reader(&mut app);
    let queue: serde_json::Value =
        serde_json::from_slice(&fs::read(data.join("webdav-queue.json")).unwrap()).unwrap();
    assert_eq!(queue["queue"].as_array().unwrap().len(), 1);
    let exported: serde_json::Value =
        serde_json::from_str(queue["queue"][0]["json"].as_str().unwrap()).unwrap();
    assert_eq!(exported["books"][0]["annotations"].as_array().unwrap().len(), 1);
    assert_eq!(queue["queue"][0]["attempts"], 0);
    tap_key(&mut app, "settings-network");
    app.command("settings:webdav:export", &[]);
    let queue: serde_json::Value =
        serde_json::from_slice(&fs::read(data.join("webdav-queue.json")).unwrap()).unwrap();
    assert_eq!(queue["queue"].as_array().unwrap().len(), 1);
}

#[test]
fn webdav_network_controls_fit_the_paged_settings_screen() {
    use kaia_gfx::golden::assert_matches_golden;
    use kaia_ui::composite::PagedList;
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("webdav-settings-golden");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-network");
    let controls = [
        "set-webdav.enabled",
        "field-webdav.server",
        "field-webdav.username",
        "field-webdav.password",
        "field-webdav.folder",
        "settings-webdav-test",
        "settings-webdav-export",
    ];
    let mut seen = std::collections::BTreeSet::new();
    let mut count = 0;
    app.update_top::<PagedList<Entry>, _>("settings-entries", |list, _| count = list.page_count());
    for page in 0..count {
        app.update_top::<PagedList<Entry>, _>("settings-entries", |list, cx| list.set_page(page, cx));
        app.process(Vec::new()).unwrap();
        let visible: Vec<_> = controls
            .iter()
            .filter(|key| app.find_rect(key).is_some())
            .collect();
        if !visible.is_empty() {
            assert_matches_golden(app.frame(), &golden(&format!("settings_webdav_{page}.png")), 14);
            seen.extend(visible.into_iter().copied());
        }
    }
    assert_eq!(seen.len(), controls.len());
}
