//! Desktop window backend. The panel is shown in a window scaled down to fit the desktop.
//!
//! The window always shows the panel in panel orientation. Rotation turns each frame
//! before it reaches the window, and mouse positions map back to logical coordinates.
//! With `eink_sim`, frames are dithered to the panel's 16 gray levels, and a flash
//! refresh inverts its region for a moment, as an E Ink panel does.
//!
//! Mouse buttons become touch slots through [`kaia_input::DesktopPointer`]. Keys map to
//! [`kaia_input::Key`] as listed on [`DesktopDisplay`].

mod raster;

use std::collections::VecDeque;
use std::thread;
use std::time::{Duration, Instant};

use kaia_gfx::dither::{self, EINK_LEVELS};
use kaia_gfx::{PixelFormat, Pixmap, Point, Size};
use kaia_input::{DesktopPointer, Key, KeyEvent, MouseButton};
use minifb::{Key as WinKey, KeyRepeat, MouseButton as WinButton, MouseMode, Window, WindowOptions};

use crate::display::{check_present, Display, DisplayError, PanelSpec};
use crate::input::{InputEvent, InputSource};
use crate::rotation::{logical_rect_to_panel, rotate_to_panel, Rotation};
use crate::waveform::Refresh;

/// How often `poll` checks the window while it waits for events.
const POLL_INTERVAL: Duration = Duration::from_millis(16);
/// How long an inverted flash stays on screen when `eink_sim` is on.
const FLASH_HOLD: Duration = Duration::from_millis(100);

/// Options for the desktop window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopOptions {
    /// Window size as a percentage of the panel size, from 1 to 100.
    pub scale_percent: u32,
    /// Show the panel as an E Ink screen: 16 gray levels, and flashes invert the region.
    pub eink_sim: bool,
    /// Window title.
    pub title: String,
}

impl Default for DesktopOptions {
    fn default() -> Self {
        Self {
            scale_percent: 50,
            eink_sim: false,
            title: "Kaia".to_string(),
        }
    }
}

/// A panel shown in a desktop window.
///
/// Keys (window key to [`Key`]):
///
/// - `PageDown`, `Right`: [`Key::RPgFwd`] (page forward)
/// - `PageUp`, `Left`: [`Key::RPgBack`] (page back)
/// - `Escape`: [`Key::Back`]
/// - `Enter`, keypad Enter: [`Key::Press`]
/// - `Up`, `Down`, `Backspace`, `Delete`, `Space`: [`Key::Up`], [`Key::Down`],
///   [`Key::Backspace`], [`Key::Del`], `Char(' ')`
/// - `A` to `Z`: `Char('a')` to `Char('z')`, and `0` to `9` as `Char`
///
/// Closing the window yields [`InputEvent::Quit`].
pub struct DesktopDisplay {
    panel: PanelSpec,
    options: DesktopOptions,
    rotation: Rotation,
    window: Window,
    window_size: Size,
    /// The last frame in window pixels. Every window update uploads it.
    buffer: Vec<u32>,
    pending: VecDeque<InputEvent>,
    pointer: DesktopPointer,
    /// Logical position of the pointer at the last check, for move detection.
    pointer_pos: Option<Point>,
    started: Instant,
    quit_sent: bool,
}

impl DesktopDisplay {
    /// Opens a window for `panel`. The window is `scale_percent` of the panel size.
    pub fn open(panel: PanelSpec, options: DesktopOptions) -> Result<Self, DisplayError> {
        if !(1..=100).contains(&options.scale_percent) {
            return Err(DisplayError::InvalidOption(format!(
                "scale_percent must be 1 to 100, got {}",
                options.scale_percent
            )));
        }
        let window_size = Size::new(
            scaled(panel.size.w, options.scale_percent),
            scaled(panel.size.h, options.scale_percent),
        );
        let window = Window::new(
            &options.title,
            window_size.w as usize,
            window_size.h as usize,
            WindowOptions::default(),
        )
        .map_err(|e| DisplayError::Backend(e.to_string()))?;
        let rotation = Rotation::Deg0;
        let pixels = window_size.w as usize * window_size.h as usize;
        Ok(Self {
            panel,
            options,
            rotation,
            window,
            window_size,
            buffer: vec![0x00ff_ffff; pixels],
            pending: VecDeque::new(),
            pointer: DesktopPointer::new(centre(rotation.logical_size(panel.size))),
            pointer_pos: None,
            started: Instant::now(),
            quit_sent: false,
        })
    }

