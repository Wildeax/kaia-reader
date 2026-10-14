//! Lua reader calls use the same session as the native reader.
//!
//! Lua callbacks run on the app thread, so nothing here may open or lay out a
//! document. Page counts, the position, the TOC and the current xpointer come
//! from the native session. Page text, other xpointers and page sizes come from
//! a second document on its own thread ([`super::plugin_doc`]). The reader
//! attaches plugins only after that document is laid out and the current page
//! text is cached, so `onReaderReady` reads the book as it does in KOReader.
//!
//! An answer that is not cached costs one engine call on that thread. The app
//! thread waits up to [`ANSWER_WAIT`] for it, and only while the document is
//! laid out. Otherwise the host answers with the newest answer from an earlier
//! layout, or with what the native session knows: the current position for an
//! xpointer, the current page for a page number, empty text for page text, and
//! the screen for a page size. KOReader would block instead. A Lua error here
//! would stop the plugin, so these calls do not fail for a book that is still
//! laying out.
//!
//! Font sizes cross this boundary in KOReader units, as `configurable.font_size`
//! and `SetFontSize` use them. The session stores engine pixels. The host
//! converts with [`super::koreader_px`], the rule of the native settings path.
use super::plugin_doc::{Answer, PluginDoc, Query, ANSWER_WAIT};
use super::state::Core;
use kaia_book::{Drawer, Location, Pos, Selection, Size, TextLayout};
use kaia_events::{Key, Settings, Value};
use kaia_i18n::tr;
use kaia_lua::ReaderHost;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub(super) struct Host {
    core: Rc<RefCell<Core>>,
    file: String,
    cre: bool,
    is_new: bool,
    viewport: Size,
    document: RefCell<PluginDoc>,
    props: Value,
    toc: Vec<Value>,
    settings: Settings,
    settings_path: PathBuf,
    pages: u32,
    /// The last native position: a 1-based page and, for CREngine, its xpointer.
    position: RefCell<(u32, Option<String>)>,
}
impl Host {
    pub fn new(core: Rc<RefCell<Core>>, data: &std::path::Path) -> Result<Self, String> {
        let c = core.borrow();
        let s = c.session.as_ref().ok_or_else(|| tr("The reader is busy"))?;
        let file = s.path().to_string_lossy().to_string();
        let document = PluginDoc::spawn(
            s.path().to_owned(),
            s.uses_cre(),
            s.is_reflowable().then(|| s.layout().clone()),
            c.viewport,
        );
        let settings_path = data
            .join("plugin-books")
            .join(format!("{}.json", s.identity().partial_md5));
        std::fs::create_dir_all(settings_path.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut settings = Settings::open(&settings_path);
        settings.save(
            "partial_md5_checksum",
            Value::from(s.identity().partial_md5.clone()),
        );
        let props = Value::Table(
            [
                (
                    Key::from("display_title"),
                    Value::from(s.metadata().title.clone().unwrap_or_else(|| c.title.clone())),
                ),
                (Key::from("title"), Value::from(c.title.clone())),
                (
                    Key::from("authors"),
                    Value::from(s.metadata().author.clone().unwrap_or_default()),
                ),
                (Key::from("has_pages"), Value::from(!s.is_reflowable())),
                (
                    Key::from("language"),
                    s.metadata().language.clone().map(Value::from).unwrap_or_default(),
                ),
            ]
            .into(),
        );
        let toc = s
            .toc()
            .iter()
            .map(|t| {
                Value::Table(
                    [
                        (Key::from("title"), Value::from(t.title.clone())),
                        (Key::from("level"), Value::from(t.level as i64)),
                        (Key::from("page"), Value::from(t.page.unwrap_or(0) as i64 + 1)),
                    ]
                    .into(),
                )
            })
            .collect();
        let page = s.current().page + 1;
        let pages = s.page_count() as u32;
        let position = (page, engine_position(s));
        let cre = s.uses_cre();
        let is_new = c.book_is_new();
        let viewport = c.viewport;
        drop(c);
        Ok(Self {
            core,
            file,
            cre,
            is_new,
            viewport,
            document: RefCell::new(document),
            props,
            toc,
            settings,
            settings_path,
            pages,
            position: RefCell::new(position),
        })
    }

    /// Called from the reader's tick: follows the native layout, reads the
    /// thread's answers and asks for the current page's text ahead of time.
    pub fn tick(&self) {
        self.follow();
        let page = self.current_page();
        let mut doc = self.document.borrow_mut();
        doc.poll();
        doc.ask(Query::PageText(page.saturating_sub(1)));
    }

    /// True when the document is laid out for the native layout and the current
    /// page's text is cached. The reader attaches plugins then.
    pub fn ready(&self) -> bool {
        let page = self.current_page();
        let doc = self.document.borrow();
        doc.idle() && doc.has(&Query::PageText(page.saturating_sub(1)))
    }

    /// True while the plugin document opens or lays out. That work holds the
    /// engine lock, so the reader avoids engine calls from the app thread then.
    pub fn laying_out(&self) -> bool {
        !self.document.borrow().idle()
    }

    /// Why plugins cannot read this book, if they cannot.
    pub fn failure(&self) -> Option<String> {
        self.document.borrow().failure()
    }

    /// The layout error of the plugin document, if its last layout failed.
    pub fn layout_error(&self) -> Option<String> {
        self.document.borrow().layout_error().map(str::to_owned)
    }

    /// Sends the native layout to the plugin document when it changed. Waits
    /// for the native layout to finish first: both use the engine lock, and the
    /// reader's own page comes first.
    fn follow(&self) {
        let c = self.core.borrow();
        if c.layout_pending || c.job.is_some() {
            return;
        }
        let Some(s) = c.session.as_ref() else {
            return;
        };
        *self.position.borrow_mut() = (s.current().page + 1, engine_position(s));
        let layout = s.is_reflowable().then(|| s.layout().clone());
        self.document.borrow_mut().follow(layout, c.viewport);
    }

    /// The answer to `query`: cached, or computed within [`ANSWER_WAIT`], or the
    /// newest answer from an earlier layout.
    fn fetch(&self, query: Query) -> Option<Result<Answer, String>> {
        self.follow();
        let mut doc = self.document.borrow_mut();
        doc.answer(&query, ANSWER_WAIT)
            .or_else(|| doc.stale(&query).map(Ok))
    }

    fn refresh_annotations(&self) {
        self.core
            .borrow_mut()
            .queued
            .push_back(("reader:plugin-refresh".into(), vec![]));
    }

    fn screen(&self) -> kaia_gfx::Size {
        self.viewport
    }

    /// The session's font size in KOReader units.
    fn font_units(&self, layout: &TextLayout) -> f64 {
        koreader_units(self.screen(), layout.font_size)
    }
}

/// The xpointer of the session's position, for CREngine books.
fn engine_position(s: &kaia_book::BookSession) -> Option<String> {
    match s.current().location {
        Location::Engine(xp) if s.uses_cre() => Some(xp),
        _ => None,
    }
}

/// KOReader units for `px` engine pixels: the inverse of [`super::koreader_px`].
/// That function rounds up, so several unit values give the same pixels. This
/// picks a whole number of units when one fits, then a half, then a tenth, as
/// KOReader stores sizes such as 22 or 22.5. The result always maps back to `px`.
pub(super) fn koreader_units(screen: kaia_gfx::Size, px: f32) -> f64 {
    let scale = f64::from(screen.w.min(screen.h).max(1)) / 600.0;
    let exact = f64::from(px) / scale;
    for steps in [1.0, 2.0, 10.0, 100.0] {
        let scaled = exact * steps;
        for candidate in [scaled.round(), scaled.floor(), scaled.ceil()] {
            let units = candidate / steps;
            if super::koreader_px(screen, units) == px {
                return units;
            }
        }
    }
    exact
}
impl ReaderHost for Host {
    fn file(&self) -> String {
        self.file.clone()
    }
    fn current_page(&self) -> u32 {
        self.core
            .borrow()
            .session
            .as_ref()
            .map_or(self.position.borrow().0, |s| s.current().page + 1)
    }
    fn page_count(&self) -> u32 {
        self.core
            .borrow()
            .session
            .as_ref()
            .map_or(self.pages, |s| s.page_count() as u32)
    }
    fn properties(&self) -> Value {
        self.props.clone()
    }
    fn toc(&self) -> Vec<Value> {
        self.toc.clone()
    }
    fn go_to_page(&mut self, page: u32) -> Result<(), String> {
        self.core.borrow_mut().queued.push_back((
            "reader:location".into(),
            vec![Value::from(page.saturating_sub(1) as i64)],
        ));
        Ok(())
    }
    fn page_text(&mut self, page: u32) -> Result<String, String> {
        match self.fetch(Query::PageText(page.saturating_sub(1))) {
            Some(Ok(Answer::Text(text))) => Ok(text),
            Some(Err(e)) => Err(e),
            // Not laid out yet: no text rather than an error that stops the plugin.
            _ => Ok(String::new()),
        }
    }
    fn xpointer_for_page(&self, page: u32) -> Result<String, String> {
        if !self.cre {
            return Err(tr("This book uses page numbers"));
        }
        self.follow();
        let (current, position) = self.position.borrow().clone();
        // KOReader's getXPointer is the reading position, which the session has.
        if page == current {
            if let Some(xp) = position {
                return Ok(xp);
            }
        }
        match self.fetch(Query::Xpointer(page.saturating_sub(1))) {
            Some(Ok(Answer::Text(xp))) => Ok(xp),
            Some(Err(e)) => Err(e),
            _ => position.ok_or_else(|| tr("The reader is busy")),
        }
    }
    fn page_for_xpointer(&self, xp: &str) -> Result<u32, String> {
        self.follow();
        let (current, position) = self.position.borrow().clone();
        if position.as_deref() == Some(xp) {
            return Ok(current);
        }
        match self.fetch(Query::PageFor(xp.to_owned())) {
            Some(Ok(Answer::Page(page))) => Ok(page + 1),
            Some(Err(e)) => Err(e),
            _ => Ok(current),
        }
    }
    fn native_page_dimensions(&self, page: u32) -> Result<Value, String> {
        let (width, height) = match self.fetch(Query::Dimensions(page.saturating_sub(1))) {
            Some(Ok(Answer::Size(w, h))) => (w, h),
            Some(Err(e)) => return Err(e),
            // A reflowable page has the size of the screen.
            _ => (self.viewport.w as f32, self.viewport.h as f32),
        };
        Ok(Value::Table(
            [
                (Key::from("w"), Value::from(width as f64)),
                (Key::from("h"), Value::from(height as f64)),
            ]
            .into(),
        ))
    }
    fn page_margins(&self, _: u32) -> Result<Value, String> {
        let c = self.core.borrow();
        let m = &c
            .session
            .as_ref()
            .ok_or_else(|| tr("The reader is busy"))?
            .layout()
            .margins;
        Ok(Value::Table(
            [
                (Key::from("left"), Value::from(m.left as f64)),
                (Key::from("right"), Value::from(m.right as f64)),
                (Key::from("top"), Value::from(m.top as f64)),
                (Key::from("bottom"), Value::from(m.bottom as f64)),
            ]
            .into(),
        ))
    }
    fn document_query(&mut self, method: &str, args: Vec<String>) -> Result<Value, String> {
        let query = Query::Compat(method.into(), args, self.current_page().saturating_sub(1));
        let result = self
            .document
            .borrow_mut()
            .answer(&query, std::time::Duration::from_secs(30));
        let Some(result) = result else {
            return Err("plugin text query is not ready".into());
        };
        match result? {
            Answer::Value(mut value) => {
                if method == "getScreenBoxesFromPositions" {
                    let offset = self.core.borrow().header_height;
                    if let Value::Table(boxes) = &mut value {
                        for b in boxes.values_mut() {
                            let y = b.get("y").as_f64().unwrap_or(0.0) + offset as f64;
                            if let Value::Table(fields) = b {
                                fields.insert(Key::from("y"), Value::from(y));
                            }
                        }
                    }
                }
                Ok(value)
            }
            _ => Err("invalid plugin text answer".into()),
        }
    }
    fn text_from_positions(&mut self, start: &Value, end: &Value) -> Result<String, String> {
        let c = self.core.borrow();
        let selection = c
            .selection
            .as_ref()
            .ok_or_else(|| tr("There is no active selection"))?;
        if position_value(selection.pos0.clone()) != *start || position_value(selection.pos1.clone()) != *end
        {
            return Err(tr("The requested positions are outside the active selection"));
        }
        Ok(selection.text.clone())
    }
    fn boxes_from_positions(&mut self, start: &Value, end: &Value) -> Result<Vec<Value>, String> {
        self.text_from_positions(start, end)?;
        let c = self.core.borrow();
        Ok(c.selection
            .as_ref()
            .unwrap()
            .screen_rects
            .iter()
            .map(|r| {
                Value::Table(
                    [
                        (Key::from("x"), Value::from(r.x as i64)),
                        (Key::from("y"), Value::from(r.y as i64)),
                        (Key::from("w"), Value::from(r.w as i64)),
                        (Key::from("h"), Value::from(r.h as i64)),
                    ]
                    .into(),
                )
            })
            .collect())
    }
    fn annotations(&mut self) -> Vec<Value> {
        self.core
            .borrow_mut()
            .session
            .as_mut()
            .and_then(|s| s.annotations().ok())
            .unwrap_or_default()
            .into_iter()
            .map(|a| a.to_value())
            .collect()
    }
    fn add_annotation(&mut self, item: Value) -> Result<u64, String> {
        let position = |key| -> Result<Pos, String> {
            let v = item.get(key);
            if let Some(xp) = v.as_str() {
                return Ok(Pos::Xpointer(xp.into()));
            }
            Ok(Pos::Point {
                page: v.get("page").as_i64().filter(|p| *p > 0).ok_or("missing page")? as u32 - 1,
                x: v.get("x").as_f64().ok_or("missing x")? as f32,
                y: v.get("y").as_f64().ok_or("missing y")? as f32,
            })
        };
        let selection = Selection {
            page: self.current_page() - 1,
            text: item.get("text").as_str().ok_or("missing text")?.into(),
            pos0: position("pos0")?,
            pos1: position("pos1")?,
            page_rects: vec![],
            screen_rects: vec![],
        };
        let id = self
            .core
            .borrow_mut()
            .session
            .as_mut()
            .ok_or_else(|| tr("The reader is busy"))?
            .add_highlight(
                &selection,
                Drawer::parse(item.get("drawer").as_str().unwrap_or("lighten")).ok_or("invalid drawer")?,
                item.get("color").as_str().unwrap_or("yellow"),
                item.get("note").as_str(),
            )
            .map_err(|e| e.to_string())?;
        self.refresh_annotations();
        Ok(id)
    }
    fn update_annotation(&mut self, id: u64, item: Value) -> Result<(), String> {
        self.core
            .borrow_mut()
            .session
            .as_mut()
            .ok_or_else(|| tr("The reader is busy"))?
            .edit_note(id, item.get("note").as_str())
            .map_err(|e| e.to_string())?;
        if let Some(drawer) = item.get("drawer").as_str() {
            self.core
                .borrow_mut()
                .session
                .as_mut()
                .ok_or_else(|| tr("The reader is busy"))?
                .set_highlight_style(
                    id,
                    Drawer::parse(drawer).ok_or("invalid drawer")?,
                    item.get("color").as_str().unwrap_or("yellow"),
                )
                .map_err(|e| e.to_string())?;
        }
        self.refresh_annotations();
        Ok(())
    }
    fn replace_annotations(&mut self, items: Vec<Value>) -> Result<(), String> {
        self.core
            .borrow_mut()
            .session
            .as_mut()
            .ok_or_else(|| tr("The reader is busy"))?
            .replace_plugin_annotations(&items)
            .map_err(|e| e.to_string())?;
        self.refresh_annotations();
        Ok(())
    }
    fn delete_annotation(&mut self, id: u64) -> Result<(), String> {
        self.core
            .borrow_mut()
            .session
            .as_mut()
            .ok_or_else(|| tr("The reader is busy"))?
            .delete_annotation(id)
            .map_err(|e| e.to_string())?;
        self.refresh_annotations();
        Ok(())
    }
    fn flush_settings(&mut self) -> Result<(), String> {
        self.settings
            .flush(&self.settings_path)
            .map_err(|e| e.to_string())?;
        self.core
            .borrow_mut()
            .session
            .as_mut()
            .ok_or_else(|| tr("The reader is busy"))?
            .save()
            .map_err(|e| e.to_string())
    }

