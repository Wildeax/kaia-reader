//! Blocking session calls run away from the app. The document stays on its engine thread.
use super::page::{Mark, MarkStyle};
use kaia_book::{BookSession, Drawer, Selection, Size};
use kaia_gfx::{Pixmap, Rect};
use std::sync::{mpsc, Arc};

pub(super) struct Frame {
    pub session: BookSession,
    pub page: Option<Arc<Pixmap>>,
    pub marks: Vec<Mark>,
    pub hits: Vec<Rect>,
    pub selection: Option<Selection>,
    pub error: Option<String>,
}
pub(super) type Reply = mpsc::Receiver<Result<Frame, String>>;
pub(super) fn spawn(
    viewport: Size,
    task: impl FnOnce() -> Result<(BookSession, Option<Selection>, Option<String>), String> + Send + 'static,
) -> Reply {
    let (send, reply) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = task().map(|(mut session, selection, operation_error)| {
            let rendered = session.render_current(viewport);
            let error = rendered
                .as_ref()
                .err()
                .map(|e| format!("Cannot draw this page. {e}"))
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
                        _ => MarkStyle::Box,
                    };
                    marks.extend(item.rects.iter().map(|rect| Mark { rect: *rect, style }));
                }
            }
            let mut hits = Vec::new();
            let page = session.current().page;
            if let Ok(transform) = session.page_transform(viewport) {
                for result in session.search_results().iter().filter(|r| r.page == page) {
                    hits.extend(result.rects.iter().map(|r| transform.to_screen(*r)));
                }
            }
            // A suspend may arrive before the app receives this frame.
            // Keep any save error on the session for the normal retry path.
            let _ = session.save();
            Frame {
                session,
                page: rendered.ok(),
                marks,
                hits,
                selection,
                error,
            }
        });
        let _ = send.send(result);
    });
    reply
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
            Err("test document".into())
        });
        assert!(matches!(reply.try_recv(), Err(mpsc::TryRecvError::Empty)));
        release.send(()).unwrap();
        assert!(reply
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .is_err());
    }
}
