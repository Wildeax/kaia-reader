//! The context that handlers and scheduled tasks receive.
//!
//! The window stack is owned by the app, not by `Ctx`, so a handler cannot
//! reach it while the stack is delivering an event. Handlers ask for window
//! changes with `show` and `close`. Those requests wait in `Ctx` and the stack
//! applies them after the handler returns, which is when KOReader's `show` and
//! `close` would have taken effect.

use std::collections::VecDeque;
use std::time::Duration;

use crate::dirty::{DirtyQueue, Refresh};
use crate::geom::Rect;
use crate::handler::EventHandler;
use crate::scheduler::{Clock, Scheduler};
use crate::settings::Settings;
use crate::window::{ShowOpts, Window, WindowId};

/// A window change waiting for the stack to apply it.
pub(crate) enum WindowRequest {
    Show {
        id: WindowId,
        window: Window,
        opts: ShowOpts,
    },
    Close {
        id: WindowId,
        refresh: Option<Refresh>,
    },
}

/// Shared app state for handlers and tasks: the scheduler, the refresh
/// queue, the settings and the queued window changes.
pub struct Ctx {
    pub scheduler: Scheduler,
    pub dirty: DirtyQueue,
    pub settings: Settings,
    next_window: u64,
    pub(crate) requests: VecDeque<WindowRequest>,
}

impl Ctx {
    /// A context with an empty scheduler, empty settings and a refresh
    /// queue for a screen of the given size.
    pub fn new(clock: Box<dyn Clock>, screen: Rect) -> Self {
        Self {
            scheduler: Scheduler::new(clock),
            dirty: DirtyQueue::new(screen),
            settings: Settings::new(),
            next_window: 0,
            requests: VecDeque::new(),
        }
    }

    /// Queues a `show`. The window gets its id now and joins the stack when
    /// the stack applies the request.
    pub fn show(&mut self, window: impl EventHandler + 'static, opts: ShowOpts) -> WindowId {
        let id = self.alloc_window_id();
        let window: Window = Box::new(window);
        self.requests.push_back(WindowRequest::Show { id, window, opts });
        id
    }

    /// Queues a `close`. It does nothing if the window is no longer on the stack.
    pub fn close(&mut self, id: WindowId, refresh: Option<Refresh>) {
        self.requests.push_back(WindowRequest::Close { id, refresh });
    }

    /// Number of window changes waiting to be applied.
    pub fn pending_window_requests(&self) -> usize {
        self.requests.len()
    }

    pub(crate) fn alloc_window_id(&mut self) -> WindowId {
        self.next_window += 1;
        WindowId::from_raw(self.next_window)
    }

    /// Runs every task due at `now`. Tasks scheduled while they run wait for
    /// the next call. Returns the number of tasks run. Window changes they
    /// queue are applied by `WindowStack::apply_requests`.
    pub fn run_due(&mut self, now: Duration) -> usize {
        let tasks = self.scheduler.run_due(now);
        let count = tasks.len();
        for task in tasks {
            task(self);
        }
        count
    }
}