    fn panel_size(&self) -> Size {
        self.panel.size
    }

    /// Reads keys and the mouse since the last window update, and queues the events.
    fn gather(&mut self) {
        let time_us = self.started.elapsed().as_micros() as u64;
        if !self.window.is_open() && !self.quit_sent {
            self.quit_sent = true;
            self.pending.push_back(InputEvent::Quit);
        }
        for physical_key in self.window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(key) = map_key(physical_key) {
                if !matches!(key, Key::LPgBack | Key::LPgFwd | Key::RPgBack | Key::RPgFwd)
                    && !self.window.is_key_pressed(physical_key, KeyRepeat::No)
                {
                    continue;
                }
                self.pending.push_back(InputEvent::Key(KeyEvent {
                    key,
                    pressed: true,
                    time_us,
                }));
            }
        }
        for key in self.window.get_keys_released() {
            if let Some(key) = map_key(key) {
                self.pending.push_back(InputEvent::Key(KeyEvent {
                    key,
                    pressed: false,
                    time_us,
                }));
            }
        }
        self.gather_pointer(time_us);
    }

    fn gather_pointer(&mut self, time_us: u64) {
        let Some((x, y)) = self.window.get_mouse_pos(MouseMode::Clamp) else {
            return;
        };
        let pos = self.window_to_logical(x, y);
        let button = if self.window.get_mouse_down(WinButton::Left) {
            Some(MouseButton::Left)
        } else if self.window.get_mouse_down(WinButton::Right) {
            Some(MouseButton::Right)
        } else {
            None
        };
        let ctrl = self.window.is_key_down(WinKey::LeftCtrl) || self.window.is_key_down(WinKey::RightCtrl);
        let touches = match button {
            Some(b) if !self.pointer.is_pressed() => self.pointer.press(pos, time_us, b, ctrl),
            Some(_) if self.pointer_pos != Some(pos) => self.pointer.move_to(pos, time_us),
            None if self.pointer.is_pressed() => self.pointer.release(pos, time_us),
            _ => Vec::new(),
        };
        self.pointer_pos = Some(pos);
        self.pending.extend(touches.into_iter().map(InputEvent::Touch));
    }

    fn window_to_logical(&self, x: f32, y: f32) -> Point {
        let panel_px = raster::window_to_panel(x, y, self.panel_size(), self.window_size);
        self.rotation.panel_to_logical(panel_px, self.panel_size())
    }

    /// Uploads the buffer to the window. On failure the display reports a quit.
    fn upload(&mut self) {
        let result = self.window.update_with_buffer(
            &self.buffer,
            self.window_size.w as usize,
            self.window_size.h as usize,
        );
        if let Err(e) = result {
            eprintln!("kaia-display: window update failed: {e}");
            if !self.quit_sent {
                self.quit_sent = true;
                self.pending.push_back(InputEvent::Quit);
            }
        }
    }
}

