//! The plugin host's own copy of the book, on its own thread.
//!
//! KOReader plugins ask for page text, xpointers and page sizes that
//! [`kaia_book::BookSession`] does not expose. A second document answers them.
//! That document opens and lays out on the thread this module starts, never on
//! the app thread. The app thread sends layouts and questions and reads the
//! answers from a cache.
//!
//! Each layout has a generation. Answers belong to the generation they were
//! computed for. When the layout changes, the answers move to a stale table,
//! which the host offers as a fallback until the new layout is ready.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use kaia_book::{Location, Size, TextLayout};
use kaia_doc::Document;

/// The longest time the app thread waits for an answer that is not cached.
/// The host only waits while the document is laid out, so a wait is one short
/// engine call and not a layout.
pub(super) const ANSWER_WAIT: Duration = Duration::from_millis(20);

/// A question for the plugin document. Pages are 0-based here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Query {
    PageText(u32),
    Xpointer(u32),
    PageFor(String),
    Dimensions(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Answer {
    Text(String),
    /// A 0-based page.
    Page(u32),
    Size(f32, f32),
}

enum Job {
    /// A generation, the reflowable layout (None for fixed-layout books) and the viewport.
    Layout(u64, Option<TextLayout>, Size),
    Ask(u64, Query),
}

enum Note {
    Opened(Result<(), String>),
    LaidOut(u64, Result<(), String>),
    Answer(u64, Query, Result<Answer, String>),
}

/// The app thread's handle on the plugin document.
pub(super) struct PluginDoc {
    jobs: mpsc::Sender<Job>,
    notes: mpsc::Receiver<Note>,
    opened: Option<Result<(), String>>,
    stopped: bool,
    generation: u64,
    sent: (Option<TextLayout>, Size),
    laid_out: Option<u64>,
    layout_error: Option<String>,
    answers: HashMap<Query, Result<Answer, String>>,
    stale: HashMap<Query, Answer>,
    asked: HashSet<Query>,
}

impl PluginDoc {
    /// Starts the thread, which opens `path` and applies `layout` for `viewport`.
    pub fn spawn(path: PathBuf, cre: bool, layout: Option<TextLayout>, viewport: Size) -> Self {
        let (jobs, job_queue) = mpsc::channel();
        let (note, notes) = mpsc::channel();
        std::thread::Builder::new()
            .name("kaia-plugin-doc".into())
            .spawn(move || run(path, cre, job_queue, note))
            .map(|_| ())
            .unwrap_or_else(|e| eprintln!("kaia: plugin document thread: {e}"));
        let _ = jobs.send(Job::Layout(1, layout.clone(), viewport));
        Self {
            jobs,
            notes,
            opened: None,
            stopped: false,
            generation: 1,
            sent: (layout, viewport),
            laid_out: None,
            layout_error: None,
            answers: HashMap::new(),
            stale: HashMap::new(),
            asked: HashSet::new(),
        }
    }

    /// Sends a new layout when it differs from the last one sent. The answers
    /// so far become stale.
    pub fn follow(&mut self, layout: Option<TextLayout>, viewport: Size) {
        if self.sent.0 == layout && self.sent.1 == viewport {
            return;
        }
        self.generation += 1;
        for (query, answer) in self.answers.drain() {
            if let Ok(answer) = answer {
                self.stale.insert(query, answer);
            }
        }
        self.asked.clear();
        self.layout_error = None;
        self.sent = (layout.clone(), viewport);
        if self
            .jobs
            .send(Job::Layout(self.generation, layout, viewport))
            .is_err()
        {
            self.stopped = true;
        }
    }

    /// Reads the notes the thread has sent so far.
    pub fn poll(&mut self) {
        loop {
            match self.notes.try_recv() {
                Ok(note) => self.take(note),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.stopped = true;
                    return;
                }
            }
        }
    }

    fn take(&mut self, note: Note) {
        match note {
            Note::Opened(result) => self.opened = Some(result),
            Note::LaidOut(generation, result) if generation == self.generation => {
                self.laid_out = Some(generation);
                self.layout_error = result.err();
            }
            Note::Answer(generation, query, result) if generation == self.generation => {
                self.asked.remove(&query);
                if let Ok(answer) = &result {
                    self.stale.insert(query.clone(), answer.clone());
                }
                self.answers.insert(query, result);
            }
            _ => {}
        }
    }

    /// Why the document cannot answer at all: it did not open, or its thread stopped.
    pub fn failure(&self) -> Option<String> {
        match &self.opened {
            Some(Err(e)) => Some(e.clone()),
            _ if self.stopped => Some("the plugin document thread stopped".into()),
            _ => None,
        }
    }

    /// True when the document is open and laid out for the last layout sent.
    /// The thread then holds no long engine work.
    pub fn idle(&self) -> bool {
        matches!(self.opened, Some(Ok(()))) && self.laid_out == Some(self.generation)
    }

    /// The error of the last layout, if it failed.
    pub fn layout_error(&self) -> Option<&str> {
        self.layout_error.as_deref()
    }

    /// True when `query` has an answer for the current layout.
    pub fn has(&self, query: &Query) -> bool {
        self.answers.contains_key(query)
    }

    /// Asks the thread for `query` unless it is answered or asked already.
    pub fn ask(&mut self, query: Query) {
        if self.answers.contains_key(&query) || self.asked.contains(&query) {
            return;
        }
        if self.jobs.send(Job::Ask(self.generation, query.clone())).is_err() {
            self.stopped = true;
            return;
        }
        self.asked.insert(query);
    }

    /// The answer for the current layout. When there is none and the document is
    /// laid out, waits up to `wait` for it. While the document opens or lays
    /// out, it does not wait: the answer would come after the layout.
    pub fn answer(&mut self, query: &Query, wait: Duration) -> Option<Result<Answer, String>> {
        self.poll();
        if let Some(answer) = self.answers.get(query) {
            return Some(answer.clone());
        }
        self.ask(query.clone());
        if !self.idle() {
            return None;
        }
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            match self.notes.recv_timeout(left) {
                Ok(note) => self.take(note),
                Err(mpsc::RecvTimeoutError::Timeout) => return None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.stopped = true;
                    return None;
                }
            }
            if let Some(answer) = self.answers.get(query) {
                return Some(answer.clone());
            }
        }
    }

    /// The newest answer to `query` from any layout.
    pub fn stale(&self, query: &Query) -> Option<Answer> {
        self.stale.get(query).cloned()
    }
}

