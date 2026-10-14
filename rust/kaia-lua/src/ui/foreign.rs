//! One KOReader widget on the window stack.
//!
//! [`Foreign`] holds the widget in the Lua registry, with the position it was
//! shown at. [`Renderer`] is the `ForeignRenderer` of the window's
//! `ForeignWidget`: it paints the widget into the window's pixmap and turns the
//! Kaia events that reach the window into KOReader events. Lua errors are not
//! raised into Rust. They are recorded on the bridge, and `LuaUi` reports them.

use std::rc::Rc;

use kaia_gfx::{PixelFormat, Pixmap, Rect, Size};
use kaia_ui::foreign::ForeignRenderer;
use kaia_ui::{Handled, UiEvent};
use mlua::{Function, Lua, RegistryKey, Table, Value as LuaValue};

use super::bridge::Bridge;
use super::input;

/// `Blitbuffer.TYPE_BB8` in `base/ffi/blitbuffer.lua`.
const BB_TYPE_BB8: i64 = 1;
/// `Blitbuffer.TYPE_BBRGB32` in `base/ffi/blitbuffer.lua`.
const BB_TYPE_BBRGB32: i64 = 5;

/// The shim module that paints a widget into a buffer.
const PAINT_MODULE: &str = "kaia/paint";

/// A shown widget: the registry key, the position and the Lua handles.
pub(crate) struct Foreign {
    lua: Lua,
    bridge: Rc<Bridge>,
    widget: Rc<RegistryKey>,
    x: i32,
    y: i32,
}

impl Foreign {
    pub(crate) fn new(lua: Lua, bridge: Rc<Bridge>, widget: Rc<RegistryKey>, x: i32, y: i32) -> Self {
        Self {
            lua,
            bridge,
            widget,
            x,
            y,
        }
    }

    /// The widget table. It stays in the registry while this `Foreign` lives.
    pub(crate) fn widget_key(&self) -> &Rc<RegistryKey> {
        &self.widget
    }

    fn table(&self) -> mlua::Result<Table> {
        self.lua.registry_value(&self.widget)
    }

    /// Calls `widget:handleEvent(event)`. Returns true when the widget used the
    /// event. A Lua error is recorded and counts as not used.
    pub(crate) fn deliver(&self, event: LuaValue) -> bool {
        let outcome = self.table().and_then(|widget| input::handle(&widget, event));
        match outcome {
            Ok(used) => used,
            Err(e) => {
                self.bridge.record_error(format!("handleEvent failed: {e}"));
                false
            }
        }
    }

    /// Paints the widget into `target`, which covers the whole window. The
    /// widget draws through a Blitbuffer over the pixels of `target`.
    ///
    /// Colour frames are RGB24, but KOReader's colour buffers are 32-bit
    /// (`TYPE_BBRGB32`). Its C blitter aborts when an alpha icon meets a 24-bit
    /// buffer. So a colour widget paints into a 32-bit scratch copy of the
    /// frame, and the copy is written back as RGB24.
    pub(crate) fn paint(&self, target: &mut Pixmap) {
        let (width, height) = (target.width(), target.height());
        let outcome = match target.format() {
            PixelFormat::Gray8 => {
                let stride = width as usize;
                self.paint_into(target.data_mut(), width, height, BB_TYPE_BB8, stride)
            }
            PixelFormat::Rgb24 => {
                let pixels = width as usize * height as usize;
                let mut scratch = vec![255u8; pixels * 4];
                let (src, _) = target.data().as_chunks::<3>();
                let (dst, _) = scratch.as_chunks_mut::<4>();
                for (px, out) in src.iter().zip(dst.iter_mut()) {
                    out[..3].copy_from_slice(px);
                }
                let stride = width as usize * 4;
                let painted = self.paint_into(&mut scratch, width, height, BB_TYPE_BBRGB32, stride);
                let (rgba, _) = scratch.as_chunks::<4>();
                let (rgb, _) = target.data_mut().as_chunks_mut::<3>();
                for (px, out) in rgba.iter().zip(rgb.iter_mut()) {
                    out.copy_from_slice(&px[..3]);
                }
                painted
            }
        };
        if let Err(e) = outcome {
            self.bridge.record_error(format!("paintTo failed: {e}"));
        }
    }

    /// Calls the shim's paint function with a Blitbuffer over `buffer`.
    fn paint_into(
        &self,
        buffer: &mut [u8],
        width: u32,
        height: u32,
        bb_type: i64,
        stride: usize,
    ) -> mlua::Result<()> {
        // The address is a number for Lua. The buffer lives until this call
        // returns, and the shim does not keep it past `paintTo`.
        let addr = buffer.as_mut_ptr() as usize as f64;
        let require: Function = self.lua.globals().get("require")?;
        let module: Table = require.call(PAINT_MODULE)?;
        let paint: Function = module.get("paint")?;
        let widget = self.table()?;
        paint.call::<()>((widget, width, height, bb_type, addr, stride, self.x, self.y))
    }
}

/// The `ForeignRenderer` of a window's `ForeignWidget`.
pub(crate) struct Renderer(pub(crate) Rc<Foreign>);

impl ForeignRenderer for Renderer {
    fn paint(&mut self, target: &mut Pixmap, _rect: Rect) {
        self.0.paint(target);
    }

    fn event(&mut self, ev: &UiEvent) -> Handled {
        let lua = self.0.lua.clone();
        let event = match ev {
            UiEvent::Gesture(g) => input::gesture_event(&lua, g),
            UiEvent::Key(k) => input::key_event(&lua, k.key, k.pressed),
            UiEvent::Named(e) => input::named_event(&lua, &e.name, &e.args),
            UiEvent::FocusGained | UiEvent::FocusLost | UiEvent::Pen(_) => return Handled::No,
        };
        match event {
            Ok(event) => {
                if self.0.deliver(event) {
                    Handled::Yes
                } else {
                    Handled::No
                }
            }
            Err(e) => {
                self.0
                    .bridge
                    .record_error(format!("building an event failed: {e}"));
                Handled::No
            }
        }
    }

    fn preferred_size(&self, max: Size) -> Size {
        max
    }
}
