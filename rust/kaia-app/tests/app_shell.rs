//! Headless tests for the app loop and the router: scripted input, the refresh
//! order, and the stack of screens. Surface tests live in the files named after
//! each surface folder.

use kaia_app::{Route, ScriptInput};
use kaia_events::RefreshMode;

mod common;

use common::*;

#[test]
fn a_scripted_session_produces_the_expected_refreshes() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("scripted");

    // Find where the Library button sits on the empty Home, so the script can tap it.
    let mut probe = make_app(&dir, Vec::new());
    probe.start().unwrap();
    let p = centre_of(&probe, "nav-library");

    let home = centre_of(&probe, "nav-home");
    let script = format!("tap {} {}\ntap {} {}\n", p.x, p.y, home.x, home.y);
    drop(probe);
    let input = ScriptInput::from_script(&script, true).unwrap();
    let mut app = make_app_with(&dir, Vec::new(), input);
    app.start().unwrap();
    app.run().unwrap();

    // Home draws in full. The tabs open My books, then return to Home.
    assert_eq!(
        app.refresh_modes(),
        &[RefreshMode::Full, RefreshMode::Ui, RefreshMode::Ui]
    );
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert_eq!(app.stats().refreshes, 3);
}
