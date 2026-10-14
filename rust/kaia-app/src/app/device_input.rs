//! Device settings, orientation and USB events handled by the app loop.

use kaia_display::{DisplayError, InputEvent, Rotation};
use kaia_events::{DirtyQueue, Event, RefreshMode, Value};
use kaia_gfx::{Pixmap, Size};
use kaia_i18n::{format as tr_format, tr};
use kaia_input::Key;
use kaia_platform::usb::UsbEvent;

use super::App;
use crate::backend::Backend;
use crate::surface::SurfaceEnv;
use crate::surfaces::settings::{self, keys};

pub(super) fn rotation(degrees: i64) -> Rotation {
    match degrees {
        90 => Rotation::Deg90,
        180 => Rotation::Deg180,
        270 => Rotation::Deg270,
        _ => Rotation::Deg0,
    }
}

pub(super) fn is_page_key(key: Key) -> bool {
    matches!(key, Key::LPgBack | Key::LPgFwd | Key::RPgBack | Key::RPgFwd)
}

pub(super) fn page_key(key: Key, rotation: Rotation, inverted: bool) -> Key {
    // KOReader modes 1 and 2 reverse the bezel keys. Mode 3 keeps their order.
    if inverted ^ matches!(rotation, Rotation::Deg90 | Rotation::Deg180) {
        match key {
            Key::LPgBack => Key::LPgFwd,
            Key::LPgFwd => Key::LPgBack,
            Key::RPgBack => Key::RPgFwd,
            Key::RPgFwd => Key::RPgBack,
            _ => key,
        }
    } else {
        key
    }
}

impl<B: Backend> App<B> {
    pub(super) fn counts_as_activity(&self, event: &InputEvent) -> bool {
        if self.sleeping() {
            return false;
        }
        match event {
            InputEvent::Touch(_) | InputEvent::Light => true,
            InputEvent::TouchFrame(frame) => !frame.is_empty(),
            InputEvent::Key(k) if matches!(k.key, Key::SleepCoverClosed | Key::SleepCoverOpened) => {
                !self.usb_mass_storage
                    && keys::bool_or(&self.services.settings, keys::DEVICE_SLEEP_COVER, true)
            }
            InputEvent::Key(_) => true,
            _ => false,
        }
    }

    /// Tracks USB storage ownership. The Kobo runner hands storage to Nickel.
    /// Unplug starts a new scan, or queues one after the current scan finishes.
    pub fn usb_event(&mut self, event: UsbEvent) {
        match event {
            UsbEvent::PluggedIntoHost => self.usb_mass_storage = true,
            UsbEvent::UnpluggedFromHost => {
                self.usb_mass_storage = false;
                self.note_activity();
                self.start_scan();
            }
            _ => {}
        }
    }

    pub(super) fn device_command(&mut self, id: &str, args: &[Value]) -> bool {
        match id {
            "settings:quick:rotation-lock" | "settings:set:display.rotation_lock" => {
                if let Some(on) = args.first().and_then(Value::as_bool) {
                    self.services
                        .settings
                        .save(keys::DISPLAY_ROTATION_LOCK, Value::Bool(on));
                    if on {
                        self.pending_rotation = None;
                    }
                    settings::save(&mut env!(self));
                }
                true
            }
            "settings:quick:orientation" => {
                self.back();
                let entries = settings::quick::orientation_picker(&self.services);
                env!(self).sheet(entries);
                true
            }
            "settings:quick:orientation-pick" | "settings:pick:display.rotation" => {
                if let Some(index) = args
                    .first()
                    .and_then(Value::as_i64)
                    .filter(|n| (0..4).contains(n))
                {
                    self.pending_rotation = Some(rotation(index * 90));
                    if id == "settings:quick:orientation-pick" {
                        self.back();
                    }
                }
                true
            }
            "settings:set:device.page_button_wake" if !self.services.platform.supports_page_button_wake() => {
                true
            }
            _ => false,
        }
    }

    pub(super) fn rotate(&mut self, rotation: Rotation) -> Result<(), DisplayError> {
        if self.sleeping() {
            self.pending_rotation = Some(rotation);
            return Ok(());
        }
        self.backend.set_rotation(rotation)?;
        self.rotation = rotation;
        self.services
            .settings
            .save(keys::DISPLAY_ROTATION, Value::Number(rotation.degrees().into()));
        if let Err(error) = self.services.settings.flush(&self.services.settings_path()) {
            self.notices.push(tr_format(
                &tr("Cannot save orientation: %1"),
                &[&error.to_string()],
            ));
        }
        self.resize(self.backend.size());
        Ok(())
    }

    pub(super) fn resize(&mut self, size: Size) {
        self.screen = size;
        self.services.theme.scale.width = size.w;
        self.services.theme.scale.height = size.h;
        self.frame = Pixmap::new(size.w, size.h, self.services.theme.pixel_format());
        self.ctx.dirty = DirtyQueue::new(kaia_events::Rect::new(0, 0, size.w as i32, size.h as i32));
        self.ctx.dirty.configure_from(&self.services.settings);
        self.ctx.dirty.full_refresh_count = 0;
        for id in self.stack.ids_top_down() {
            if let Some(window) = self.app_window(id) {
                window.resize(size);
            }
        }
        // The reader handles this event by changing its document viewport on its worker.
        self.broadcast_app(Event::with_args(
            "ScreenResized",
            vec![Value::Number(size.w.into()), Value::Number(size.h.into())],
        ));
        self.rebuild_all();
        self.ctx.dirty.refresh(RefreshMode::Full, None, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_keys_follow_all_orientations_and_inversion() {
        for (rotation, swaps) in [
            (Rotation::Deg0, false),
            (Rotation::Deg90, true),
            (Rotation::Deg180, true),
            (Rotation::Deg270, false),
        ] {
            for inverted in [false, true] {
                for (back, forward) in [(Key::RPgBack, Key::RPgFwd), (Key::LPgBack, Key::LPgFwd)] {
                    assert_eq!(
                        page_key(back, rotation, inverted),
                        if swaps ^ inverted { forward } else { back }
                    );
                    assert_eq!(
                        page_key(forward, rotation, inverted),
                        if swaps ^ inverted { back } else { forward }
                    );
                }
                assert_eq!(page_key(Key::Power, rotation, inverted), Key::Power);
            }
        }
    }
}
