//! Screenshots. A two-finger swipe along a screen diagonal saves the frame on
//! screen as a PNG. The app thread copies the frame and starts a worker. The
//! worker encodes and writes the file, and the app thread shows the toast when
//! the worker reports back.

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use chrono::{DateTime, Local};
use kaia_gfx::{png, Pixmap, Size};
use kaia_i18n::{format as tr_format, tr};
use kaia_input::{Direction, Gesture};
use kaia_ui::Scale;

use super::App;
use crate::backend::Backend;
use crate::surface::SurfaceEnv;

/// The folder for screenshots. It sits in the books folder and in the data folder.
pub const FOLDER: &str = "screenshots";

/// The swipe must span the screen diagonal minus this many units, as in KOReader's
/// screenshot gesture (`diagonal - scaleBySize(200)`).
const DIAGONAL_SLACK_UNITS: u32 = 200;

/// How long the save toast stays on screen.
const SAVED_TOAST_SECONDS: f64 = 2.0;

/// How long the error toast stays on screen.
const ERROR_TOAST_SECONDS: f64 = 5.0;

/// True for a two-finger swipe along a diagonal that spans most of the screen.
pub fn is_gesture(gesture: &Gesture, screen: Size, scale: &Scale) -> bool {
    let Gesture::TwoFingerSwipe {
        direction: Some(direction),
        distance,
        ..
    } = gesture
    else {
        return false;
    };
    let diagonal = f64::from(screen.w).hypot(f64::from(screen.h));
    let min_distance = diagonal - f64::from(scale.dp(DIAGONAL_SLACK_UNITS));
    is_diagonal(*direction) && *distance >= min_distance
}

fn is_diagonal(direction: Direction) -> bool {
    matches!(
        direction,
        Direction::NorthEast | Direction::NorthWest | Direction::SouthEast | Direction::SouthWest
    )
}

/// Saves that are still running. Each one ends with a toast.
#[derive(Default)]
pub struct Saves {
    pending: Vec<Receiver<Result<PathBuf, String>>>,
}

impl Saves {
    /// Starts a save of `frame`. The file name uses the local time `when`. The
    /// worker tries each folder in order and keeps the first one that accepts the file.
    pub fn start(&mut self, frame: Pixmap, folders: Vec<PathBuf>, when: DateTime<Local>) {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let stamp = when.format("%Y-%m-%d_%H%M%S").to_string();
            let _ = sender.send(save(&frame, &folders, &stamp));
        });
        self.pending.push(receiver);
    }

    /// The saves that finished since the last call. A worker that died without a
    /// result counts as a failed save.
    pub fn poll(&mut self) -> Vec<Result<PathBuf, String>> {
        let mut done = Vec::new();
        self.pending.retain(|receiver| match receiver.try_recv() {
            Ok(result) => {
                done.push(result);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                done.push(Err("the save stopped without a result".to_string()));
                false
            }
        });
        done
    }
}

/// Encodes `frame` and writes it as `Kaia_<stamp>.png` in the first folder that
/// accepts it. The name gets `_2`, `_3` and so on when the file exists already.
/// Runs on the worker, so it does not use [`tr`].
fn save(frame: &Pixmap, folders: &[PathBuf], stamp: &str) -> Result<PathBuf, String> {
    let bytes = png::encode_pixmap(frame).map_err(|e| e.to_string())?;
    let mut last = String::from("no folder to save to");
    for folder in folders {
        match write_new(folder, stamp, &bytes) {
            Ok(path) => return Ok(path),
            Err(e) => last = format!("{}: {e}", folder.display()),
        }
    }
    Err(last)
}

fn write_new(folder: &Path, stamp: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    fs::create_dir_all(folder)?;
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("Kaia_{stamp}.png")
        } else {
            format!("Kaia_{stamp}_{n}.png")
        };
        let path = folder.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                return Ok(path);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

impl<B: Backend> App<B> {
    /// Copies the frame on screen and starts the save. The toast comes later, from
    /// [`App::poll_screenshots`]. The copy is taken before any toast of this gesture
    /// is painted, so the toast is not in the file.
    pub(super) fn save_screenshot(&mut self) {
        let books = self.services.paths.books_dir.join(FOLDER);
        let data = self.services.paths.data_dir.join(FOLDER);
        let frame = self.frame.clone();
        self.screenshots.start(frame, vec![books, data], Local::now());
    }

    /// Shows the toast for each save that finished.
    pub(super) fn poll_screenshots(&mut self) {
        for result in self.screenshots.poll() {
            match result {
                Ok(_) => {
                    env!(self).toast(tr("Screenshot saved"), SAVED_TOAST_SECONDS);
                }
                Err(error) => {
                    let text = tr_format(&tr("Could not save the screenshot: %1"), &[&error]);
                    env!(self).toast(text, ERROR_TOAST_SECONDS);
                }
            }
        }
    }
}