impl Display for DesktopDisplay {
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
        check_present(self.size(), frame, refresh)?;
        let panel_frame = rotate_to_panel(frame, self.rotation);
        let panel_frame = if self.options.eink_sim {
            dither::ordered(&panel_frame, EINK_LEVELS)
        } else {
            panel_frame
        };
        let panel = self.panel_size();
        let pixels = raster::to_xrgb(&panel_frame);
        self.buffer = raster::downscale(
            &pixels,
            panel.w as usize,
            panel.h as usize,
            self.window_size.w as usize,
            self.window_size.h as usize,
        );
        if refresh.flash && self.options.eink_sim {
            let panel_rect = logical_rect_to_panel(refresh.region, self.rotation, panel);
            let rect = raster::panel_rect_to_window(panel_rect, panel, self.window_size);
            let mut flashed = self.buffer.clone();
            raster::invert_region(&mut flashed, self.window_size.w as usize, rect);
            let previous = std::mem::replace(&mut self.buffer, flashed);
            self.upload();
            thread::sleep(FLASH_HOLD);
            self.buffer = previous;
        }
        self.upload();
        self.gather();
        Ok(())
    }

    fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DisplayError> {
        let before = self.size();
        self.rotation = rotation;
        let after = self.size();
        if before != after {
            self.pending.push_back(InputEvent::Resize(after));
        }
        if !self.pointer.is_pressed() {
            self.pointer = DesktopPointer::new(centre(after));
        }
        Ok(())
    }
}

impl InputSource for DesktopDisplay {
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            self.upload();
            self.gather();
            if !self.pending.is_empty() || deadline.is_some_and(|d| Instant::now() >= d) {
                return self.pending.drain(..).collect();
            }
            thread::sleep(POLL_INTERVAL);
        }
    }
}

fn scaled(size: u32, percent: u32) -> u32 {
    (size * percent / 100).max(1)
}

/// The middle of a logical screen, where the second finger of a pinch mirrors around.
fn centre(logical: Size) -> Point {
    Point::new(logical.w as i32 / 2, logical.h as i32 / 2)
}

fn map_key(key: WinKey) -> Option<Key> {
    let code = key as u8;
    let digit_start = WinKey::Key0 as u8;
    let letter_start = WinKey::A as u8;
    if (digit_start..digit_start + 10).contains(&code) {
        return Some(Key::Char((b'0' + code - digit_start) as char));
    }
    if (letter_start..letter_start + 26).contains(&code) {
        return Some(Key::Char((b'a' + code - letter_start) as char));
    }
    Some(match key {
        WinKey::Right | WinKey::PageDown => Key::RPgFwd,
        WinKey::Left | WinKey::PageUp => Key::RPgBack,
        WinKey::Escape => Key::Back,
        WinKey::Enter | WinKey::NumPadEnter => Key::Press,
        WinKey::Up => Key::Up,
        WinKey::Down => Key::Down,
        WinKey::Backspace => Key::Backspace,
        WinKey::Delete => Key::Del,
        WinKey::Space => Key::Char(' '),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_and_navigation_keys_map_as_documented() {
        assert_eq!(map_key(WinKey::PageDown), Some(Key::RPgFwd));
        assert_eq!(map_key(WinKey::Right), Some(Key::RPgFwd));
        assert_eq!(map_key(WinKey::PageUp), Some(Key::RPgBack));
        assert_eq!(map_key(WinKey::Left), Some(Key::RPgBack));
        assert_eq!(map_key(WinKey::Escape), Some(Key::Back));
        assert_eq!(map_key(WinKey::Enter), Some(Key::Press));
        assert_eq!(map_key(WinKey::LeftShift), None);
    }

    #[test]
    fn letters_and_digits_become_chars() {
        assert_eq!(map_key(WinKey::A), Some(Key::Char('a')));
        assert_eq!(map_key(WinKey::Z), Some(Key::Char('z')));
        assert_eq!(map_key(WinKey::Key0), Some(Key::Char('0')));
        assert_eq!(map_key(WinKey::Key9), Some(Key::Char('9')));
    }

    #[test]
    fn scale_keeps_at_least_one_pixel() {
        assert_eq!(scaled(1264, 50), 632);
        assert_eq!(scaled(1, 1), 1);
    }
}
