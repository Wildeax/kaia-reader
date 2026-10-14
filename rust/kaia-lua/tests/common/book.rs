use super::TempDir;
use kaia_book::{BookSession, Drawer, Location, OpenOptions, Pos, Selection};
use kaia_doc::Document;
use kaia_events::{Key, Value};
use kaia_lua::ReaderHost;
use std::collections::BTreeMap;

pub struct BookReader {
    session: BookSession,
    document: Box<dyn Document>,
    settings: BTreeMap<String, Value>,
    cre: Option<kaia_doc::cre::CreDocument>,
}

impl BookReader {
    pub fn render(&mut self) -> kaia_gfx::Pixmap {
        (*self.session.render_current(super::UI_SCREEN).unwrap()).clone()
    }
    pub fn open(file: &str, data: &TempDir) -> Self {
        let path = super::repo_root().join("test").join(file);
        Self::open_path(&path, data)
    }

    pub fn open_path(path: &std::path::Path, data: &TempDir) -> Self {
        let session = BookSession::open(path, OpenOptions::new(data.path())).unwrap();
        let document = kaia_doc::open(path).unwrap();
        let mut settings = BTreeMap::new();
        settings.insert(
            "partial_md5_checksum".into(),
            Value::from(session.identity().partial_md5.clone()),
        );
        Self {
            cre: None,
            session,
            document,
            settings,
        }
    }
}

impl ReaderHost for BookReader {
    fn file(&self) -> String {
        self.session.path().to_string_lossy().to_string()
    }
    fn page_count(&self) -> u32 {
        self.session.page_count() as u32
    }
    fn current_page(&self) -> u32 {
        self.session.current().page + 1
    }
    fn go_to_page(&mut self, page: u32) -> Result<(), String> {
        self.session
            .go_to(Location::Page(page.saturating_sub(1)))
            .map_err(|e| e.to_string())
    }
    fn page_text(&mut self, page: u32) -> Result<String, String> {
        self.document
            .page_text(page.saturating_sub(1))
            .map(|p| p.text)
            .map_err(|e| e.to_string())
    }
    fn document_query(&mut self, method: &str, args: Vec<String>) -> Result<Value, String> {
        if self.cre.is_none() {
            self.cre =
                Some(kaia_doc::cre::CreDocument::open(self.session.path()).map_err(|e| e.to_string())?);
        }
        self.cre
            .as_ref()
            .unwrap()
            .plugin_query(method, &args, self.current_page() - 1)
            .map_err(|e| e.to_string())
    }
    fn xpointer_for_page(&self, page: u32) -> Result<String, String> {
        if page != self.current_page() {
            return Err("BookReader xpointer page is not current".into());
        }
        match self.session.current().location {
            Location::Engine(xp) => Ok(xp),
            Location::Page(_) => Err("BookReader has no xpointer for a fixed page".into()),
        }
    }
    fn page_for_xpointer(&self, xpointer: &str) -> Result<u32, String> {
        self.document
            .resolve(&Location::Engine(xpointer.into()))
            .map(|page| page + 1)
            .map_err(|e| e.to_string())
    }
    fn native_page_dimensions(&self, page: u32) -> Result<Value, String> {
        let size = self
            .document
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
    fn properties(&self) -> Value {
        let m = self.session.metadata();
        Value::Table(
            [
                (
                    Key::from("display_title"),
                    Value::from(m.title.clone().unwrap_or_else(|| "Test book".into())),
                ),
                (
                    Key::from("authors"),
                    Value::from(m.author.clone().unwrap_or_default()),
                ),
            ]
            .into(),
        )
    }
    fn annotations(&mut self) -> Vec<Value> {
        self.session
            .annotations()
            .unwrap()
            .into_iter()
            .map(|a| a.to_value())
            .collect()
    }
    fn toc(&self) -> Vec<Value> {
        self.session
            .toc()
            .iter()
            .map(|entry| {
                Value::Table(
                    [
                        (Key::from("title"), Value::from(entry.title.clone())),
                        (Key::from("level"), Value::from(entry.level as i64)),
                        (Key::from("page"), Value::from(entry.page.unwrap_or(0) as i64 + 1)),
                    ]
                    .into(),
                )
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
                page: v.get("page").as_i64().ok_or("missing page")? as u32 - 1,
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
        self.session
            .add_highlight(
                &selection,
                Drawer::parse(item.get("drawer").as_str().unwrap_or("lighten")).ok_or("invalid drawer")?,
                item.get("color").as_str().unwrap_or("yellow"),
                item.get("note").as_str(),
            )
            .map_err(|e| e.to_string())
    }
    fn update_annotation(&mut self, id: u64, item: Value) -> Result<(), String> {
        self.session
            .edit_note(id, item.get("note").as_str())
            .map_err(|e| e.to_string())?;
        if let Some(drawer) = item.get("drawer").as_str() {
            self.session
                .set_highlight_style(
                    id,
                    Drawer::parse(drawer).ok_or("invalid drawer")?,
                    item.get("color").as_str().unwrap_or("yellow"),
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn replace_annotations(&mut self, items: Vec<Value>) -> Result<(), String> {
        self.session
            .replace_plugin_annotations(&items)
            .map_err(|e| e.to_string())
    }
    fn delete_annotation(&mut self, id: u64) -> Result<(), String> {
        self.session.delete_annotation(id).map_err(|e| e.to_string())
    }
    fn flush_settings(&mut self) -> Result<(), String> {
        self.session.save().map_err(|e| e.to_string())
    }
    fn get_setting(&self, key: &str) -> Value {
        if key == "configurable" {
            return Value::Table(
                [(
                    "font_size".into(),
                    Value::from(self.session.layout().font_size as f64),
                )]
                .into(),
            );
        }
        if key == "font_size" {
            return Value::from(self.session.layout().font_size as f64);
        }
        if key == "font_face" {
            return self
                .session
                .layout()
                .font_family
                .clone()
                .map(Value::from)
                .unwrap_or_default();
        }
        self.settings.get(key).cloned().unwrap_or_default()
    }
    fn set_setting(&mut self, key: &str, value: Value) -> Result<(), String> {
        if key == "font_size" || key == "font_face" {
            let mut layout = self.session.layout().clone();
            if key == "font_size" {
                layout.font_size = value.as_f64().ok_or("font size must be numeric")? as f32;
            } else {
                layout.font_family = value.as_str().map(str::to_owned);
            }
            self.session.set_layout(layout).map_err(|e| e.to_string())?;
        }
        self.settings.insert(key.into(), value);
        Ok(())
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
