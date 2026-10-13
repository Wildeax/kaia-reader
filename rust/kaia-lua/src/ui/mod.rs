//! KOReader widgets running in Kaia windows (stage L2 and L3 of `LUA_COMPAT.md`).
//!
//! [`LuaUi`] owns the Lua host, the window stack and the scheduler. Each
//! `UIManager:show(widget)` puts a `kaia_ui::Window` on the stack. Its root is a
//! `ForeignWidget` that paints the widget into the window's pixmap, so the
//! toolkit clips, dirties and paints the window the same way it paints its own
//! widgets. `setDirty` and the other calls keep their KOReader meaning. The
//! design and the differences from KOReader are in `rust/kaia-lua/README.md`.
//!
//! One thread drives a `LuaUi`. The loop is: feed input with
//! [`LuaUi::gesture`] and [`LuaUi::key`], run due tasks with [`LuaUi::run_due`],
//! and paint with [`LuaUi::frame`], which returns the refreshes to send.

mod bridge;
mod foreign;
mod input;

use std::rc::Rc;

use kaia_events::{Clock, Ctx, DirtyQueue, Refresh, ShowOpts, Task, Value, WindowId, WindowStack};
use kaia_gfx::{Pixmap, Size};
use kaia_input::{Gesture, Key};
use kaia_ui::assets::SharedAssets;
use kaia_ui::foreign::ForeignWidget;
use kaia_ui::theme::{Scale, Theme};
use kaia_ui::Window;
use mlua::{Function, Lua, Table, Value as LuaValue};

use crate::error::lua_error;
use crate::{Error, LuaHost};
use bridge::{Bridge, Flags, Mirror, Request, TaskRecord};
use foreign::{Foreign, Renderer};

/// The screen that the Lua widgets see: the size, the DPI and the panel type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiConfig {
    pub size: Size,
    /// `None` keeps KOReader's default: `scaleBySize` follows the shorter side only.
    pub dpi: Option<u32>,
    /// True for a colour panel (RGB frames), false for grayscale (gray frames).
    pub color: bool,
}

/// A widget that the window stack shows.
struct Shown {
    id: WindowId,
    ptr: usize,
    invisible: bool,
    foreign: Rc<Foreign>,
    window: Window,
}

/// The Lua widget UI: KOReader's `UIManager` on Kaia's window stack.
///
/// Field order matters. The stack and its windows go before the host, so the
/// Lua values in the windows are released before the state closes.
pub struct LuaUi {
    stack: WindowStack,
    ctx: Ctx,
    shown: Vec<Shown>,
    bridge: Rc<Bridge>,
    theme: Theme,
    assets: SharedAssets,
    screen: Pixmap,
    lua: Lua,
    host: LuaHost,
}

impl LuaUi {
    /// Creates the UI on `host`. Sets the display that `device` reads, and the
    /// `KAIA_UI` table that `ui/uimanager` calls. Time comes from `clock`, so
    /// tests pass a `ManualClock`.
    ///
    /// KOReader's widgets open some resources relative to the working directory
    /// (`resources/icons`), as `koreader.sh` expects. So this sets the working
    /// directory to the KOReader tree. The change is process-wide: one process
    /// runs one `LuaUi`.
    pub fn new(
        host: LuaHost,
        config: UiConfig,
        assets: SharedAssets,
        clock: Box<dyn Clock>,
    ) -> Result<Self, Error> {
        std::env::set_current_dir(&host.paths().koreader_dir).map_err(|e| {
            Error::Init(format!(
                "cannot enter the KOReader tree {}: {e}",
                host.paths().koreader_dir.display()
            ))
        })?;
        let lua = host.lua().clone();
        let theme = Theme::new(
            Scale {
                width: config.size.w,
                height: config.size.h,
                dpi: config.dpi,
            },
            config.color,
        );
        let mut screen = Pixmap::new(config.size.w, config.size.h, theme.pixel_format());
        let frame_addr = screen.data_mut().as_mut_ptr() as usize as f64;
        set_display(&lua, &config, frame_addr).map_err(lua_error)?;
        let bridge = Rc::new(Bridge::new());
        bridge::install(&lua, &bridge).map_err(lua_error)?;
        let ctx = Ctx::new(clock, screen_rect(config.size));
        Ok(Self {
            stack: WindowStack::new(),
            ctx,
            shown: Vec::new(),
            bridge,
            theme,
            assets,
            screen,
            lua,
            host,
        })
    }

    /// The Lua host. Tests run Lua snippets through it.
    pub fn host(&self) -> &LuaHost {
        &self.host
    }

