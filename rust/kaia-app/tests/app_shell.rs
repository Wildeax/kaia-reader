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
    let p = centre_of(&probe, "library");
    drop(probe);

    let script = format!("tap {} {}\nkey Back\n", p.x, p.y);
    let input = ScriptInput::from_script(&script, true).unwrap();
    let mut app = make_app_with(&dir, Vec::new(), input);
    app.start().unwrap();
    app.run().unwrap();

    // Start: Home is drawn in full. Tap: the pressed button (Fast), then the Library
    // root (Ui). Back: Home again (Ui). The loop then quits with the script.
    assert_eq!(
        app.refresh_modes(),
        &[
            RefreshMode::Full,
            RefreshMode::Fast,
            RefreshMode::Ui,
            RefreshMode::Ui
        ]
    );
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert_eq!(app.stats().refreshes, 4);
}
