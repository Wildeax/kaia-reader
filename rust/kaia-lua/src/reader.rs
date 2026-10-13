//! Document and library facts supplied by the Rust reader.

use kaia_events::Value;

/// The document boundary used by KOReader reader plugins.
///
/// Page numbers at this boundary are one based, as in KOReader Lua. Values in
/// `annotations` use KOReader's `pos0`, `pos1`, `page`, `text`, `note`,
/// `chapter` and `datetime` fields. Every saved item includes a stable `kaia_id`.
pub trait ReaderHost {
    fn file(&self) -> String;
    fn page_count(&self) -> u32;
    fn current_page(&self) -> u32;
    fn go_to_page(&mut self, page: u32) -> Result<(), String>;
    fn page_text(&mut self, page: u32) -> Result<String, String>;
    fn properties(&self) -> Value;
    fn annotations(&mut self) -> Vec<Value>;
    fn toc(&self) -> Vec<Value>;

    fn text_from_positions(&mut self, _start: &Value, _end: &Value) -> Result<String, String> {
        Err("ReaderHost.text_from_positions is unavailable".into())
    }

    fn boxes_from_positions(&mut self, _start: &Value, _end: &Value) -> Result<Vec<Value>, String> {
        Err("ReaderHost.boxes_from_positions is unavailable".into())
    }

    fn xpointer_for_page(&self, _page: u32) -> Result<String, String> {
        Err("ReaderHost.xpointer_for_page is unavailable".into())
    }

    fn page_for_xpointer(&self, _xpointer: &str) -> Result<u32, String> {
        Err("ReaderHost.page_for_xpointer is unavailable".into())
    }

    fn page_margins(&self, _page: u32) -> Result<Value, String> {
        Err("ReaderHost.page_margins is unavailable".into())
    }

    fn native_page_dimensions(&self, _page: u32) -> Result<Value, String> {
        Err("ReaderHost.native_page_dimensions is unavailable".into())
    }

    /// Adds a KOReader annotation and returns its stable host id.
    fn add_annotation(&mut self, _item: Value) -> Result<u64, String> {
        Err("ReaderHost.add_annotation is unavailable".into())
    }

    /// Replaces the editable fields of an annotation identified by `kaia_id`.
    fn update_annotation(&mut self, _id: u64, _item: Value) -> Result<(), String> {
        Err("ReaderHost.update_annotation is unavailable".into())
    }

    fn delete_annotation(&mut self, _id: u64) -> Result<(), String> {
        Err("ReaderHost.delete_annotation is unavailable".into())
    }

    fn flush_settings(&mut self) -> Result<(), String> {
        Err("ReaderHost.flush_settings is unavailable".into())
    }

    fn get_setting(&self, key: &str) -> Value;
    fn set_setting(&mut self, key: &str, value: Value) -> Result<(), String>;
}

/// The library boundary used by file manager plugins.
pub trait LibraryHost {
    fn current_folder(&self) -> String;
    fn files(&self) -> Vec<String>;
    fn open_book(&mut self, path: &str) -> Result<(), String>;
    fn copy_book(&mut self, _source: &str, _folder: &str) -> Result<(), String> {
        Err("LibraryHost.copy_book is unavailable".into())
    }
    fn move_book(&mut self, _source: &str, _folder: &str) -> Result<(), String> {
        Err("LibraryHost.move_book is unavailable".into())
    }
    fn show_folder(&mut self, _path: &str) -> Result<(), String> {
        Err("LibraryHost.show_folder is unavailable".into())
    }
}