    /// The app context: the scheduler, the dirty queue and the settings.
    pub fn ctx(&mut self) -> &mut Ctx {
        &mut self.ctx
    }

    /// The frame that [`LuaUi::frame`] paints.
    pub fn screen(&self) -> &Pixmap {
        &self.screen
    }

    /// The number of windows on the stack.
    pub fn window_count(&self) -> usize {
        self.stack.len()
    }

    /// Takes the Lua errors recorded since the last call. A failed handler or
    /// paint is recorded and the UI keeps running.
    pub fn take_errors(&self) -> Vec<String> {
        self.bridge.take_errors()
    }

    /// Delivers a gesture the way `UIManager:sendEvent` delivers
    /// `Event:new("Gesture", ges)`. Returns true when a widget used it.
    pub fn gesture(&mut self, g: &Gesture) -> bool {
        match input::gesture_event(&self.lua, g) {
            Ok(event) => self.send_lua(event, false),
            Err(e) => {
                self.bridge
                    .record_error(format!("building a gesture event failed: {e}"));
                false
            }
        }
    }

    /// Delivers a key press or release as `KeyPress` or `KeyRelease`. Returns true when a widget used it.
    pub fn key(&mut self, key: Key, pressed: bool) -> bool {
        match input::key_event(&self.lua, key, pressed) {
            Ok(event) => self.send_lua(event, false),
            Err(e) => {
                self.bridge
                    .record_error(format!("building a key event failed: {e}"));
                false
            }
        }
    }

    /// Sends a named event with Kaia values as arguments, as `UIManager:sendEvent`
    /// does. Returns true when a widget used it.
    pub fn send_named(&mut self, name: &str, args: &[Value]) -> bool {
        match input::named_event(&self.lua, name, args) {
            Ok(event) => self.send_lua(event, false),
            Err(e) => {
                self.bridge
                    .record_error(format!("building event {name} failed: {e}"));
                false
            }
        }
    }

    /// Runs a Lua chunk, then applies the window, timer and event requests that
    /// it made. Use this for Lua that runs outside a UI event, such as plugin
    /// start-up code.
    pub fn exec(&mut self, name: &str, source: &str) -> Result<(), Error> {
        let outcome = self.host.exec(name, source);
        self.drain();
        outcome
    }

    /// Evaluates a Lua chunk and returns its first value, after applying the
    /// requests it made. See [`LuaUi::exec`].
    pub fn eval(&mut self, name: &str, source: &str) -> Result<kaia_events::Value, Error> {
        let outcome = self.host.eval(name, source);
        self.drain();
        outcome
    }

    /// Runs the tasks that are due now, then applies what they asked for. A
    /// task that schedules another task runs on a later call. Returns the number of tasks run.
    pub fn run_due(&mut self) -> usize {
        let now = self.ctx.scheduler.now();
        let count = self.ctx.run_due(now);
        self.drain();
        count
    }

    /// Paints the windows that are dirty and returns the refreshes for the
    /// panel, as `UIManager:_repaint` does. Read the frame with [`LuaUi::screen`].
    pub fn frame(&mut self) -> Vec<Refresh> {
        self.drain();
        if self.bridge.take_repaint() {
            self.stack.set_dirty_all(None, &mut self.ctx);
        }
        let plan = self.stack.paint_plan(&mut self.ctx.dirty);
        let painted = !plan.is_empty();
        // Window paints report their own refreshes. Those are dropped: the
        // refreshes come from the Lua calls (`setDirty`, `show`, `close`), so
        // the modes match KOReader's names.
        let mut paint_refreshes = DirtyQueue::new(screen_rect(self.screen.size()));
        for id in plan {
            let window = self.shown.iter().find(|s| s.id == id).map(|s| s.window.clone());
            if let Some(window) = window {
                window.invalidate();
                window.paint(&mut self.screen, &mut paint_refreshes);
            }
        }
        self.drain();
        self.run_after_paint();
        self.ctx.dirty.take_frame(painted)
    }

    /// Runs the refresh functions that `setDirty` got, after the paint. Each
    /// returns a mode, a region and a dither flag, and the mode is queued.
    fn run_after_paint(&mut self) {
        for key in self.bridge.take_after_paint() {
            if let Err(e) = self.run_refresh_function(&key) {
                self.bridge.record_error(format!("refresh function failed: {e}"));
            }
        }
    }

