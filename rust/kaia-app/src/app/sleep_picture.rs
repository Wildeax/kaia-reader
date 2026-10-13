//! The cover on the sleep screen, prepared on a worker thread.
//!
//! Drawing the cover means a library read, a decode, a trim and a scale to the
//! screen. Built on the app thread, that took 950 ms for one test book in a debug
//! build. The worker does that work for the book being read, as soon as the book
//! opens. Power then draws the finished picture. If Power comes first, the screen
//! shows the label on a blank page, and `App` swaps in the cover when the worker
//! is done.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use kaia_gfx::image::{scale_pixmap, FitMode};
use kaia_gfx::{Pixmap, Size};
use kaia_library::Library;
use kaia_ui::components::{Align, Image, Label, Padding, Stack};
use kaia_ui::geometry::Insets;
use kaia_ui::{Alignment, Tone, WidgetPod};

use crate::services::Services;
use crate::surfaces::common;
use crate::surfaces::settings::sleep;

/// A book and the screen size. A picture is only valid for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    book: i64,
    screen: Size,
}

enum Picture {
    Pending,
    /// The scaled cover, or None when the book has none.
    Ready(Option<Pixmap>),
}

/// The newest request and its state. Replies for any other key are dropped.
pub(super) struct SleepPicture {
    requests: Sender<Key>,
    replies: Receiver<(Key, Option<Pixmap>)>,
    current: Option<(Key, Picture)>,
}

impl SleepPicture {
    /// Starts the worker. It opens its own connection to the library at `library`.
    pub(super) fn new(library: PathBuf) -> Self {
        let (requests, asked) = mpsc::channel::<Key>();
        let (done, replies) = mpsc::channel();
        std::thread::spawn(move || {
            let library = Library::open(&library).ok();
            while let Ok(mut key) = asked.recv() {
                // Only the newest request matters. An older book is no longer being read.
                while let Ok(newer) = asked.try_recv() {
                    key = newer;
                }
                let picture = library.as_ref().and_then(|lib| prepare(lib, key));
                if done.send((key, picture)).is_err() {
                    break;
                }
            }
        });
        Self {
            requests,
            replies,
            current: None,
        }
    }

    /// Asks for the picture of `book` at `screen`. A request for the same key that
    /// is already current does nothing.
    pub(super) fn request(&mut self, book: i64, screen: Size) {
        let key = Key { book, screen };
        if self.current.as_ref().is_some_and(|(current, _)| *current == key) {
            return;
        }
        self.current = Some((key, Picture::Pending));
        let _ = self.requests.send(key);
    }

    /// Takes the replies that arrived since the last call.
    pub(super) fn poll(&mut self) {
        while let Ok((key, picture)) = self.replies.try_recv() {
            if let Some((current, state)) = &mut self.current {
                if *current == key {
                    *state = Picture::Ready(picture);
                }
            }
        }
    }

    /// Whether the current request is still running.
    pub(super) fn busy(&self) -> bool {
        matches!(self.current, Some((_, Picture::Pending)))
    }

    /// The picture for `book` at `screen`. `Some(None)` means the book has no cover.
    /// `None` means the picture is not ready, or the current request is for another key.
    pub(super) fn ready(&self, book: i64, screen: Size) -> Option<Option<&Pixmap>> {
        let wanted = Key { book, screen };
        match &self.current {
            Some((key, Picture::Ready(picture))) if *key == wanted => Some(picture.as_ref()),
            _ => None,
        }
    }
}

/// The cover of `key.book`, cut and scaled the way the sleep screen draws it.
fn prepare(library: &Library, key: Key) -> Option<Pixmap> {
    let cover = common::cover(library, key.book)?;
    let cover = kaia_ui::composite::trim_blank_margins(&cover);
    let source = cover.size();
    let k = (f64::from(key.screen.w) / f64::from(source.w.max(1)))
        .min(f64::from(key.screen.h) / f64::from(source.h.max(1)));
    let target = Size::new(
        (f64::from(source.w) * k).round() as u32,
        (f64::from(source.h) * k).round() as u32,
    );
    Some(scale_pixmap(&cover, target))
}

/// The sleep screen with `picture` as its background, or the blank sleep screen
/// when there is no picture. The layout matches `settings::sleep::sleep_screen_with_label`.
pub(super) fn sleep_screen(services: &Services, screen: Size, picture: Option<Pixmap>, label: &str) -> Stack {
    let Some(picture) = picture else {
        return sleep::sleep_screen_with_label(services, screen, None, label);
    };
    let t = &services.theme;
    let target = picture.size();
    let insets = Insets {
        left: screen.w.saturating_sub(target.w) / 2,
        top: screen.h.saturating_sub(target.h) / 2,
        right: 0,
        bottom: 0,
    };
    let background = WidgetPod::new(Padding::new(
        WidgetPod::new(Image::new(picture, FitMode::Contain)),
        insets,
    ));
    let label = WidgetPod::new(Padding::new(
        WidgetPod::new(Label::new(label, t.font.caption.clone(), Tone::InkSecondary)),
        Insets::all(t.space.md),
    ));
    let label = WidgetPod::new(Align::new(label, Alignment::Center, Alignment::End));
    Stack::new(vec![background, label])
}
