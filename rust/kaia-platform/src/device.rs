//! Device identity: model, codename, firmware and screen.
//!
//! KOReader's sources (`frontend/device/kobo/device.lua`):
//! - codename: `getCodeName` at 1158-1197. The order is `PRODUCT`, then
//!   `/bin/kobo_config.sh` (1163), `/usr/bin/hwdetect.sh` (1171), then the
//!   `hwdetect` binary (1179-1197).
//! - model table: the `codename == ...` chain at 1813. Only the
//!   Libra Colour (`monza`, `monzaKobo`, `monzaTolino`) has a profile here.
//! - firmware: `Kobo:getFirmwareVersion` at 1200-1217. Field 3 of
//!   `/mnt/onboard/.kobo/version`, comma-separated.
//! - product id: `getProductId` at 1219-1234. `MODEL_NUMBER` first, then
//!   the last three characters of the version line, or `000`.
//! - model id and name: `model = "Kobo_monza"` at 534. The name is the
//!   marketing name, not from KOReader.
//! - DPI: `display_dpi = 300` at 541. Colour screen: `hasColorScreen = yes`
//!   at 555.
//!
//! The serial number is never read.

use crate::sysfs::{Root, Runner};

/// Firmware and product version file (`device.lua:1201`, `1224`).
pub const VERSION_FILE: &str = "/mnt/onboard/.kobo/version";

/// Kobo models with a profile in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Model {
    /// Kobo Libra Colour, KOReader's `KoboMonza`.
    LibraColour,
}

impl Model {
    /// The model for a codename (`device.lua:1813`), or `None` for other devices.
    pub fn from_codename(codename: &str) -> Option<Model> {
        match codename {
            "monza" | "monzaKobo" | "monzaTolino" => Some(Model::LibraColour),
            _ => None,
        }
    }
}

/// What the app needs to know about the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// KOReader's model string, for example `Kobo_monza`.
    pub model_id: String,
    /// Display name, for example `Kobo Libra Colour`.
    pub model_name: String,
    /// Codename from the platform, for example `monza`.
    pub codename: String,
    /// Product number from the version file or `MODEL_NUMBER`.
    pub product_id: String,
    /// Firmware revision, for example `4.38.23171`, or `None` when unknown.
    pub firmware: Option<String>,
    /// Whether the panel shows colour.
    pub has_color_screen: bool,
    /// Pixels per inch.
    pub dpi: u32,
    /// Portrait width in pixels.
    pub screen_width: u32,
    /// Portrait height in pixels.
    pub screen_height: u32,
}

impl DeviceInfo {
    /// The launcher sets the framebuffer to upright portrait before opening it.
    /// Logical rotation starts from that orientation on the Libra Colour.
    pub fn startup_rotation_degrees(&self) -> u32 {
        0
    }

    /// The Libra Colour. The screen size comes from the Kobo product page
    /// (1264 by 1680), not from KOReader, which reads the framebuffer. Unverified.
    pub fn libra_colour(codename: String, product_id: String, firmware: Option<String>) -> Self {
        DeviceInfo {
            model_id: "Kobo_monza".to_string(),
            model_name: "Kobo Libra Colour".to_string(),
            codename,
            product_id,
            firmware,
            has_color_screen: true,
            dpi: 300,
            screen_width: 1264,
            screen_height: 1680,
        }
    }

    /// The desktop stand-in. It mirrors the Libra Colour screen, so layouts match.
    pub fn desktop(screen_width: u32, screen_height: u32, dpi: u32) -> Self {
        DeviceInfo {
            model_id: "desktop".to_string(),
            model_name: "Desktop".to_string(),
            codename: "desktop".to_string(),
            product_id: String::new(),
            firmware: None,
            has_color_screen: true,
            dpi,
            screen_width,
            screen_height,
        }
    }
}

/// The first line of the version file, if it exists.
pub fn read_version_line(root: &Root) -> Option<String> {
    root.read_line(VERSION_FILE)
}

/// Field 3 of the version line. Empty fields are skipped, as KOReader's `gmatch("([^,]+)")` does.
pub fn firmware_from_version(line: &str) -> Option<String> {
    line.split(',')
        .filter(|field| !field.is_empty())
        .nth(2)
        .map(str::to_string)
}

/// The last three characters of the version line, or `000` when the file is missing.
pub fn product_id_from_version(line: Option<&str>) -> String {
    match line {
        Some(line) => {
            let rev: Vec<char> = line.chars().rev().take(3).collect();
            rev.into_iter().rev().collect()
        }
        None => "000".to_string(),
    }
}

