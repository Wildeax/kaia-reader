//! Long-lived services: the platform, settings, the library index, paths, the
//! theme and the fonts and icons. The app owns one [`Services`] for its lifetime.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kaia_display::PanelSpec;
use kaia_events::Settings;
use kaia_gfx::PixelFormat;
use kaia_i18n::{format as tr_format, tr};
use kaia_library::{Library, LibraryError};
use kaia_platform::{Paths, Platform};
use kaia_ui::assets::{AssetError, Assets, SharedAssets};
use kaia_ui::theme::{Scale, LARGE_PRINT_SCALE};
use kaia_ui::Theme;

use crate::surfaces::settings::keys;

/// Environment variable that points at the `resources` folder. Desktop builds
/// default to the copy in this repository.
pub const RESOURCES_ENV: &str = "KAIA_RESOURCES";

/// The app's services.
pub struct Services {
    _lease: fs::File,
    pub startup_notices: Vec<String>,
    #[cfg(feature = "lua")]
    pub plugins: crate::plugins::Plugins,
    pub platform: Platform,
    pub sync: crate::sync::ProgressSync,
    pub clipboard: Option<String>,
    pub dictionary: Option<kaia_dict::Library>,
    pub dictionary_downloads: crate::dictionary_downloads::Downloads,
    /// Kaia's settings, at `<data>/settings.json`.
    pub settings: Settings,
    /// The library index, at `<data>/library.sqlite3`. The scan thread has its own connection.
    pub library: Library,
    /// Books folders supplied when the app starts.
    pub library_roots: Vec<PathBuf>,
    /// A copy of `platform.paths`, with the data folder the app is using.
    pub paths: Paths,
    pub theme: Theme,
    pub assets: SharedAssets,
    /// The language the screens show, as a tag such as `fr`. See [`crate::i18n`].
    pub language: String,
    /// Goes up by one on each language change. The app builds its screens again when it sees a change.
    pub language_epoch: u64,
    /// The folder with the `l10n` catalogs. See [`crate::i18n::catalog_root`].
    pub catalog_root: PathBuf,
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
        let data_dir = std::path::absolute(&data_dir).map_err(|source| ServicesError::Io {
            path: data_dir.clone(),
            source,
        })?;
        platform.paths.data_dir = data_dir.clone();
        // CREngine registers the fonts in this folder, beside the built-in ones.
        kaia_doc::cre::set_user_font_dir(Some(data_dir.join("fonts")));

