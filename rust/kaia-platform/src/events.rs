//! Kobo key and sleep cover events, and what each one triggers.
//!
//! KOReader names the keys in `frontend/device/kobo/device.lua:827-832` and the adapters at `device.lua:839-848`
//! (`event_map` and `event_map_adapter`), and `frontend/device/input.lua:832-838`
//! names the Power key edges. The handlers are in `Kobo:setEventHandlers`
//! (`device.lua:1654-1750`). [`react`] returns those decisions as data, and
//! the caller carries them out. Unverified on hardware.

/// Power key (`device.lua:832`, key 116).
pub const POWER_KEY_CODE: u16 = 116;
/// Light button (`device.lua:830`, key 90). Acts on release (`device.lua:846-848`).
pub const LIGHT_KEY_CODE: u16 = 90;
/// Sleep cover keys (`device.lua:828`, keys 35 and 59).
pub const SLEEP_COVER_KEY_CODES: [u16; 2] = [35, 59];
/// Nickel software power-off hold time.
pub const POWER_OFF_HOLD: std::time::Duration = std::time::Duration::from_millis(750);

/// Key press or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEdge {
    /// Key went down.
    Press,
    /// Key came up.
    Release,
}

/// The named event for a key edge. `None` for keys this module does not handle.
pub fn key_event(code: u16, edge: KeyEdge) -> Option<&'static str> {
    match (code, edge) {
        (POWER_KEY_CODE, KeyEdge::Press) => Some("PowerPress"),
        (POWER_KEY_CODE, KeyEdge::Release) => Some("PowerRelease"),
        (LIGHT_KEY_CODE, KeyEdge::Release) => Some("Light"),
        (35 | 59, KeyEdge::Press) => Some("SleepCoverClosed"),
        (35 | 59, KeyEdge::Release) => Some("SleepCoverOpened"),
        _ => None,
    }
}

/// Settings and state that change which handler wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EventState {
    /// The screensaver is showing (`generic/device.lua:413`).
    pub screen_saver_mode: bool,
    /// The screensaver is locked, so wake-ups are ignored (`generic/device.lua:434`).
    pub screen_saver_lock: bool,
    /// The power off stage has started. Release does nothing then (`device.lua:1670-1671`).
    pub poweroff_stage: bool,
    /// Sleep cover events are ignored (`ignore_power_sleepcover`, `device.lua:1728-1731`).
    pub ignore_power_sleepcover: bool,
    /// Opening the cover does not wake the device (`ignore_open_sleepcover`, `device.lua:1733-1740`).
    pub ignore_open_sleepcover: bool,
}

/// What the caller should do after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reaction {
    /// Power off after the hold delay.
    ArmPowerOff {
        /// Time to wait.
        delay: std::time::Duration,
    },
    /// Cancel a pending power off (`device.lua:1671`).
    CancelPowerOff,
    /// Suspend the device (`UIManager.event_handlers.Suspend`).
    Suspend,
    /// Resume the device.
    Resume,
    /// Toggle the frontlight (`device.lua:1684-1686`).
    ToggleFrontlight,
    /// Record whether the sleep cover is closed (`device.lua:1738-1748`).
    SetCoverClosed(bool),
    /// Broadcast a named event to the UI (`Charging` or `NotCharging`).
    Broadcast(&'static str),
    /// Offer USB mass storage (`device.lua:1710-1712`).
    StartMassStorage,
    /// Close the mass storage prompt (`device.lua:1722-1724`).
    DismissMassStorage,
}

