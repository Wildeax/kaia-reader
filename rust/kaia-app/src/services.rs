//! Long-lived services: the platform, settings, the library index, paths, the
//! theme and the fonts and icons. The app owns one [`Services`] for its lifetime.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kaia_display::PanelSpec;
use kaia_events::{Settings, Value};
use kaia_gfx::PixelFormat;
use kaia_i18n::{format as tr_format, tr};
use kaia_library::{Library, LibraryError};
use kaia_platform::frontlight::Frontlight;
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
    pub webdav: crate::webdav::WebDav,
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
        start_light(platform.frontlight.as_mut(), &mut settings);
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
            webdav: crate::webdav::WebDav::new(data_dir.clone()),
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

/// Sets the frontlight at startup. The hardware wins: the light keeps the level
/// and warmth that Nickel or the last boot left, and nothing is written to the LED
/// or the mixer. The values the hardware reports replace the stored ones, so the
/// sliders and the settings show what the light does now. The stored values are
/// applied only when the hardware cannot be read, as on the desktop.
pub fn start_light(light: &mut dyn Frontlight, settings: &mut Settings) {
    match light.hardware_levels() {
        Some(hw) => {
            settings.save("display.frontlight", Value::Number(f64::from(hw.intensity)));
            if let Some(warmth) = hw.warmth {
                settings.save("display.warmth", Value::Number(f64::from(warmth)));
            }
        }
        None => {
            if let Some(intensity) = settings.read("display.frontlight").as_i64() {
                let _ = light.set_intensity(intensity.clamp(0, 100) as u8);
            }
            if let Some(warmth) = settings.read("display.warmth").as_i64() {
                let _ = light.set_warmth(warmth.clamp(0, 100) as u8);
            }
        }
    }
}

#[cfg(test)]
mod startup_light_tests {
    use super::*;
    use kaia_platform::frontlight::{desktop_frontlight, FrontlightConfig, SysfsFrontlight};
    use kaia_platform::sysfs::{Root, SystemIo};

    const BRIGHTNESS: &str = "/sys/class/backlight/mxc_msp430.0/brightness";
    const ACTUAL: &str = "/sys/class/backlight/mxc_msp430.0/actual_brightness";
    const MIXER: &str = "/sys/class/backlight/lm3630a_led/color";

    /// A Libra Colour light over a fake device tree on disk, opened as at startup.
    /// `actual` is left out when `None`.
    fn kobo_light(
        brightness: &str,
        actual: Option<&str>,
        mixer: &str,
    ) -> (tempfile::TempDir, Box<dyn Frontlight>) {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        for (path, text) in [
            (BRIGHTNESS, Some(brightness)),
            (ACTUAL, actual),
            (MIXER, Some(mixer)),
        ] {
            if let Some(text) = text {
                let full = dir.path().join(path.trim_start_matches('/'));
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, text).unwrap();
            }
        }
        let light = SysfsFrontlight::open(
            FrontlightConfig::monza(),
            SystemIo::new(root.clone()),
            &root,
            None,
        );
        (dir, Box::new(light))
    }

    fn file(dir: &tempfile::TempDir, device_path: &str) -> String {
        std::fs::read_to_string(dir.path().join(device_path.trim_start_matches('/'))).unwrap()
    }

    #[test]
    fn a_stored_zero_does_not_turn_off_a_light_that_is_on() {
        // Nickel left the light at 26. The stored value from the last Kaia session is 0.
        let (dir, mut light) = kobo_light("0\n", Some("26\n"), "1\n");
        let mut settings = Settings::new();
        settings.save("display.frontlight", Value::Number(0.0));
        start_light(light.as_mut(), &mut settings);
        assert!(light.is_on());
        assert_eq!(light.intensity(), 26);
        assert_eq!(settings.read("display.frontlight").as_i64(), Some(26));
        assert_eq!(file(&dir, BRIGHTNESS), "0\n", "startup must not write the LED");
    }

    #[test]
    fn without_a_stored_warmth_the_slider_shows_the_mixer_warmth() {
        // Color 1 is warmth 90 on the inverted Libra Colour scale.
        let (dir, mut light) = kobo_light("0\n", Some("26\n"), "1\n");
        let mut settings = Settings::new();
        start_light(light.as_mut(), &mut settings);
        assert_eq!(light.warmth(), 90);
        assert_eq!(settings.read("display.warmth").as_i64(), Some(90));
        assert_eq!(file(&dir, MIXER), "1\n", "startup must not write the mixer");
    }

    #[test]
    fn startup_writes_nothing_to_the_led_or_the_mixer_even_with_other_stored_values() {
        let (dir, mut light) = kobo_light("0\n", Some("26\n"), "1\n");
        let mut settings = Settings::new();
        settings.save("display.frontlight", Value::Number(40.0));
        settings.save("display.warmth", Value::Number(70.0));
        start_light(light.as_mut(), &mut settings);
        assert_eq!(light.intensity(), 26);
        assert_eq!(light.warmth(), 90);
        assert_eq!(file(&dir, BRIGHTNESS), "0\n");
        assert_eq!(file(&dir, ACTUAL), "26\n");
        assert_eq!(file(&dir, MIXER), "1\n");
    }

    #[test]
    fn stored_values_apply_when_the_hardware_cannot_be_read() {
        // The desktop light reports no hardware levels, so the stored values are applied.
        let mut light: Box<dyn Frontlight> = Box::new(desktop_frontlight());
        let mut settings = Settings::new();
        settings.save("display.frontlight", Value::Number(40.0));
        settings.save("display.warmth", Value::Number(55.0));
        start_light(light.as_mut(), &mut settings);
        assert_eq!(light.intensity(), 40);
        assert_eq!(light.warmth(), 55);
    }
}
