//! Kobo display, touch frames and USB handover to Nickel.

use std::time::Duration;

use kaia_app::Flow;
use kaia_display::kobo::{EvdevInput, KoboDisplay, Uevents};
use kaia_display::{Display, DisplayError, InputEvent, InputSource, Refresh, Rotation};
use kaia_gfx::{PixelFormat, Pixmap, Size};
use kaia_platform::usb::{self, UsbEvent};
use kaia_platform::{Kind, Model, Platform};

use super::{Options, Result};

struct KoboBackend {
    display: Box<dyn Display>,
    input: EvdevInput,
    page_buttons: [bool; 2],
}

impl Display for KoboBackend {
    fn size(&self) -> Size {
        self.display.size()
    }
    fn format(&self) -> PixelFormat {
        self.display.format()
    }
    fn dpi(&self) -> u32 {
        self.display.dpi()
    }
    fn present(&mut self, frame: &Pixmap, refresh: &Refresh) -> std::result::Result<(), DisplayError> {
        self.display.present(frame, refresh)
    }
    fn set_rotation(&mut self, rotation: Rotation) -> std::result::Result<(), DisplayError> {
        self.display.set_rotation(rotation)?;
        self.input.set_rotation(rotation);
        Ok(())
    }
    fn wait_idle(&mut self) {
        self.display.wait_idle();
    }
    fn set_night_mode(&mut self, on: bool) {
        self.display.set_night_mode(on);
    }
}

impl InputSource for KoboBackend {
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent> {
        // Bound the wait so USB events arrive even when no one touches the screen.
        let events = self.input.poll(Some(
            timeout
                .unwrap_or(Duration::from_millis(250))
                .min(Duration::from_millis(250)),
        ));
        for event in &events {
            if let InputEvent::Key(key) = event {
                match key.key {
                    kaia_input::Key::RPgBack => self.page_buttons[0] = key.pressed,
                    kaia_input::Key::RPgFwd => self.page_buttons[1] = key.pressed,
                    _ => {}
                }
                if self.page_buttons == [true, true] {
                    return vec![InputEvent::Quit];
                }
            }
        }
        events
    }
}

pub(super) fn run(options: Options) -> Result<()> {
    let platform = Platform::detect()?;
    if platform.kind != Kind::Kobo(Model::LibraColour) {
        return Err("the Kobo backend requires a Libra Colour; use --headless without a device".into());
    }
    let rotation = match platform.device.startup_rotation_degrees() {
        90 => Rotation::Deg90,
        180 => Rotation::Deg180,
        270 => Rotation::Deg270,
        _ => Rotation::Deg0,
    };
    let resources = kaia_doc::cre::CreOptions::default();
    if !resources.data_dir.join("epub.css").is_file() || resources.font_files().is_empty() {
        return Err("missing CREngine data or fonts beside the kaia binary".into());
    }
    eprintln!(
        "kaia: {} firmware {:?}, rotation {}",
        platform.device.model_name,
        platform.device.firmware,
        rotation.degrees()
    );
    eprintln!(
        "kaia: data {}, books {}",
        platform.paths.data_dir.display(),
        platform.paths.books_dir.display()
    );
    let mut backend = KoboBackend {
        display: KoboDisplay::open_auto()?,
        input: EvdevInput::libra_colour(rotation)?,
        page_buttons: [false; 2],
    };
    backend.set_rotation(rotation)?;
    let uevents = Uevents::open()?;
    let mut app = super::prepare(backend, platform, &options)?;
    let result = (|| {
        let mut opened = options.open.is_none();
        loop {
            for bytes in uevents.drain()? {
                if let Some(event) = usb::classify_bytes(&bytes) {
                    eprintln!("kaia: USB {event:?}");
                    match event {
                        UsbEvent::PluggedIntoHost => {
                            eprintln!("kaia: saving and returning to Nickel for USB");
                            return Ok(());
                        }
                        UsbEvent::Charging | UsbEvent::NotCharging => {
                            app.broadcast_app(kaia_events::Event::new(if event == UsbEvent::Charging {
                                "Charging"
                            } else {
                                "NotCharging"
                            }))
                        }
                        _ => {}
                    }
                }
            }
            if !opened && !app.scan_running() {
                super::open_requested(&mut app, &options)?;
                opened = true;
            }
            if app.iterate(Some(Duration::from_millis(250)))? == Flow::Exit {
                return Ok(());
            }
        }
    })();
    app.shutdown();
    for notice in app.notices() {
        eprintln!("kaia: {notice}");
    }
    result
}
