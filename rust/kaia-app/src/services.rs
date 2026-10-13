//! Long-lived services: the platform, settings, the library index, paths, the
//! theme and the fonts and icons. The app owns one [`Services`] for its lifetime.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kaia_display::PanelSpec;
use kaia_events::Settings;
use kaia_gfx::PixelFormat;
use kaia_library::{Library, LibraryError};
use kaia_platform::{Paths, Platform};
use kaia_ui::assets::{AssetError, Assets, SharedAssets};
use kaia_ui::theme::Scale;
use kaia_ui::Theme;

/// Environment variable that points at the `resources` folder. Desktop builds
/// default to the copy in this repository.
pub const RESOURCES_ENV: &str = "KAIA_RESOURCES";

/// The app's services.
pub struct Services {
    _lease: fs::File,
    pub startup_notices: Vec<String>,
    pub platform: Platform,
    pub sync: crate::sync::ProgressSync,
    pub clipboard: Option<String>,
    pub dictionary: Option<kaia_dict::Library>,
    /// Kaia's settings, at `<data>/settings.json`.
    pub settings: Settings,
    /// The library index, at `<data>/library.sqlite3`. The scan thread has its own connection.
    pub library: Library,
    /// A copy of `platform.paths`, with the data folder the app is using.
    pub paths: Paths,
    pub theme: Theme,
    pub assets: SharedAssets,
}

/// Why services could not start.
#[derive(Debug)]
pub enum ServicesError {
    Io { path: PathBuf, source: io::Error },
    Library(LibraryError),
    Assets(AssetError),
}

impl fmt::Display for ServicesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServicesError::Io { path, source } => write!(f, "cannot use {}: {source}", path.display()),
            ServicesError::Library(e) => write!(f, "library: {e}"),
            ServicesError::Assets(e) => write!(f, "assets: {e}"),
        }
    }
}

impl std::error::Error for ServicesError {}

impl Services {
    /// Opens the services for a panel. `data_dir` overrides the platform's data
    /// folder. The folder is created when it is missing.
    pub fn open(
        mut platform: Platform,
        panel: PanelSpec,
        data_dir: Option<PathBuf>,
    ) -> Result<Services, ServicesError> {
        if let Some(dir) = data_dir {
            platform.paths.data_dir = dir;
        }
        let data_dir = platform.paths.data_dir.clone();
        fs::create_dir_all(&data_dir).map_err(|source| ServicesError::Io {
            path: data_dir.clone(),
            source,
        })?;

        let lease_path = data_dir.join("kaia.lock");
        let lease = crate::durable::lock(&lease_path, false).map_err(|source| ServicesError::Io {
            path: lease_path,
            source: io::Error::other(format!(
                "Kaia is already using this data folder, or its lock cannot be opened: {source}"
            )),
        })?;
        let settings_path = data_dir.join("settings.json");
        let mut startup_notices = Vec::new();
        for path in [settings_path.clone(), data_dir.join("settings.json.old")] {
            if Settings::read_exact(&path).is_err() {
                let backup = crate::durable::preserve(&path).map_err(|source| ServicesError::Io {
                    path: path.clone(),
                    source,
                })?;
                startup_notices.push(format!(
                    "Settings were unreadable. Kept {} in the data folder.",
                    backup.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
        let mut settings = Settings::open(&settings_path);
        if let Some(warmth) = settings.read("display.warmth").as_i64() {
            let _ = platform.frontlight.set_warmth(warmth.clamp(0, 100) as u8);
        }
        if settings.read("sync.device_id").as_str().is_none() {
            let identity = format!(
                "{}-{}-{}",
                std::process::id(),
                crate::sync::now_unix(),
                data_dir.display()
            );
            settings.save(
                "sync.device_id",
                kaia_events::Value::from(kaia_sync::md5_hex(identity.as_bytes())),
            );
            settings
                .flush(&settings_path)
                .map_err(|source| ServicesError::Io {
                    path: settings_path.clone(),
                    source,
                })?;
        }
        let library = Library::open(&data_dir.join("library.sqlite3")).map_err(ServicesError::Library)?;

        let scale = Scale {
            width: panel.size.w,
            height: panel.size.h,
            dpi: Some(panel.dpi),
        };
        let theme = Theme::new(scale, panel.format == PixelFormat::Rgb24);

        let resources = resources_dir(&platform);
        let assets = Assets::load(&resources.join("fonts/noto"), &resources.join("icons/mdlight"))
            .map_err(ServicesError::Assets)?
            .shared();

        let paths = platform.paths.clone();
        let mut services = Services {
            _lease: lease,
            startup_notices,
            platform,
            clipboard: None,
            sync: crate::sync::ProgressSync::default(),
            dictionary: None,
            settings,
            library,
            paths,
            theme,
            assets,
        };
        if let Err(error) = crate::book_index::reconcile(&mut services) {
            services
                .startup_notices
                .push(format!("Cannot update library progress: {error}"));
        }
        Ok(services)
    }

    /// Where the settings file lives.
    pub fn settings_path(&self) -> PathBuf {
        self.paths.data_dir.join("settings.json")
    }

    /// Where the library database lives.
    pub fn library_path(&self) -> PathBuf {
        self.paths.data_dir.join("library.sqlite3")
    }
}

/// The `resources` folder: `KAIA_RESOURCES` when set, else the installed copy on
/// the Kobo, else the copy in this repository.
pub fn resources_dir(platform: &Platform) -> PathBuf {
    if let Some(dir) = std::env::var_os(RESOURCES_ENV) {
        return PathBuf::from(dir);
    }
    match platform.kind {
        kaia_platform::platform::Kind::Kobo(_) => platform.paths.install_dir.join("resources"),
        kaia_platform::platform::Kind::Desktop => {
            Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources")).to_path_buf()
        }
    }
}