/// The reactions to a named event, in order. Event names are those in [`key_event`]
/// and the USB names in `frontend/device/input.lua:289-294`.
pub fn react(event: &str, st: &EventState) -> Vec<Reaction> {
    let suspend_on_plug = st.screen_saver_mode && !st.screen_saver_lock;
    match event {
        "PowerPress" => vec![Reaction::ArmPowerOff {
            delay: POWER_OFF_HOLD,
        }],
        "PowerRelease" => {
            if st.poweroff_stage {
                return Vec::new();
            }
            let wake = if st.screen_saver_mode {
                if st.screen_saver_lock {
                    Reaction::Suspend
                } else {
                    Reaction::Resume
                }
            } else {
                Reaction::Suspend
            };
            vec![Reaction::CancelPowerOff, wake]
        }
        "Light" => vec![Reaction::ToggleFrontlight],
        // device.lua:1688-1694: power-only charger.
        "Charging" => {
            let mut out = vec![Reaction::Broadcast("Charging")];
            if suspend_on_plug {
                out.push(Reaction::Suspend);
            }
            out
        }
        // device.lua:1695-1702.
        "NotCharging" => {
            let mut out = vec![Reaction::Broadcast("NotCharging")];
            if suspend_on_plug {
                out.push(Reaction::Suspend);
            }
            out
        }
        // device.lua:1704-1713: data-aware host. Offers mass storage when awake.
        "UsbPlugIn" => {
            let mut out = vec![Reaction::Broadcast("Charging")];
            if suspend_on_plug {
                out.push(Reaction::Suspend);
            } else if !st.screen_saver_lock {
                out.push(Reaction::StartMassStorage);
            }
            out
        }
        // device.lua:1715-1726.
        "UsbPlugOut" => {
            let mut out = vec![Reaction::Broadcast("NotCharging")];
            if suspend_on_plug {
                out.push(Reaction::Suspend);
            } else if !st.screen_saver_lock {
                out.push(Reaction::DismissMassStorage);
            }
            out
        }
        "SleepCoverClosed" => {
            if st.ignore_power_sleepcover {
                Vec::new()
            } else if st.ignore_open_sleepcover {
                vec![Reaction::Suspend]
            } else {
                vec![Reaction::SetCoverClosed(true), Reaction::Suspend]
            }
        }
        "SleepCoverOpened" => {
            if st.ignore_power_sleepcover {
                Vec::new()
            } else if st.ignore_open_sleepcover {
                vec![Reaction::SetCoverClosed(false)]
            } else {
                vec![Reaction::SetCoverClosed(false), Reaction::Resume]
            }
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names() {
        assert_eq!(key_event(116, KeyEdge::Press), Some("PowerPress"));
        assert_eq!(key_event(116, KeyEdge::Release), Some("PowerRelease"));
        assert_eq!(key_event(90, KeyEdge::Press), None);
        assert_eq!(key_event(90, KeyEdge::Release), Some("Light"));
        assert_eq!(key_event(59, KeyEdge::Press), Some("SleepCoverClosed"));
        assert_eq!(key_event(35, KeyEdge::Release), Some("SleepCoverOpened"));
        assert_eq!(key_event(1, KeyEdge::Press), None);
    }

    #[test]
    fn power_release_wakes_or_suspends() {
        let awake = EventState::default();
        assert_eq!(
            react("PowerRelease", &awake),
            vec![Reaction::CancelPowerOff, Reaction::Suspend]
        );
        let saver = EventState {
            screen_saver_mode: true,
            ..Default::default()
        };
        assert_eq!(
            react("PowerRelease", &saver),
            vec![Reaction::CancelPowerOff, Reaction::Resume]
        );
        let locked = EventState {
            screen_saver_mode: true,
            screen_saver_lock: true,
            ..Default::default()
        };
        assert_eq!(
            react("PowerRelease", &locked),
            vec![Reaction::CancelPowerOff, Reaction::Suspend]
        );
        let stage = EventState {
            poweroff_stage: true,
            ..Default::default()
        };
        assert!(react("PowerRelease", &stage).is_empty());
    }

    #[test]
    fn power_press_arms_750_ms_power_off() {
        assert_eq!(
            react("PowerPress", &EventState::default()),
            vec![Reaction::ArmPowerOff {
                delay: std::time::Duration::from_millis(750)
            }]
        );
    }

    #[test]
    fn usb_plug_in_offers_mass_storage_when_awake() {
        assert_eq!(
            react("UsbPlugIn", &EventState::default()),
            vec![Reaction::Broadcast("Charging"), Reaction::StartMassStorage]
        );
        let saver = EventState {
            screen_saver_mode: true,
            ..Default::default()
        };
        assert_eq!(
            react("UsbPlugIn", &saver),
            vec![Reaction::Broadcast("Charging"), Reaction::Suspend]
        );
    }

    #[test]
    fn charging_events_suspend_only_in_unlocked_screensaver() {
        let saver = EventState {
            screen_saver_mode: true,
            ..Default::default()
        };
        assert_eq!(
            react("Charging", &saver),
            vec![Reaction::Broadcast("Charging"), Reaction::Suspend]
        );
        assert_eq!(
            react("NotCharging", &EventState::default()),
            vec![Reaction::Broadcast("NotCharging")]
        );
    }

    #[test]
    fn sleep_cover_respects_settings() {
        let default = EventState::default();
        assert_eq!(
            react("SleepCoverClosed", &default),
            vec![Reaction::SetCoverClosed(true), Reaction::Suspend]
        );
        assert_eq!(
            react("SleepCoverOpened", &default),
            vec![Reaction::SetCoverClosed(false), Reaction::Resume]
        );
        let ignore_open = EventState {
            ignore_open_sleepcover: true,
            ..Default::default()
        };
        assert_eq!(
            react("SleepCoverOpened", &ignore_open),
            vec![Reaction::SetCoverClosed(false)]
        );
        let ignore_all = EventState {
            ignore_power_sleepcover: true,
            ..Default::default()
        };
        assert!(react("SleepCoverClosed", &ignore_all).is_empty());
        assert!(react("SleepCoverOpened", &ignore_all).is_empty());
    }
}