    fn is_new(&self) -> bool {
        self.is_new
    }

    fn get_setting(&self, key: &str) -> Value {
        let c = self.core.borrow();
        if let Some(s) = c.session.as_ref() {
            match key {
                "font_size" => return Value::from(self.font_units(s.layout())),
                "font_face" => {
                    return s
                        .layout()
                        .font_family
                        .clone()
                        .map(Value::from)
                        .unwrap_or_default()
                }
                "configurable" => {
                    return Value::Table(
                        [(Key::from("font_size"), Value::from(self.font_units(s.layout())))].into(),
                    )
                }
                _ => {}
            }
        }
        self.settings.read(key).clone()
    }
    fn set_setting(&mut self, key: &str, value: Value) -> Result<(), String> {
        if key == "font_size" || key == "font_face" {
            let screen = self.screen();
            let mut c = self.core.borrow_mut();
            c.change_layout(|l| {
                if key == "font_size" {
                    // KOReader units, scaled as ReaderFont scales them for the engine.
                    if let Some(units) = value.as_f64().filter(|v| v.is_finite() && *v > 0.0) {
                        l.font_size = super::koreader_px(screen, units).clamp(8.0, 200.0);
                    }
                } else {
                    l.font_family = value.as_str().map(str::to_owned);
                }
            })?;
        }
        self.settings.save(key, value);
        self.settings
            .flush(&self.settings_path)
            .map_err(|e| e.to_string())
    }
}
fn position_value(pos: Pos) -> Value {
    match pos {
        Pos::Xpointer(xp) => Value::from(xp),
        Pos::Point { page, x, y } => Value::Table(
            [
                (Key::from("page"), Value::from(page as i64 + 1)),
                (Key::from("x"), Value::from(x as f64)),
                (Key::from("y"), Value::from(y as f64)),
                (Key::from("zoom"), Value::from(1i64)),
                (Key::from("rotation"), Value::from(0i64)),
            ]
            .into(),
        ),
    }
}
