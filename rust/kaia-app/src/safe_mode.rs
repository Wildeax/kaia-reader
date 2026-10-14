//! Safe mode at launch, and the recovery state that goes with it.
//!
//! The launcher, `platform/kobo/kaia/kaia-rust.sh`, adds one to the failed start
//! count in `<data>/kaia-startup-attempts` before each launch. It passes
//! `--safe-mode` when the count is above [`FAILURE_LIMIT`]. It clears the count
//! after a clean exit. This module reads the same files. It clears the count
//! after a healthy start, in safe mode too, as the Lua app does: a start that keeps
//! failing stays in safe mode until one start runs healthy. The `safemode` marker
//! belongs to the user. The app reads it and never removes it.

use std::io;
use std::path::Path;
use std::time::Duration;

use kaia_i18n::tr;

use crate::services::Services;

/// The file the user creates over USB to start in safe mode. The Lua app uses the same name.
pub const MARKER: &str = "safemode";

/// The failed start count, in the Rust data folder.
pub const COUNTER: &str = "kaia-startup-attempts";

/// A start that finds the count above this runs in safe mode. Three failed starts
/// in a row mean the fourth start is safe mode. The Lua app and the launcher use the same limit.
pub const FAILURE_LIMIT: u64 = 3;

/// How long the first screen must stay up before the start counts as healthy.
pub const HEALTHY_AFTER: Duration = Duration::from_secs(10);

/// Why a start runs in safe mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The `safemode` file is in the data folder.
    Marker,
    /// The failed start count is above [`FAILURE_LIMIT`].
    Failures,
    /// `--safe-mode` was passed without a marker or a failed start count, such as a manual run.
    Requested,
}

/// Applies safe mode for this start. Call it once, after the services open and before the app starts.
/// `requested` is the `--safe-mode` flag. Returns the reason when the start is in safe mode.
pub fn begin(services: &mut Services, requested: bool) -> Option<Reason> {
    let data = services.paths.data_dir.clone();
    let marker = data.join(MARKER).is_file();
    if !requested && !marker {
        return None;
    }
    let reason = if marker {
        Reason::Marker
    } else if failed_starts(&data) > FAILURE_LIMIT {
        Reason::Failures
    } else {
        Reason::Requested
    };
    set_plugins_off(services);
    eprintln!("kaia: safe mode ({reason:?})");
    services.startup_notices.push(notice(reason));
    Some(reason)
}

/// The failed start count. A missing or unreadable file counts as zero.
pub fn failed_starts(data: &Path) -> u64 {
    std::fs::read_to_string(data.join(COUNTER))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Clears the failed start count. The app calls it once the first screen has stayed
/// up for [`HEALTHY_AFTER`]. The marker stays. The user removes it to leave safe mode.
pub fn mark_healthy(data: &Path) {
    remove_counter(data);
}

/// User plugins stay off for this start. Bundled plugins still load.
#[cfg(feature = "lua")]
fn set_plugins_off(services: &mut Services) {
    services.plugins.startup_safe_mode = true;
}

#[cfg(not(feature = "lua"))]
fn set_plugins_off(_services: &mut Services) {}

fn remove_counter(data: &Path) {
    if let Err(error) = std::fs::remove_file(data.join(COUNTER)) {
        if error.kind() != io::ErrorKind::NotFound {
            eprintln!("kaia: cannot clear the failed start count: {error}");
        }
    }
}

/// The notice that says why the start is in safe mode and how to leave it.
pub fn notice(reason: Reason) -> String {
    match reason {
        Reason::Marker => tr(
            "Kaia started in safe mode because the safemode file is in the .kaia folder. Third-party plugins are off. To leave safe mode, connect the Kobo over USB, delete the safemode file from the .kaia folder, then restart Kaia.",
        ),
        Reason::Failures => tr(
            "Kaia did not start correctly several times in a row, so it started in safe mode. Third-party plugins are off. Once this start has run for a few seconds, the next start is normal.",
        ),
        Reason::Requested => tr("Kaia started in safe mode. Third-party plugins are off."),
    }
}
