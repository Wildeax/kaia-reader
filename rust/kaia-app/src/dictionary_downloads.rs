//! One user-requested dictionary download, independent of the settings page lifetime.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use kaia_dict::download::{self, CatalogEntry, Permission};
use kaia_events::Settings;
use kaia_i18n::tr;

use crate::surfaces::settings::keys;

pub const ALLOW_KEY: &str = "dictionary.allow_downloads";

pub fn allowed(settings: &Settings) -> bool {
    keys::bool_or(settings, ALLOW_KEY, false)
}

#[derive(Default)]
pub struct Downloads {
    worker: Option<JoinHandle<Result<PathBuf, String>>>,
    cancel: Arc<AtomicBool>,
}

impl Downloads {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }

    pub fn start(&mut self, entry: CatalogEntry, root: PathBuf, settings: &Settings) -> Result<(), String> {
        if !allowed(settings) {
            return Err(tr("Dictionary downloads are off."));
        }
        if keys::airplane_mode(settings) {
            return Err(tr("Airplane mode is on. Turn it off to use Wi-Fi."));
        }
        if self.busy() {
            return Err(tr("A dictionary download is already running."));
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        self.worker = Some(std::thread::spawn(move || {
            download::install(
                &entry,
                &root,
                Permission {
                    allowed: true,
                    airplane_mode: false,
                    cancel: &cancel,
                },
            )
            .map_err(|error| error.to_string())
        }));
        Ok(())
    }

    pub fn poll(&mut self, settings: &Settings) -> Option<Result<PathBuf, String>> {
        if !allowed(settings) || keys::airplane_mode(settings) {
            self.cancel.store(true, Ordering::Relaxed);
        }
        if !self.worker.as_ref().is_some_and(|worker| worker.is_finished()) {
            return None;
        }
        Some(
            self.worker
                .take()
                .unwrap()
                .join()
                .unwrap_or_else(|_| Err(tr("Dictionary download failed."))),
        )
    }
}

impl Drop for Downloads {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