        let lease_path = data_dir.join("kaia.lock");
        let lease = crate::durable::lock(&lease_path, false).map_err(|source| ServicesError::Io {
            path: lease_path,
            source: io::Error::other(format!(
                "Kaia is already using this data folder, or its lock cannot be opened: {source}"
            )),
        })?;
        // An erase that stopped after its first step is finished before the folder is read.
        // The lease above keeps a second run from erasing at the same time.
        let mut cleanup_notices = Vec::new();
        if let Err(error) = kaia_platform::reset::finish_leftover_erases(&data_dir) {
            cleanup_notices.push(tr_format(
                &tr("Cannot finish an erase that stopped: %1"),
                &[&error.to_string()],
            ));
        }
        let settings_path = data_dir.join("settings.json");
        let mut kept = Vec::new();
        for path in [settings_path.clone(), data_dir.join("settings.json.old")] {
            if Settings::read_exact(&path).is_err() {
                let backup = crate::durable::preserve(&path).map_err(|source| ServicesError::Io {
                    path: path.clone(),
                    source,
                })?;
                kept.push(
                    backup
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
        let mut settings = Settings::open(&settings_path);
        // Kaia's clock zone. It is set here, before any local time is read and before the
        // threads below start. See `kaia_platform::timezone::apply`.
        kaia_platform::timezone::apply(keys::time_zone(&settings));
        // The language comes before the notices, so the notices show in it.
        let catalog_root = crate::i18n::catalog_root(&platform);
        let on_offer = crate::i18n::available(&catalog_root);
        let language = crate::i18n::resolve(settings.read(crate::i18n::SETTING).as_str(), &on_offer);
        crate::i18n::activate(&catalog_root, &language);
        let mut startup_notices: Vec<String> = cleanup_notices;
        startup_notices.extend(kept.iter().map(|name| {
            tr_format(
                &tr("Settings were unreadable. Kept %1 in the data folder."),
                &[name.as_str()],
            )
        }));
        if let Some(intensity) = settings.read("display.frontlight").as_i64() {
            let _ = platform.frontlight.set_intensity(intensity.clamp(0, 100) as u8);
        }
        if let Some(warmth) = settings.read("display.warmth").as_i64() {
            let _ = platform.frontlight.set_warmth(warmth.clamp(0, 100) as u8);
        }
        // Older versions stored the sync password as typed. Keep only its user key, once.
        crate::sync::migrate_password_file(&mut settings, &settings_path).map_err(|source| {
            ServicesError::Io {
                path: settings_path.clone(),
                source,
            }
        })?;
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

        // Airplane mode keeps Wi-Fi off across restarts. Turn it off if the radio came up on its own.
        if keys::airplane_mode(&settings) && platform.network.is_wifi_on() {
            if let Err(error) = platform.network.disable_wifi() {
                startup_notices.push(tr_format(
                    &tr("Cannot turn off Wi-Fi for airplane mode: %1"),
                    &[&error.to_string()],
                ));
            }
        }

        let scale = Scale {
            width: panel.size.w,
            height: panel.size.h,
            dpi: Some(panel.dpi),
        };
        let theme = Theme::new(scale, panel.format == PixelFormat::Rgb24)
            .with_text_scale(large_print_factor(&settings));

        let resources = resources_dir(&platform);
        let assets = Assets::load(&resources.join("fonts/noto"), &resources.join("icons/mdlight"))
            .map_err(ServicesError::Assets)?
            .shared();

        let paths = platform.paths.clone();
        // Sync sends the device model, as KOReader does, so other devices can tell this one apart.
        let sync = crate::sync::ProgressSync::for_device(&platform.kind, &platform.device);
        let mut services = Services {
            _lease: lease,
            #[cfg(feature = "lua")]
            plugins: crate::plugins::Plugins::new(&paths),
            startup_notices,
            platform,
            clipboard: None,
            sync,
            dictionary: None,
            dictionary_downloads: crate::dictionary_downloads::Downloads::default(),
            settings,
            library,
            library_roots: Vec::new(),
            paths,
            theme,
            assets,
            language,
            language_epoch: 0,
            catalog_root,
        };
        if let Err(error) = crate::book_index::reconcile(&mut services) {
            services.startup_notices.push(tr_format(
                &tr("Cannot update library progress: %1"),
                &[error.as_str()],
            ));
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

    /// Applies the large print setting to the theme. The screens built before
    /// this call keep their old sizes until they are built again.
    pub fn apply_large_print(&mut self) {
        self.theme = self.theme.with_text_scale(large_print_factor(&self.settings));
    }

    /// Shows the screens in `tag` from now on. `tag` is one of the languages of
    /// [`crate::i18n::available`]. The app rebuilds its screens when the epoch changes.
    pub fn set_language(&mut self, tag: &str) {
        crate::i18n::activate(&self.catalog_root, tag);
        self.language = tag.to_string();
        self.language_epoch += 1;
    }
}

/// The text scale that large print mode asks for: [`LARGE_PRINT_SCALE`] when
/// `display.large_print` is on, 1.0 otherwise.
pub fn large_print_factor(settings: &Settings) -> f32 {
    if keys::bool_or(settings, keys::DISPLAY_LARGE_PRINT, false) {
        LARGE_PRINT_SCALE
    } else {
        1.0
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
