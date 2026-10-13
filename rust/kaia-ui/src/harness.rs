//! Headless rendering and input replay for tests and agents.
//!
//! [`render`] lays out and paints one tree into a pixmap. [`Harness`] runs a
//! window on a window stack with a manual clock. Taps and swipes go through
//! the gesture recognizer, so they produce the same gestures as a device.

use kaia_events::{Ctx, DirtyQueue, Event, ManualClock, Refresh, ShowOpts, WindowStack};
use kaia_gfx::{Pixmap, Point, Rect, Size};
use kaia_input::script::replay;
use kaia_input::{Key, KeyEvent};

use crate::assets::SharedAssets;
use crate::theme::{Theme, Tone};
use crate::widget::{Handled, Out, UiEvent, Widget};
use crate::window::{to_events, Window};
use crate::WidgetId;

/// Lays out and paints `root` into a new pixmap of `size`, in the format of
/// `theme` (gray for grayscale panels, RGB for the color panel). The background
/// is the surface tone.
pub fn render(root: impl Widget + 'static, size: Size, theme: &Theme, assets: &SharedAssets) -> Pixmap {
    let mut target = Pixmap::new(size.w, size.h, theme.pixel_format());
    target.fill_rect(Rect::from_size(size), theme.color(Tone::Surface));
    let window = Window::new(root, theme.clone(), assets.clone(), size);
    let mut dirty = DirtyQueue::new(to_events(Rect::from_size(size)));
    window.paint(&mut target, &mut dirty);
    target
}

/// A window on a real window stack, driven by scripted input.
///
/// Time is manual and only moves when a gesture script says so. Each tap or
/// swipe starts at time zero, so runs are repeatable.
pub struct Harness {
    ctx: Ctx,
    stack: WindowStack,
    window: Window,
    screen: Pixmap,
    refreshes: Vec<Refresh>,
}

impl Harness {
    /// Puts `root` in a full-screen window of `size` on a new stack and applies the show.
    pub fn new(root: impl Widget + 'static, size: Size, theme: &Theme, assets: &SharedAssets) -> Self {
        let mut ctx = Ctx::new(Box::new(ManualClock::new()), to_events(Rect::from_size(size)));
        let mut stack = WindowStack::new();
        let window = Window::new(root, theme.clone(), assets.clone(), size);
        window.show(
            &mut ctx,
            ShowOpts {
                covers_fullscreen: true,
                ..ShowOpts::default()
            },
        );
        stack.apply_requests(&mut ctx);
        let mut screen = Pixmap::new(size.w, size.h, theme.pixel_format());
        screen.fill_rect(Rect::from_size(size), theme.color(Tone::Surface));
        Self {
            ctx,
            stack,
            window,
            screen,
            refreshes: Vec::new(),
        }
    }

    /// The window under test.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// The core context, for scheduling and settings in tests.
    pub fn ctx(&mut self) -> &mut Ctx {
        &mut self.ctx
    }

    /// The window stack.
    pub fn stack(&mut self) -> &mut WindowStack {
        &mut self.stack
    }

    /// Taps at `p`. Returns true when a widget handled the gesture.
    pub fn tap(&mut self, p: Point) -> bool {
        let script = format!("down 0 {} {} 0\nup 0 {} {} 50\n", p.x, p.y, p.x, p.y);
        self.run_script(&script)
    }

    /// Drags from `from` to `to` in five moves over 600 ms. Returns true when a widget handled a gesture.
    pub fn swipe(&mut self, from: Point, to: Point) -> bool {
        let mut script = format!("down 0 {} {} 0\n", from.x, from.y);
        for step in 1..=5 {
            let t = step * 100;
            let x = from.x + (to.x - from.x) * step / 5;
            let y = from.y + (to.y - from.y) * step / 5;
            script.push_str(&format!("move 0 {x} {y} {t}\n"));
        }
        script.push_str(&format!("up 0 {} {} 600\n", to.x, to.y));
        self.run_script(&script)
    }

    /// Presses a key. Returns what the window did with it.
    pub fn key(&mut self, key: Key) -> Handled {
        let ev = KeyEvent {
            key,
            pressed: true,
            time_us: 0,
        };
        self.window.dispatch(UiEvent::Key(ev), &mut self.ctx)
    }

    /// Sends a named event through the stack. Returns true when a handler used it.
    pub fn send(&mut self, ev: Event) -> bool {
        let handled = self.stack.send(&ev, &mut self.ctx);
        self.stack.apply_requests(&mut self.ctx);
        handled
    }

    /// Paints the dirty parts of the screen and records the refresh requests.
    /// Returns the frame.
    pub fn frame(&mut self) -> &Pixmap {
        self.stack.apply_requests(&mut self.ctx);
        let painted = self.window.paint(&mut self.screen, &mut self.ctx.dirty);
        self.refreshes = self.ctx.dirty.take_frame(painted);
        &self.screen
    }

    /// The frame as of the last [`Harness::frame`] call.
    pub fn screen(&self) -> &Pixmap {
        &self.screen
    }

    /// The refresh requests from the last [`Harness::frame`] call.
    pub fn refresh_requests(&self) -> &[Refresh] {
        &self.refreshes
    }

    /// Takes what the window sent to the app since the last call.
    pub fn outbox(&self) -> Vec<Out> {
        self.window.drain_outbox()
    }

    /// The focused widget.
    pub fn focus(&self) -> Option<WidgetId> {
        self.window.focus()
    }

    fn run_script(&mut self, script: &str) -> bool {
        let mut handled = false;
        for g in replay(script) {
            if self.window.dispatch(UiEvent::Gesture(g), &mut self.ctx) == Handled::Yes {
                handled = true;
            }
        }
        handled
    }
}
