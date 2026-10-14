//! Backends without a window: a display that keeps the last frame and records every
//! refresh, and an input source that replays a list of events.

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use kaia_gfx::png::{self, PngError};
use kaia_gfx::{draw, PixelFormat, Pixmap, Point, Rect, Size};
use kaia_input::{parse_script, ScriptError, Step};

use crate::display::{check_present, Display, DisplayError, PanelSpec};
use crate::input::{InputEvent, InputSource};
use crate::rotation::Rotation;
use crate::waveform::Refresh;

/// A display with no hardware. It keeps what the panel shows and records every
/// [`Refresh`], so tests can check what was drawn and which regions and waveforms
/// were requested.
///
/// The panel keeps its pixels outside a refresh region, as an E Ink panel does. A
/// present copies only its region into the panel. A full-screen present, and the first
/// present after a rotation, copy the whole frame. A widget that forgets to mark its
/// region dirty therefore leaves stale pixels here, as it would on the device.
#[derive(Debug, Clone)]
pub struct HeadlessDisplay {
    panel: PanelSpec,
    rotation: Rotation,
    frame: Pixmap,
    /// Set by a rotation and cleared by the next present, which copies the whole frame.
    rotated: bool,
    refreshes: Vec<Refresh>,
}

impl HeadlessDisplay {
    /// A display for `panel`, at rotation 0. The snapshot starts white.
    pub fn new(panel: PanelSpec) -> Self {
        Self {
            panel,
            rotation: Rotation::Deg0,
            frame: Pixmap::new(panel.size.w, panel.size.h, panel.format),
            rotated: false,
            refreshes: Vec::new(),
        }
    }

    /// What the panel shows, in logical orientation. Each present replaces its refresh
    /// region and leaves the rest as it was. A rotation resets the snapshot to white at
    /// the new size.
    pub fn snapshot(&self) -> &Pixmap {
        &self.frame
    }

    /// Every successful `present` call so far, in order.
    pub fn refreshes(&self) -> &[Refresh] {
        &self.refreshes
    }

    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Writes the snapshot as a PNG file, replacing any file at `path`.
    pub fn save_png(&self, path: &Path) -> Result<(), PngError> {
        png::write_pixmap(&self.frame, path)
    }
}

impl Display for HeadlessDisplay {
    fn size(&self) -> Size {
        self.rotation.logical_size(self.panel.size)
    }

    fn format(&self) -> PixelFormat {
        self.panel.format
    }

    fn dpi(&self) -> u32 {
        self.panel.dpi
    }

    fn present(&mut self, frame: &Pixmap, refresh: &Refresh) -> Result<(), DisplayError> {
        let screen = self.size();
        check_present(screen, frame, refresh)?;
        let region = refresh.region;
        if self.rotated || region == Rect::from_size(screen) {
            self.frame = frame.clone();
            self.rotated = false;
        } else if !region.is_empty() {
            let at = Point::new(region.x, region.y);
            draw::blit(&mut self.frame, frame, region, at);
        }
        self.refreshes.push(*refresh);
        Ok(())
    }

    fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DisplayError> {
        let before = self.size();
        if rotation != self.rotation {
            self.rotated = true;
        }
        self.rotation = rotation;
        if self.size() != before {
            let size = self.size();
            self.frame = Pixmap::new(size.w, size.h, self.panel.format);
        }
        Ok(())
    }
}

/// An input source that yields a fixed list of events.
///
/// It never waits. Each `poll` returns every queued event at once, and the timeout is
/// ignored. When the queue is empty, `poll` returns no events. Use [`ScriptedInput::is_done`]
/// to stop a loop that would otherwise wait for input forever.
#[derive(Debug, Clone, Default)]
pub struct ScriptedInput {
    queue: VecDeque<InputEvent>,
}

impl ScriptedInput {
    /// Queues `events` in the order given.
    pub fn new(events: impl IntoIterator<Item = InputEvent>) -> Self {
        Self {
            queue: events.into_iter().collect(),
        }
    }

    /// Builds the touch events from a `kaia_input` replay script. The `poll` lines
    /// produce no events here, because they only fire gesture timers. Key events cannot
    /// be written in the script format, so add them with [`ScriptedInput::new`].
    pub fn from_script(script: &str) -> Result<Self, ScriptError> {
        let events = parse_script(script)?.into_iter().filter_map(|step| match step {
            Step::Touch(touch) => Some(InputEvent::Touch(touch)),
            Step::Poll(_) => None,
        });
        Ok(Self::new(events))
    }

    /// True when every queued event has been returned.
    pub fn is_done(&self) -> bool {
        self.queue.is_empty()
    }
}

impl InputSource for ScriptedInput {
    fn poll(&mut self, _timeout: Option<Duration>) -> Vec<InputEvent> {
        self.queue.drain(..).collect()
    }
}