enum TextDocument {
    Cre(kaia_doc::cre::CreDocument),
    Other(Box<dyn Document>),
}

impl TextDocument {
    fn doc(&self) -> &dyn Document {
        match self {
            Self::Cre(doc) => doc,
            Self::Other(doc) => doc.as_ref(),
        }
    }

    fn apply(&mut self, l: &TextLayout, viewport: Size) -> Result<(), String> {
        let err = |e: kaia_doc::DocError| e.to_string();
        if let Self::Cre(cre) = self {
            cre.set_hyphenation(l.hyphenation.then_some(kaia_book::layout::HYPHENATION_DICT))
                .map_err(err)?;
            cre.set_embedded_styles(l.embedded_styles).map_err(err)?;
        }
        let layout = kaia_doc::Layout {
            width: viewport.w as f32,
            height: viewport.h as f32,
            em: l.font_size,
            margins: Some(l.margins),
            line_height: Some(l.line_spacing),
            font_family: l.font_family.clone(),
            css_extra: if l.justify {
                "body { text-align: justify !important; }\n".into()
            } else {
                String::new()
            },
        };
        match self {
            Self::Cre(doc) => doc.set_layout(&layout),
            Self::Other(doc) => doc.set_layout(&layout),
        }
        .map_err(err)
    }

