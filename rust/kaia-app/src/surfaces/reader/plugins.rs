//! Lua reader calls use the same session as the native reader.
use super::state::Core;
use kaia_book::{Drawer, Location, Pos, Selection};
use kaia_doc::Document;
use kaia_events::{Key, Settings, Value};
use kaia_i18n::tr;
use kaia_lua::ReaderHost;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub(super) struct Host {
    core: Rc<RefCell<Core>>,
    file: String,
    document: RefCell<TextDocument>,
    text_layout: RefCell<Option<kaia_book::TextLayout>>,
    props: Value,
    toc: Vec<Value>,
    settings: Settings,
    settings_path: PathBuf,
    page: u32,
    pages: u32,
}
impl Host {
    pub fn new(core: Rc<RefCell<Core>>, data: &std::path::Path) -> Result<Self, String> {
        let c = core.borrow();
        let s = c.session.as_ref().ok_or_else(|| tr("The reader is busy"))?;
        let file = s.path().to_string_lossy().to_string();
        let document = if s.uses_cre() {
            TextDocument::Cre(kaia_doc::cre::CreDocument::open(s.path()).map_err(|e| e.to_string())?)
        } else {
            TextDocument::Other(kaia_doc::open(s.path()).map_err(|e| e.to_string())?)
        };
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
        drop(c);
        Ok(Self {
            core,
            file,
            document: RefCell::new(document),
            text_layout: RefCell::new(None),
            props,
            toc,
            settings,
            settings_path,
            page,
            pages,
        })
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
    fn doc_mut(&mut self) -> &mut dyn Document {
        match self {
            Self::Cre(doc) => doc,
            Self::Other(doc) => doc.as_mut(),
        }
    }
}
impl Host {
    fn refresh_annotations(&self) {
        self.core
            .borrow_mut()
            .queued
            .push_back(("reader:plugin-refresh".into(), vec![]));
    }

    // BookSession owns navigation. This read-only document supplies text methods
    // that its public API does not expose, with the same engine and layout.
    fn sync_text_layout(&self) -> Result<(), String> {
        let c = self.core.borrow();
        let s = c.session.as_ref().ok_or_else(|| tr("The reader is busy"))?;
        if !s.is_reflowable() || self.text_layout.borrow().as_ref() == Some(s.layout()) {
            return Ok(());
        }
        let l = s.layout();
        let mut doc = self.document.borrow_mut();
        if let TextDocument::Cre(cre) = &mut *doc {
            cre.set_hyphenation(l.hyphenation.then_some("English_US.pattern"))
                .map_err(|e| e.to_string())?;
            cre.set_embedded_styles(l.embedded_styles)
                .map_err(|e| e.to_string())?;
        }
        doc.doc_mut()
            .set_layout(&kaia_doc::Layout {
                width: c.viewport.w as f32,
                height: c.viewport.h as f32,
                em: l.font_size,
                margins: Some(l.margins),
                line_height: Some(l.line_spacing),
                font_family: l.font_family.clone(),
                css_extra: if l.justify {
                    "body { text-align: justify !important; }\n".into()
                } else {
                    String::new()
                },
            })
            .map_err(|e| e.to_string())?;
        *self.text_layout.borrow_mut() = Some(l.clone());
        Ok(())
    }
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
            .map_or(self.page, |s| s.current().page + 1)
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
        self.sync_text_layout()?;
        self.document
            .borrow()
            .doc()
            .page_text(page.saturating_sub(1))
            .map(|p| p.text)
            .map_err(|e| e.to_string())
    }
    fn xpointer_for_page(&self, page: u32) -> Result<String, String> {
        self.sync_text_layout()?;
        match &*self.document.borrow() {
            TextDocument::Cre(doc) => doc
                .page_xpointer(page.saturating_sub(1))
                .map_err(|e| e.to_string()),
            _ => Err(tr("This book uses page numbers")),
        }
    }
    fn page_for_xpointer(&self, xp: &str) -> Result<u32, String> {
        self.sync_text_layout()?;
        self.document
            .borrow()
            .doc()
            .resolve(&Location::Engine(xp.into()))
            .map(|p| p + 1)
            .map_err(|e| e.to_string())
    }
    fn native_page_dimensions(&self, page: u32) -> Result<Value, String> {
        self.sync_text_layout()?;
        let size = self
            .document
            .borrow()
            .doc()
            .page_size(page.saturating_sub(1))
            .map_err(|e| e.to_string())?;
        Ok(Value::Table(
            [
                (Key::from("w"), Value::from(size.width as f64)),
                (Key::from("h"), Value::from(size.height as f64)),
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
            .map(|a| {
                let mut fields = kaia_events::Table::new();
                fields.insert(Key::from("kaia_id"), Value::from(a.id as i64));
                fields.insert(Key::from("pageno"), Value::from(a.pageno.unwrap_or(1) as i64));
                if let Some(color) = a.color {
                    fields.insert(Key::from("color"), Value::from(color));
                }
                fields.insert(Key::from("datetime"), Value::from(a.datetime));
                if let Some(text) = a.text {
                    fields.insert(Key::from("text"), Value::from(text));
                }
                if let Some(note) = a.note {
                    fields.insert(Key::from("note"), Value::from(note));
                }
                if let Some(chapter) = a.chapter {
                    fields.insert(Key::from("chapter"), Value::from(chapter));
                }
                if let Some(drawer) = a.drawer {
                    fields.insert(Key::from("drawer"), Value::from(drawer));
                }
                if let Some(pos) = a.pos0 {
                    fields.insert(Key::from("pos0"), position_value(pos));
                }
                if let Some(pos) = a.pos1 {
                    fields.insert(Key::from("pos1"), position_value(pos));
                }
                fields.insert(
                    Key::from("page"),
                    match a.page {
                        Location::Page(page) => Value::from(page as i64 + 1),
                        Location::Engine(xp) => Value::from(xp),
                    },
                );
                Value::Table(fields)
            })
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

    fn get_setting(&self, key: &str) -> Value {
        let c = self.core.borrow();
        if let Some(s) = c.session.as_ref() {
            match key {
                "font_size" => return Value::from(s.layout().font_size as f64),
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
                        [(Key::from("font_size"), Value::from(s.layout().font_size as f64))].into(),
                    )
                }
                _ => {}
            }
        }
        self.settings.read(key).clone()
    }
    fn set_setting(&mut self, key: &str, value: Value) -> Result<(), String> {
        if key == "font_size" || key == "font_face" {
            let mut c = self.core.borrow_mut();
            c.change_layout(|l| {
                if key == "font_size" {
                    if let Some(size) = value.as_f64().filter(|v| v.is_finite()) {
                        l.font_size = size.clamp(8.0, 200.0) as f32;
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