fn first_line(text: &str) -> Option<String> {
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_string())
    }
}

/// The codename, in the order KOReader tries (`device.lua:1158-1197`).
/// `env_product` is the `PRODUCT` variable.
pub fn resolve_codename(env_product: Option<&str>, runner: &mut dyn Runner) -> Option<String> {
    if let Some(product) = env_product.filter(|p| !p.is_empty()) {
        return Some(product.to_string());
    }
    if let Some(name) = runner
        .output("/bin/kobo_config.sh", &[])
        .ok()
        .and_then(|out| first_line(&out))
    {
        return Some(name);
    }
    if let Some(name) = runner
        .output("/usr/bin/hwdetect.sh", &[])
        .ok()
        .and_then(|out| first_line(&out))
    {
        return Some(name);
    }
    // device.lua:1179-1197: device name, then branding with its first letter upper-cased, then screen variant.
    let device = runner
        .output("/usr/bin/hwdetect", &["device-name"])
        .ok()
        .and_then(|o| first_line(&o));
    let branding = runner
        .output("/usr/bin/hwdetect", &["branding"])
        .ok()
        .and_then(|o| first_line(&o));
    let screen = runner
        .output("/usr/bin/hwdetect", &["screen-variant"])
        .ok()
        .and_then(|o| first_line(&o));
    match (device, branding, screen) {
        (Some(device), Some(branding), Some(screen)) => {
            let mut chars = branding.chars();
            let upper: String = match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            };
            Some(format!("{device}{upper}{screen}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Answers `output` calls from a table. Records the calls.
    struct FakeRunner {
        answers: Vec<(&'static str, Vec<&'static str>, &'static str)>,
        calls: RefCell<Vec<String>>,
    }

    impl Runner for FakeRunner {
        fn run(
            &mut self,
            _program: &str,
            _args: &[String],
            _cwd: Option<&std::path::Path>,
            _env: &[(String, String)],
        ) -> std::io::Result<bool> {
            Ok(true)
        }

        fn output(&mut self, program: &str, args: &[&str]) -> std::io::Result<String> {
            self.calls
                .borrow_mut()
                .push(format!("{program} {}", args.join(" ")).trim().to_string());
            for (p, a, out) in &self.answers {
                if *p == program && a.as_slice() == args {
                    return Ok(out.to_string());
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such program",
            ))
        }
    }

    #[test]
    fn env_wins_and_stops_the_search() {
        let mut runner = FakeRunner {
            answers: vec![],
            calls: RefCell::new(vec![]),
        };
        assert_eq!(
            resolve_codename(Some("monza"), &mut runner),
            Some("monza".to_string())
        );
        assert!(runner.calls.borrow().is_empty());
    }

    #[test]
    fn kobo_config_then_hwdetect_script() {
        let mut runner = FakeRunner {
            answers: vec![("/bin/kobo_config.sh", vec![], "\n")],
            calls: RefCell::new(vec![]),
        };
        assert_eq!(resolve_codename(None, &mut runner), None);

        let mut runner = FakeRunner {
            answers: vec![("/usr/bin/hwdetect.sh", vec![], "monza\n")],
            calls: RefCell::new(vec![]),
        };
        assert_eq!(resolve_codename(None, &mut runner), Some("monza".to_string()));
    }

    #[test]
    fn hwdetect_binary_builds_codename() {
        let mut runner = FakeRunner {
            answers: vec![
                ("/usr/bin/hwdetect", vec!["device-name"], "monza"),
                ("/usr/bin/hwdetect", vec!["branding"], "kobo"),
                ("/usr/bin/hwdetect", vec!["screen-variant"], "Colour"),
            ],
            calls: RefCell::new(vec![]),
        };
        assert_eq!(
            resolve_codename(None, &mut runner),
            Some("monzaKoboColour".to_string())
        );
    }

    #[test]
    fn model_table() {
        assert_eq!(Model::from_codename("monza"), Some(Model::LibraColour));
        assert_eq!(Model::from_codename("monzaTolino"), Some(Model::LibraColour));
        assert_eq!(Model::from_codename("spaBW"), None);
    }

    #[test]
    fn version_line_fields() {
        let line = "N3-0-1,4.38.23171,4.38.23171,4.38.23171,a,b,c,320";
        assert_eq!(firmware_from_version(line).as_deref(), Some("4.38.23171"));
        assert_eq!(product_id_from_version(Some(line)), "320");
        assert_eq!(product_id_from_version(None), "000");
        assert_eq!(firmware_from_version("a,,b"), None);
        assert_eq!(firmware_from_version(",,x,y,z"), Some("z".to_string()));
    }
}