    fn answer(&self, query: &Query) -> Result<Answer, String> {
        let err = |e: kaia_doc::DocError| e.to_string();
        match query {
            Query::PageText(page) => self
                .doc()
                .page_text(*page)
                .map(|p| Answer::Text(p.text))
                .map_err(err),
            Query::Xpointer(page) => match self {
                Self::Cre(doc) => doc.page_xpointer(*page).map(Answer::Text).map_err(err),
                Self::Other(_) => Err("this book uses page numbers".into()),
            },
            Query::PageFor(xp) => self
                .doc()
                .resolve(&Location::Engine(xp.clone()))
                .map(Answer::Page)
                .map_err(err),
            Query::Dimensions(page) => self
                .doc()
                .page_size(*page)
                .map(|s| Answer::Size(s.width, s.height))
                .map_err(err),
        }
    }
}

fn run(path: PathBuf, cre: bool, jobs: mpsc::Receiver<Job>, notes: mpsc::Sender<Note>) {
    let opened = if cre {
        kaia_doc::cre::CreDocument::open(&path).map(TextDocument::Cre)
    } else {
        kaia_doc::open(&path).map(TextDocument::Other)
    };
    let mut doc = match opened {
        Ok(doc) => {
            let _ = notes.send(Note::Opened(Ok(())));
            doc
        }
        Err(e) => {
            let _ = notes.send(Note::Opened(Err(e.to_string())));
            return;
        }
    };
    let mut generation = 0;
    let mut queue = VecDeque::new();
    loop {
        if queue.is_empty() {
            match jobs.recv() {
                Ok(job) => queue.push_back(job),
                Err(_) => return,
            }
        }
        while let Ok(job) = jobs.try_recv() {
            queue.push_back(job);
        }
        // Only the newest layout counts. The questions before it were asked for
        // an older layout, so they go too.
        if let Some(last) = queue.iter().rposition(|j| matches!(j, Job::Layout(..))) {
            if let Some(Job::Layout(next, layout, viewport)) = queue.drain(..=last).next_back() {
                let result = match &layout {
                    Some(layout) => doc.apply(layout, viewport),
                    None => Ok(()),
                };
                generation = next;
                if notes.send(Note::LaidOut(next, result)).is_err() {
                    return;
                }
            }
            continue;
        }
        if let Some(Job::Ask(asked, query)) = queue.pop_front() {
            if asked == generation {
                let result = doc.answer(&query);
                if notes.send(Note::Answer(asked, query, result)).is_err() {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/juliet.epub")
    }

    fn wait_idle(doc: &mut PluginDoc) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !doc.idle() {
            assert!(doc.failure().is_none(), "{:?}", doc.failure());
            assert!(Instant::now() < deadline, "the plugin document did not lay out");
            doc.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn answers_come_from_the_thread_and_layouts_make_them_stale() {
        let viewport = Size::new(600, 800);
        let layout = TextLayout::default();
        let mut doc = PluginDoc::spawn(book(), true, Some(layout.clone()), viewport);
        // Opening and layout run on the thread, so the first call does not wait.
        let start = Instant::now();
        assert!(doc.answer(&Query::Xpointer(2), ANSWER_WAIT).is_none() || doc.idle());
        assert!(start.elapsed() < Duration::from_millis(100));
        wait_idle(&mut doc);
        let xp = match doc.answer(&Query::Xpointer(2), Duration::from_secs(10)) {
            Some(Ok(Answer::Text(xp))) => xp,
            other => panic!("no xpointer: {other:?}"),
        };
        assert!(xp.starts_with('/'), "{xp}");
        assert_eq!(
            doc.answer(&Query::PageFor(xp.clone()), Duration::from_secs(10)),
            Some(Ok(Answer::Page(2)))
        );
        let bigger = TextLayout {
            font_size: layout.font_size * 2.0,
            ..layout
        };
        doc.follow(Some(bigger), viewport);
        assert!(!doc.has(&Query::Xpointer(2)));
        assert_eq!(doc.stale(&Query::Xpointer(2)), Some(Answer::Text(xp.clone())));
        wait_idle(&mut doc);
        match doc.answer(&Query::PageFor(xp), Duration::from_secs(10)) {
            Some(Ok(Answer::Page(page))) => assert!(page > 2, "{page}"),
            other => panic!("no page: {other:?}"),
        }
    }
}
