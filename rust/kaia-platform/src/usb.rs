//! USB plug events and the USB mass storage handover.
//!
//! KOReader listens for kernel uevents with libue (`base/input/input-kobo.h`,
//! `generateFakeEvent`). For `/devices/platform/usb_plug` and
//! `/devices/platform/usb_host` it writes synthetic `EV_KEY` events to the
//! input pipe (`base/input/input.c:38-50` for the codes). The Lua side maps the
//! codes to names (`frontend/device/input.lua:289-294`). [`classify`] is the
//! port of the uevent part. Reading netlink is left to the caller, because it
//! needs a socket that this crate does not open.
//!
//! The handover is `platform/kobo/koreader.sh:421` and `:528-602`. KOReader
//! exits with code 86, and the launcher unpacks `data/KoboUSBMS.tar.gz` into
//! a tmpfs at `/mnt/usbms` and runs `./usbms`. [`handover_plan`] describes the
//! same steps. Unverified on hardware.

use crate::paths::Paths;
use crate::plan::{OnError, Plan, Step};

/// Exit code that asks the launcher to start a USB mass storage session (`koreader.sh:421`).
pub const KO_RC_USBMS: i32 = 86;
/// Mount point for the USB mass storage tool (`koreader.sh:530`).
pub const USBMS_HOME: &str = "/mnt/usbms";
/// Device path of the USB plug (`base/input/input-kobo.h:22`).
pub const USB_PLUG_DEVPATH: &str = "/devices/platform/usb_plug";
/// Device path of the USB host port (`base/input/input-kobo.h:23`).
pub const USB_HOST_DEVPATH: &str = "/devices/platform/usb_host";
/// Device path that KOReader compares for OTG input devices (`base/input/input-kobo.h:24`, `:98`).
pub const PLATFORM_SOC_DEVPATH: &str = "/devices/platform/soc";

/// A USB event, with the synthetic code KOReader uses (`base/input/input.c:42-50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbEvent {
    /// The device was plugged into a host (code 10010, `UsbPlugIn`).
    PluggedIntoHost,
    /// The device was unplugged from a host (code 10011, `UsbPlugOut`).
    UnpluggedFromHost,
    /// A power-only charger was connected (code 10020, `Charging`).
    Charging,
    /// The charger was disconnected (code 10021, `NotCharging`).
    NotCharging,
    /// An OTG input device appeared on this evdev node (code 10040, `UsbDevicePlugIn`).
    DevicePluggedIn(u32),
    /// An OTG input device left this evdev node (code 10041, `UsbDevicePlugOut`).
    DevicePluggedOut(u32),
}

impl UsbEvent {
    /// The synthetic key code (`base/input/input.c:42-50`).
    pub fn code(&self) -> u32 {
        match self {
            UsbEvent::PluggedIntoHost => 10010,
            UsbEvent::UnpluggedFromHost => 10011,
            UsbEvent::Charging => 10020,
            UsbEvent::NotCharging => 10021,
            UsbEvent::DevicePluggedIn(_) => 10040,
            UsbEvent::DevicePluggedOut(_) => 10041,
        }
    }
}

/// The parts of a kernel uevent that [`classify`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uevent<'a> {
    /// `DEVPATH`.
    pub devpath: &'a str,
    /// `SUBSYSTEM`.
    pub subsystem: Option<&'a str>,
    /// `DEVNAME`.
    pub devname: Option<&'a str>,
    /// `add` or `remove`. Other actions are ignored.
    pub action: &'a str,
}

/// Maps a uevent to a USB event, as `generateFakeEvent` does (`input-kobo.h:68-121`).
///
/// The OTG branch compares `devpath` with `/devices/platform/soc` exactly,
/// as the C code does. KOReader's comment there shows longer paths, so this
/// branch may never match on a real device. Unverified.
pub fn classify(ev: &Uevent<'_>) -> Option<UsbEvent> {
    let add = ev.action == "add";
    let remove = ev.action == "remove";
    if ev.devpath == USB_PLUG_DEVPATH {
        return match (add, remove) {
            (true, _) => Some(UsbEvent::Charging),
            (_, true) => Some(UsbEvent::NotCharging),
            _ => None,
        };
    }
    if ev.devpath == USB_HOST_DEVPATH {
        return match (add, remove) {
            (true, _) => Some(UsbEvent::PluggedIntoHost),
            (_, true) => Some(UsbEvent::UnpluggedFromHost),
            _ => None,
        };
    }
    if ev.subsystem == Some("input") && ev.devpath == PLATFORM_SOC_DEVPATH {
        let evdev = ev
            .devname
            .and_then(|name| name.strip_prefix("input/event"))
            .and_then(|n| n.parse::<u32>().ok())?;
        return match (add, remove) {
            (true, _) => Some(UsbEvent::DevicePluggedIn(evdev)),
            (_, true) => Some(UsbEvent::DevicePluggedOut(evdev)),
            _ => None,
        };
    }
    None
}

