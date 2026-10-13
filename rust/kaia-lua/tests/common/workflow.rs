use super::{book::BookReader, lua_ui, TempDir};
use kaia_events::Value;
use kaia_gfx::Point;
use kaia_input::{Gesture, Key};
use kaia_lua::{LuaUi, PluginOptions};
use std::{cell::RefCell, rc::Rc};

pub fn setup(data: &TempDir, book: &str, names: &[&str]) -> (LuaUi, kaia_events::ManualClock) {
    let reader = BookReader::open(book, data);
    let (mut ui, clock) = lua_ui(data, false);
    ui.set_reader_host(Rc::new(RefCell::new(reader))).unwrap();
    prepare(&mut ui);
    ui.load_plugins(PluginOptions {
        names: names.iter().map(|n| (*n).into()).collect(),
        ..Default::default()
    })
    .unwrap();
    ui.send_reader_event("ReaderReady", &[]);
    healthy(&ui);
    assert_eq!(ui.plugin_status().len(), names.len());
    (ui, clock)
}

pub fn prepare(ui: &mut LuaUi) {
    ui.exec("offline", r#"
        -- The workflow must fail if it attempts network access.
        local function denied() error('network access is excluded from desktop workflows') end
        local socket = require('socket')
        socket.tcp, socket.udp, socket.connect, socket.bind = denied, denied, denied, denied
        require('socket.http').request = denied
        require('ssl.https').request = denied
        G_reader_settings:makeFalse('flash_ui')
        function FIND_SHOWN(text)
            local U = require('ui/uimanager')
            for i = 1, 20 do
                local w = U:getNthTopWidget(i)
                if not w then return end
                local found = FIND_CONTROL(w, text)
                if found then return found end
            end
        end
        function FIND_CONTROL(w, text)
            if type(w) ~= 'table' then return end
            if w.entry and (w.entry.text == text or (w.entry.text_func and w.entry.text_func() == text)) then return w end
            if w.text == text and w.callback and w.dimen then return w end
            for _, child in ipairs(w) do
                local found = FIND_CONTROL(child, text)
                if found then return found end
            end
        end
    "#).unwrap();
}

pub fn healthy(ui: &LuaUi) {
    let errors = ui.take_errors();
    assert!(errors.is_empty(), "{errors:#?}");
    assert!(
        ui.plugin_status().iter().all(|s| s.loaded),
        "{:?}",
        ui.plugin_status()
    );
}

pub fn value(ui: &LuaUi, expression: &str) -> Value {
    ui.host()
        .eval("workflow-state", &format!("return {expression}"))
        .unwrap()
}

pub fn tap_expr(ui: &mut LuaUi, expression: &str) {
    ui.frame();
    healthy(ui);
    let pos = value(ui, &format!("(function() local w = {expression}; assert(w, 'control missing'); local d = w.dimen or w[1].dimen; return {{x=d.x+d.w/2, y=d.y+d.h/2}} end)()"));
    ui.gesture(&Gesture::Tap {
        pos: Point::new(
            pos.get("x").as_f64().unwrap() as i32,
            pos.get("y").as_f64().unwrap() as i32,
        ),
        time_us: 0,
    });
    ui.frame();
    healthy(ui);
}

pub fn tap(ui: &mut LuaUi, text: &str) {
    // Read only widget positions. The callback runs through the Kaia stack.
    for _ in 0..15 {
        ui.frame();
        let expr = format!("FIND_SHOWN({text:?})");
        if value(ui, &format!("{expr} ~= nil")).truthy() {
            tap_expr(ui, &expr);
            return;
        }
        if !value(
            ui,
            "require('ui/uimanager'):getTopmostVisibleWidget().page_info_right_chev ~= nil",
        )
        .truthy()
        {
            break;
        }
        tap_expr(
            ui,
            "require('ui/uimanager'):getTopmostVisibleWidget().page_info_right_chev",
        );
    }
    panic!(
        "control not found: {text}; {}",
        value(
            ui,
            "require('dump')(require('ui/uimanager'):getTopmostVisibleWidget().item_table)"
        )
        .as_str()
        .unwrap_or("no menu")
    );
}

pub fn type_text(ui: &mut LuaUi, text: &str) {
    for ch in text.chars() {
        ui.key(Key::Char(ch), true);
        ui.key(Key::Char(ch), false);
    }
    ui.frame();
    healthy(ui);
}

pub fn close_windows(ui: &mut LuaUi) {
    while ui.window_count() > 0 {
        ui.exec(
            "close-window",
            "local U = require('ui/uimanager'); U:close(U:getTopmostVisibleWidget())",
        )
        .unwrap();
    }
}

pub struct Library {
    pub folder: String,
    pub opened: Option<String>,
}
impl kaia_lua::LibraryHost for Library {
    fn current_folder(&self) -> String {
        self.folder.clone()
    }
    fn files(&self) -> Vec<String> {
        std::fs::read_dir(&self.folder)
            .unwrap()
            .map(|e| e.unwrap().path().to_string_lossy().into())
            .collect()
    }
    fn open_book(&mut self, path: &str) -> Result<(), String> {
        self.opened = Some(path.into());
        Ok(())
    }
    fn copy_book(&mut self, source: &str, folder: &str) -> Result<(), String> {
        let target = std::path::Path::new(folder).join(std::path::Path::new(source).file_name().unwrap());
        std::fs::copy(source, target)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn move_book(&mut self, source: &str, folder: &str) -> Result<(), String> {
        let target = std::path::Path::new(folder).join(std::path::Path::new(source).file_name().unwrap());
        std::fs::rename(source, target).map_err(|e| e.to_string())
    }
    fn show_folder(&mut self, path: &str) -> Result<(), String> {
        self.folder = path.into();
        Ok(())
    }
}
