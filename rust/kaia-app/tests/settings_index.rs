//! Settings index layout and navigation through its controls.

mod common;

use common::*;
use kaia_app::surfaces::settings::commands::SECTIONS;
use kaia_app::Route;

#[test]
fn every_category_fits_and_close_uses_the_existing_back_route() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-index");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    assert!(app.find_rect("nav-bar").is_none());
    for (index, section) in SECTIONS.iter().enumerate() {
        let row = app.find_rect(&format!("settings-{section}")).unwrap();
        assert_eq!(row.x, 100);
        assert_eq!(row.y, 280 + 107 * index as i32);
        assert_eq!(row.w, 1056);
        assert_eq!(row.h, 107);
    }
    tap_key(&mut app, "settings-advanced");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "advanced".into()
        })
    );
    app.command("back", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-close");
    assert_eq!(app.top_route(), Some(&Route::Home));
}