    fn run_refresh_function(&mut self, key: &mlua::RegistryKey) -> mlua::Result<()> {
        let function: Function = self.lua.registry_value(key)?;
        let (mode, region, dither): (Option<String>, Option<Table>, Option<bool>) = function.call(())?;
        let Some(mode) = mode else {
            return Ok(());
        };
        let mode = kaia_events::RefreshMode::from_name(&mode)
            .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown refresh mode {mode:?}")))?;
        let region = match region {
            Some(r) => Some(bridge::rect_from(&r)?),
            None => None,
        };
        self.ctx.dirty.refresh(mode, region, dither.unwrap_or(false));
        Ok(())
    }

    /// Applies the requests that Lua made, in order. Applying a request can run
    /// Lua again (a `Show` handler, for example), so the loop repeats until the
    /// queue is empty.
    fn drain(&mut self) {
        while let Some(request) = self.bridge.pop() {
            self.apply(request);
        }
        self.sync_mirror();
    }

    fn apply(&mut self, request: Request) {
        match request {
            Request::Show {
                widget,
                ptr,
                x,
                y,
                flags,
                refresh,
            } => self.apply_show(widget, ptr, x, y, flags, refresh),
            Request::Close { widget, ptr, refresh } => self.apply_close(widget, ptr, refresh),
            Request::SetDirty {
                ptr,
                all,
                invisible,
                refresh,
            } => self.apply_set_dirty(ptr, all, invisible, refresh),
            Request::Schedule { seconds, action, ptr } => self.apply_schedule(seconds, action, ptr),
            Request::Unschedule { ptr } => self.apply_unschedule(ptr),
            Request::Send { event, broadcast } => match self.lua.registry_value::<LuaValue>(&event) {
                Ok(value) => {
                    self.send_lua(value, broadcast);
                }
                Err(e) => self.bridge.record_error(format!("event lost: {e}")),
            },
        }
    }

    /// `UIManager:show`. The widget gets a window, the window goes where the
    /// stack puts it, and the widget gets its `Show` event.
    fn apply_show(
        &mut self,
        widget: Rc<mlua::RegistryKey>,
        ptr: usize,
        x: i32,
        y: i32,
        flags: Flags,
        refresh: Option<Refresh>,
    ) {
        let foreign = Rc::new(Foreign::new(
            self.lua.clone(),
            Rc::clone(&self.bridge),
            widget,
            x,
            y,
        ));
        let window = Window::new(
            ForeignWidget::new(Renderer(Rc::clone(&foreign))),
            self.theme.clone(),
            Rc::clone(&self.assets),
            self.screen.size(),
        );
        let opts = ShowOpts {
            modal: flags.modal,
            toast: flags.toast,
            covers_fullscreen: flags.covers_fullscreen,
            always_active: flags.always_active,
            refresh,
        };
        let id = window.show(&mut self.ctx, opts);
        self.stack.apply_requests(&mut self.ctx);
        self.shown.push(Shown {
            id,
            ptr,
            invisible: flags.invisible,
            foreign,
            window,
        });
    }

    /// `UIManager:close`. Every window of the widget leaves the stack, and the
    /// widget gets `FlushSettings` and `CloseWidget`. A widget that is not on
    /// the stack gets the two events and nothing else, as in KOReader.
    fn apply_close(&mut self, widget: Rc<mlua::RegistryKey>, ptr: usize, refresh: Option<Refresh>) {
        let mut refresh = refresh;
        let mut closed = false;
        while let Some(index) = self.shown.iter().position(|s| s.ptr == ptr) {
            let shown = self.shown.remove(index);
            self.stack.close(shown.id, refresh.take(), &mut self.ctx);
            closed = true;
        }
        if !closed {
            for name in ["FlushSettings", "CloseWidget"] {
                let event = input::named_event(&self.lua, name, &[]);
                match event {
                    Ok(event) => {
                        let target = self.lua.registry_value::<Table>(&widget);
                        match target.and_then(|t| input::handle(&t, event)) {
                            Ok(_) => {}
                            Err(e) => self.bridge.record_error(format!("{name} failed: {e}")),
                        }
                    }
                    Err(e) => self.bridge.record_error(format!("{name} failed: {e}")),
                }
            }
        }
    }

    /// `UIManager:setDirty`. A window-level widget is marked for repaint. Any
    /// other widget only queues the refresh, as KOReader does.
    fn apply_set_dirty(&mut self, ptr: Option<usize>, all: bool, invisible: bool, refresh: Option<Refresh>) {
        if all {
            self.stack.set_dirty_all(refresh, &mut self.ctx);
            return;
        }
        let window = match ptr {
            Some(ptr) if !invisible => self.shown.iter().find(|s| s.ptr == ptr).map(|s| s.id),
            _ => None,
        };
        self.stack.set_dirty(window, refresh, &mut self.ctx);
    }

