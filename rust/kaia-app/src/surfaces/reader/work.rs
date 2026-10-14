//! Blocking session calls run away from the app. The document stays on its engine thread.
use super::page::{Mark, MarkStyle};
use kaia_book::{BookSession, Drawer, Selection, Size};
use kaia_gfx::{Color, Pixmap, Rect};
use kaia_i18n::{format as tr_format, tr};
use std::sync::{mpsc, Arc};

pub(super) struct Frame {
    pub session: BookSession,
    pub page: Option<Arc<Pixmap>>,
    pub marks: Vec<Mark>,
    /// True when the page has a bookmark. The page draws its dog-ear.
    pub bookmarked: bool,
    pub hits: Vec<Rect>,
    pub links: Vec<(Rect, kaia_book::Link)>,
    pub selection: Option<Selection>,
    pub error: Option<WorkError>,
    pub stickers: Vec<super::stickers::Drawing>,
    pub sticker_anchor: Option<kaia_storage::Anchor>,
}

/// An error from the engine thread. That thread has no catalog, so the sentence is
/// built by [`WorkError::message`] on the UI thread, when the page shows it.
pub(super) enum WorkError {
    Open(String),
    Turn(String),
    GoTo(String),
    Draw(String),
    Stickers(String),
    /// An engine message that the page shows as it is.
    Engine(String),
    Stopped,
}

impl WorkError {
    pub(super) fn message(&self) -> String {
        match self {
            Self::Engine(e) => e.clone(),
            Self::Open(e) => tr_format(&tr("Cannot open this book. %1"), &[e.as_str()]),
            Self::Turn(e) => tr_format(&tr("Cannot turn the page. %1"), &[e.as_str()]),
            Self::GoTo(e) => tr_format(&tr("Cannot go there. %1"), &[e.as_str()]),
            Self::Draw(e) => tr_format(&tr("Cannot draw this page. %1"), &[e.as_str()]),
            Self::Stickers(e) => {
                let detail = super::stickers::error_text(e);
                tr_format(&tr("Cannot load stickers. %1"), &[detail.as_str()])
            }
            Self::Stopped => tr("The book worker stopped."),
        }
    }
}

pub(super) type Reply = mpsc::Receiver<Result<Frame, WorkError>>;
pub(super) fn spawn(
    viewport: Size,
    task: impl FnOnce() -> Result<(BookSession, Option<Selection>, Option<WorkError>), WorkError> + Send + 'static,
) -> Reply {
    let (send, reply) = mpsc::sync_channel(1);
    submit(Box::new(move || {
        let result = task().map(|(mut session, selection, operation_error)| {
            let rendered = session.render_current(viewport);
            let mut error = rendered
                .as_ref()
                .err()
                .map(|e| WorkError::Draw(e.to_string()))
                .or(operation_error);
            let mut marks = Vec::new();
            if let Ok(list) = session.annotations_on_page(viewport) {
                for item in list {
                    if item.annotation.is_bookmark() {
                        continue;
                    }
                    let style = match item.annotation.drawer.as_deref().and_then(Drawer::parse) {
                        Some(Drawer::Underscore) => MarkStyle::Underline,
                        Some(Drawer::Strikeout) => MarkStyle::Strike,
                        Some(Drawer::Invert) => MarkStyle::Invert,
                        _ => MarkStyle::Box,
                    };
                    let color = mark_color(item.annotation.color.as_deref());
                    marks.extend(item.rects.iter().map(|rect| Mark {
                        rect: *rect,
                        style,
                        color,
                    }));
                    // KOReader draws one note mark, beside the first line of the highlight.
                    if item
                        .annotation
                        .note
                        .as_deref()
                        .is_some_and(|note| !note.is_empty())
                    {
                        if let Some(first) = item.rects.iter().min_by_key(|rect| rect.y) {
                            marks.push(Mark {
                                rect: *first,
                                style: MarkStyle::Note,
                                color: None,
                            });
                        }
                    }
                }
            }
            let bookmarked = session.is_bookmarked().unwrap_or(false);
            let mut hits = Vec::new();
            let mut links = Vec::new();
            let page = session.current().page;
            if let Ok(transform) = session.page_transform(viewport) {
                let dx = (viewport.w as i32 - transform.image.w as i32) / 2;
                let dy = (viewport.h as i32 - transform.image.h as i32) / 2;
                if let Ok(page_links) = session.links() {
                    links = page_links
                        .into_iter()
                        .map(|link| {
                            let mut rect = transform.to_screen(link.bbox);
                            rect.x += dx;
                            rect.y += dy;
                            (rect, link)
                        })
                        .collect();
                }
                for result in session.search_results().iter().filter(|r| r.page == page) {
                    hits.extend(result.rects.iter().map(|r| transform.to_screen(*r)));
                }
            }
            // A suspend may arrive before the app receives this frame.
            // Keep any save error on the session for the normal retry path.
            let _ = session.save();
            let (stickers, sticker_anchor) =
                match super::stickers::frame(&mut session, selection.as_ref(), viewport) {
                    Ok(frame) => frame,
                    Err(e) => {
                        error = Some(WorkError::Stickers(e));
                        (Vec::new(), None)
                    }
                };
            Frame {
                stickers,
                sticker_anchor,
                session,
                page: rendered.ok(),
                marks,
                bookmarked,
                hits,
                links,
                selection,
                error,
            }
        });
        let _ = send.send(result);
    }));
    reply
}

/// The page color of a highlight. None when the highlight has no color, or KOReader
/// does not know its name.
fn mark_color(name: Option<&str>) -> Option<Color> {
    let [r, g, b] = kaia_book::highlight_rgb(name?)?;
    Some(Color::rgb(r, g, b))
}

// The read-only sticker document stays on this thread between page draws.
fn submit(task: Box<dyn FnOnce() + Send>) {
    static WORKER: std::sync::OnceLock<mpsc::Sender<Box<dyn FnOnce() + Send>>> = std::sync::OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let (send, receive) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
        std::thread::spawn(move || {
            while let Ok(task) = receive.recv() {
                task();
            }
        });
        send
    });
    let _ = worker.send(task);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_document_work_does_not_block_its_caller() {
        let (release, wait) = mpsc::channel();
        let caller = std::thread::current().id();
        let reply = spawn(Size::new(10, 10), move || {
            assert_ne!(caller, std::thread::current().id());
            wait.recv().unwrap();
            Err(WorkError::Open("test document".into()))
        });
        assert!(matches!(reply.try_recv(), Err(mpsc::TryRecvError::Empty)));
        release.send(()).unwrap();
        assert!(reply
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .is_err());
    }

    #[test]
    fn a_mark_takes_the_color_of_its_highlight() {
        assert_eq!(mark_color(Some("yellow")), Some(Color::rgb(0xFF, 0xFF, 0x33)));
        assert_eq!(mark_color(Some("gray")), Some(Color::rgb(204, 204, 204)));
        assert_eq!(mark_color(Some("teal")), None, "KOReader has no teal");
        assert_eq!(mark_color(None), None, "a highlight without a color has none");
    }
}
