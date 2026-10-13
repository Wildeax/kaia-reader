//! Only painted cells ask the cover worker for decoded images.
use kaia_gfx::{Pixmap, Size};
use kaia_library::{CoverSize, Library};
use kaia_ui::geometry::Constraints;
use kaia_ui::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use kaia_ui::WidgetPod;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::{mpsc, Arc};

const CACHE_BYTES: usize = 8 * 1024 * 1024;
type Image = Option<Arc<Pixmap>>;
type Entry = (i64, Image);
thread_local! { static LOADERS: RefCell<Vec<Weak<RefCell<Loader>>>> = const { RefCell::new(Vec::new()) }; }
pub type Shared = Rc<RefCell<Loader>>;
pub struct Loader {
    requests: mpsc::Sender<(i64, std::sync::Weak<()>)>,
    replies: mpsc::Receiver<(i64, Option<Image>)>,
    cache: VecDeque<Entry>,
    pending: HashSet<i64>,
    bytes: usize,
}
impl Loader {
    pub fn new(path: PathBuf) -> Shared {
        let (tx, rx) = mpsc::channel::<(i64, std::sync::Weak<()>)>();
        let (done, replies) = mpsc::sync_channel(2);
        std::thread::spawn(move || {
            let library = Library::open(&path).ok();
            while let Ok((id, visible)) = rx.recv() {
                let picture = if visible.upgrade().is_some() {
                    Some(
                        library
                            .as_ref()
                            .and_then(|lib| crate::surfaces::library::data::cover(lib, id, CoverSize::Small))
                            .map(Arc::new),
                    )
                } else {
                    None
                };
                if done.send((id, picture)).is_err() {
                    break;
                }
            }
        });
        let loader = Rc::new(RefCell::new(Self {
            requests: tx,
            replies,
            cache: VecDeque::new(),
            pending: HashSet::new(),
            bytes: 0,
        }));
        LOADERS.with_borrow_mut(|all| all.push(Rc::downgrade(&loader)));
        loader
    }
    fn get(&mut self, id: i64) -> Option<Image> {
        let index = self.cache.iter().position(|entry| entry.0 == id)?;
        let entry = self.cache.remove(index)?;
        let image = entry.1.clone();
        self.cache.push_back(entry);
        Some(image)
    }
    fn insert(&mut self, mut entry: Entry) {
        if entry.1.as_ref().is_some_and(|p| p.data().len() > CACHE_BYTES) {
            entry.1 = None;
        }
        let size = entry.1.as_ref().map_or(0, |p| p.data().len());
        while self.bytes + size > CACHE_BYTES || self.cache.len() >= 256 {
            let Some((_, old)) = self.cache.pop_front() else {
                break;
            };
            self.bytes -= old.as_ref().map_or(0, |p| p.data().len());
        }
        if size <= CACHE_BYTES {
            self.bytes += size;
            self.cache.push_back(entry);
        }
    }
}
pub fn busy() -> bool {
    LOADERS.with_borrow(|all| {
        all.iter()
            .filter_map(Weak::upgrade)
            .any(|l| !l.borrow().pending.is_empty())
    })
}
pub fn poll() -> bool {
    let mut changed = false;
    LOADERS.with_borrow_mut(|all| {
        all.retain(|weak| {
            let Some(loader) = weak.upgrade() else {
                return false;
            };
            let mut loader = loader.borrow_mut();
            while let Ok(entry) = loader.replies.try_recv() {
                loader.pending.remove(&entry.0);
                if let Some(image) = entry.1 {
                    loader.insert((entry.0, image));
                }
                changed = true;
            }
            true
        });
    });
    changed
}

pub struct Cell {
    id: i64,
    loader: Shared,
    visible: Arc<()>,
    child: WidgetPod,
    build: Box<dyn Fn(Option<Pixmap>) -> WidgetPod>,
    ready: bool,
}
impl Cell {
    pub fn new(id: i64, loader: Shared, build: impl Fn(Option<Pixmap>) -> WidgetPod + 'static) -> Self {
        Self {
            id,
            loader,
            visible: Arc::new(()),
            child: build(None),
            build: Box::new(build),
            ready: false,
        }
    }
}
impl Widget for Cell {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        if !self.ready {
            if let Some(image) = self.loader.borrow_mut().get(self.id) {
                self.ready = true;
                if let Some(image) = image {
                    self.child = (self.build)(Some((*image).clone()));
                }
            }
        }
        self.child.layout(cx, c)
    }
    fn paint(&mut self, _cx: &mut PaintCx) {
        if self.ready {
            return;
        }
        let mut loader = self.loader.borrow_mut();
        if loader.get(self.id).is_some() {
            return;
        }
        if loader.pending.insert(self.id) {
            let _ = loader.requests.send((self.id, Arc::downgrade(&self.visible)));
        }
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if matches!(ev, UiEvent::Named(e) if e.name == "CoversReady") && !self.ready {
            let image = self.loader.borrow_mut().get(self.id);
            if let Some(image) = image {
                self.ready = true;
                if let Some(image) = image {
                    self.child = (self.build)(Some((*image).clone()));
                    cx.request_layout();
                    cx.request_paint();
                }
            } else if !self.loader.borrow().pending.contains(&self.id) {
                cx.request_paint();
            }
        }
        Handled::No
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.child]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoded_cache_evicts_by_bytes() {
        let loader = Loader::new(std::env::temp_dir().join("kaia-cover-cache-test.sqlite3"));
        let mut loader = loader.borrow_mut();
        for id in 0..30 {
            loader.insert((
                id,
                Some(Arc::new(Pixmap::new(512, 512, kaia_gfx::PixelFormat::Rgb24))),
            ));
            assert!(loader.bytes <= CACHE_BYTES);
        }
        assert!(loader.get(0).is_none());
        assert!(loader.get(29).is_some());
    }
}