    /// `UIManager:scheduleIn`. The action runs once when it is due, and its
    /// registry key is released after it runs.
    fn apply_schedule(&mut self, seconds: f64, action: Rc<mlua::RegistryKey>, ptr: usize) {
        let token = self.bridge.next_token();
        let bridge = Rc::clone(&self.bridge);
        let lua = self.lua.clone();
        let task: Task = Box::new(move |_ctx: &mut Ctx| {
            bridge.forget_task(token);
            if let Err(e) = call_action(&lua, &action) {
                bridge.record_error(format!("scheduled task failed: {e}"));
            }
        });
        let id = self.ctx.scheduler.schedule_in(seconds, task);
        self.bridge.tasks.borrow_mut().push(TaskRecord { token, ptr, id });
    }

    /// `UIManager:unschedule`. Removes every queued task for the action.
    fn apply_unschedule(&mut self, ptr: usize) {
        let scheduler = &mut self.ctx.scheduler;
        self.bridge.tasks.borrow_mut().retain(|record| {
            if record.ptr == ptr {
                scheduler.unschedule(record.id);
                false
            } else {
                true
            }
        });
    }

    /// Calls `handleEvent` on each window in the order of `UIManager:sendEvent`:
    /// toasts first (their answer is ignored), then the top window, then the
    /// windows below it that are always active. Returns true when a handler used it.
    /// With `broadcast`, every window gets the event (`broadcastEvent`).
    fn send_lua(&mut self, event: LuaValue, broadcast: bool) -> bool {
        let ids = self.stack.ids_top_down();
        if broadcast {
            let mut handled = false;
            for id in ids {
                if self.deliver_to(id, &event) {
                    handled = true;
                }
            }
            return handled;
        }
        let mut top = None;
        for (index, id) in ids.iter().enumerate() {
            let Some(opts) = self.stack.options(*id) else {
                continue;
            };
            if opts.toast {
                self.deliver_to(*id, &event);
            } else {
                top = Some(index);
                break;
            }
        }
        let Some(top) = top else {
            return false;
        };
        if self.deliver_to(ids[top], &event) {
            return true;
        }
        for id in &ids[top + 1..] {
            let Some(opts) = self.stack.options(*id) else {
                continue;
            };
            if opts.always_active && self.deliver_to(*id, &event) {
                return true;
            }
        }
        false
    }

    /// Delivers one event to one window, then applies what the handler asked for.
    fn deliver_to(&mut self, id: WindowId, event: &LuaValue) -> bool {
        let Some(foreign) = self
            .shown
            .iter()
            .find(|s| s.id == id)
            .map(|s| Rc::clone(&s.foreign))
        else {
            return false;
        };
        let used = foreign.deliver(event.clone());
        self.drain();
        used
    }

    /// Copies the stack order into the bridge, for `isWidgetShown` and friends.
    fn sync_mirror(&self) {
        let mut mirror = Vec::new();
        for id in self.stack.ids_top_down() {
            if let Some(shown) = self.shown.iter().find(|s| s.id == id) {
                mirror.push(Mirror {
                    ptr: shown.ptr,
                    invisible: shown.invisible,
                    widget: Rc::clone(shown.foreign.widget_key()),
                });
            }
        }
        *self.bridge.mirror.borrow_mut() = mirror;
    }
}

/// Calls a scheduled action with no arguments.
fn call_action(lua: &Lua, action: &mlua::RegistryKey) -> mlua::Result<()> {
    let function: Function = lua.registry_value(action)?;
    function.call::<()>(())
}

fn screen_rect(size: Size) -> kaia_events::Rect {
    kaia_events::Rect::new(0, 0, size.w as i32, size.h as i32)
}

/// Sets the global `KAIA_DISPLAY` that `device.lua` reads. `frame_addr` is the
/// address of the frame pixels, which the screen buffer wraps.
fn set_display(lua: &Lua, config: &UiConfig, frame_addr: f64) -> mlua::Result<()> {
    let t = lua.create_table()?;
    t.set("width", config.size.w)?;
    t.set("height", config.size.h)?;
    // KOReader's framebuffer reports 160 DPI when the device sets none.
    t.set("dpi", config.dpi.unwrap_or(160))?;
    t.set("dpi_override", config.dpi.is_some())?;
    t.set("color", config.color)?;
    t.set("touch", true)?;
    t.set("eink", true)?;
    t.set("frame_addr", frame_addr)?;
    lua.globals().set("KAIA_DISPLAY", t)
}