/// Steps for a mass storage session, from the point where KOReader exits with code 86.
///
/// Mirrors `platform/kobo/koreader.sh:528-602`. `language` is the value for
/// `LANGUAGE` (`koreader.sh:551-553`, `C` when unknown). The mount and
/// unpack failures stop the plan. Unverified on hardware.
pub fn handover_plan(paths: &Paths, language: &str) -> Plan {
    let tar = paths.install_dir.join("tar").to_string_lossy().into_owned();
    let archive = paths.install_dir.join("data/KoboUSBMS.tar.gz");
    let archive = archive.to_string_lossy().into_owned();
    let env = vec![("LANGUAGE".to_string(), language.to_string())];
    let usbms = format!("{USBMS_HOME}/usbms");
    let cleanup = || {
        vec![
            Step::run(
                "umount",
                &[USBMS_HOME],
                None,
                &[],
                "platform/kobo/koreader.sh:597",
            ),
            Step::run(
                "rm",
                &["-rf", USBMS_HOME],
                None,
                &[],
                "platform/kobo/koreader.sh:602",
            ),
        ]
    };
    Plan::new(vec![
        Step::run(
            "mkdir",
            &["-p", USBMS_HOME],
            None,
            &[],
            "platform/kobo/koreader.sh:530",
        ),
        Step::run(
            "mount",
            &[
                "-t",
                "tmpfs",
                "tmpfs",
                USBMS_HOME,
                "-o",
                "defaults,size=32M,mode=1777,noatime",
            ],
            None,
            &[],
            "platform/kobo/koreader.sh:533 (tmpfs; failure restarts KOReader)",
        )
        .on_error(OnError::Abort),
        Step::run(
            &tar,
            &["xzf", &archive, "-C", USBMS_HOME],
            Some(paths.install_dir.as_path()),
            &[],
            "platform/kobo/koreader.sh:538 (unpack KoboUSBMS)",
        )
        .on_error(OnError::Recover(cleanup())),
        Step::run(
            &usbms,
            &[],
            Some(std::path::Path::new(USBMS_HOME)),
            &env,
            "platform/kobo/koreader.sh:564-574 (env LANGUAGE ./usbms)",
        ),
        Step::run(
            "umount",
            &[USBMS_HOME],
            None,
            &[],
            "platform/kobo/koreader.sh:597",
        ),
        Step::run(
            "rm",
            &["-rf", USBMS_HOME],
            None,
            &[],
            "platform/kobo/koreader.sh:602",
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Action;

    fn ev<'a>(
        devpath: &'a str,
        subsystem: Option<&'a str>,
        devname: Option<&'a str>,
        action: &'a str,
    ) -> Uevent<'a> {
        Uevent {
            devpath,
            subsystem,
            devname,
            action,
        }
    }

    #[test]
    fn codes_match_input_c() {
        assert_eq!(UsbEvent::PluggedIntoHost.code(), 10010);
        assert_eq!(UsbEvent::UnpluggedFromHost.code(), 10011);
        assert_eq!(UsbEvent::Charging.code(), 10020);
        assert_eq!(UsbEvent::NotCharging.code(), 10021);
        assert_eq!(UsbEvent::DevicePluggedIn(4).code(), 10040);
        assert_eq!(UsbEvent::DevicePluggedOut(4).code(), 10041);
    }

    #[test]
    fn classify_plug_and_host_events() {
        assert_eq!(
            classify(&ev(USB_PLUG_DEVPATH, None, None, "add")),
            Some(UsbEvent::Charging)
        );
        assert_eq!(
            classify(&ev(USB_PLUG_DEVPATH, None, None, "remove")),
            Some(UsbEvent::NotCharging)
        );
        assert_eq!(
            classify(&ev(USB_HOST_DEVPATH, None, None, "add")),
            Some(UsbEvent::PluggedIntoHost)
        );
        assert_eq!(
            classify(&ev(USB_HOST_DEVPATH, None, None, "remove")),
            Some(UsbEvent::UnpluggedFromHost)
        );
        assert_eq!(classify(&ev(USB_PLUG_DEVPATH, None, None, "change")), None);
        assert_eq!(classify(&ev("/devices/other", None, None, "add")), None);
    }

    #[test]
    fn classify_otg_input_devices() {
        assert_eq!(
            classify(&ev(
                PLATFORM_SOC_DEVPATH,
                Some("input"),
                Some("input/event4"),
                "add"
            )),
            Some(UsbEvent::DevicePluggedIn(4))
        );
        assert_eq!(
            classify(&ev(
                PLATFORM_SOC_DEVPATH,
                Some("input"),
                Some("input/event4"),
                "remove"
            )),
            Some(UsbEvent::DevicePluggedOut(4))
        );
        assert_eq!(
            classify(&ev(
                PLATFORM_SOC_DEVPATH,
                Some("input"),
                Some("input/eventX"),
                "add"
            )),
            None
        );
    }

    #[test]
    fn handover_steps_are_in_koreader_order() {
        let paths = Paths::kobo(&crate::sysfs::Root::new("/"));
        let plan = handover_plan(&paths, "C");
        let programs: Vec<String> = plan
            .steps
            .iter()
            .map(|s| match &s.action {
                Action::Run { program, .. } => program.clone(),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            programs,
            vec![
                "mkdir",
                "mount",
                "/mnt/onboard/.adds/kaia/tar",
                "/mnt/usbms/usbms",
                "umount",
                "rm"
            ]
        );
        match &plan.steps[3].action {
            Action::Run { env, cwd, .. } => {
                assert_eq!(env, &vec![("LANGUAGE".to_string(), "C".to_string())]);
                assert_eq!(cwd.as_deref(), Some(std::path::Path::new(USBMS_HOME)));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
