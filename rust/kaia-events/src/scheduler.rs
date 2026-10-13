//! A single-threaded task queue mirroring `UIManager`'s scheduling
//! (`frontend/ui/uimanager.lua`): `schedule`, `scheduleIn`, `nextTick`,
//! `tickAfterNext`, `unschedule`, `_checkTasks`.
//!
//! Ordering guarantees, as in `UIManager`:
//! - Tasks run in order of due time.
//! - Tasks with the same due time run in the order they were scheduled.
//! - A task scheduled while `run_due` runs is not returned by that call, even
//!   when it is already due. It runs on the next `run_due` call, which is the
//!   next loop iteration.
//!
//! Tasks are boxed closures that receive `&mut Ctx`. `run_due` removes the due
//! tasks from the queue and returns them, so a task can schedule and
//! unschedule other tasks without borrowing the queue.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::ctx::Ctx;

/// A monotonic clock. `now` returns the time since the clock's origin.
pub trait Clock {
    fn now(&self) -> Duration;
}

/// Monotonic time from `std::time::Instant`, with the origin at construction.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}

/// A clock that only moves when the caller says so. Clones share the time.
#[derive(Debug, Clone, Default)]
pub struct ManualClock {
    now: Rc<Cell<Duration>>,
}

impl ManualClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, now: Duration) {
        self.now.set(now);
    }

    pub fn advance(&self, by: Duration) {
        self.now.set(self.now.get() + by);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.now.get()
    }
}

/// Handle for a scheduled task, used by `unschedule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(u64);

/// A unit of work. It receives the app context.
pub type Task = Box<dyn FnOnce(&mut Ctx)>;

/// Queue key: due time first, then scheduling order (FIFO for equal times).
type Key = (Duration, u64);

pub struct Scheduler {
    clock: Box<dyn Clock>,
    queue: BTreeMap<Key, Task>,
    next_id: u64,
}

impl Scheduler {
    pub fn new(clock: Box<dyn Clock>) -> Self {
        Self {
            clock,
            queue: BTreeMap::new(),
            next_id: 0,
        }
    }

    /// The current time from the injected clock.
    pub fn now(&self) -> Duration {
        self.clock.now()
    }

    /// `UIManager:scheduleIn(seconds, action)`. Negative and NaN delays act as zero.
    /// Infinite delays never come due.
    pub fn schedule_in(&mut self, seconds: f64, task: Task) -> TaskId {
        let delay = if seconds.is_nan() || seconds <= 0.0 {
            Duration::ZERO
        } else if seconds.is_finite() {
            Duration::from_secs_f64(seconds)
        } else {
            Duration::MAX
        };
        let due = self.clock.now().checked_add(delay).unwrap_or(Duration::MAX);
        self.schedule_at(due, task)
    }

    /// `UIManager:schedule(sched_time, action)` with an absolute due time on the clock.
    pub fn schedule_at(&mut self, due: Duration, task: Task) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        self.queue.insert((due, id), task);
        TaskId(id)
    }

    /// `UIManager:nextTick(action)`: due now, runs on the next loop iteration.
    pub fn next_tick(&mut self, task: Task) -> TaskId {
        self.schedule_in(0.0, task)
    }

    /// `UIManager:tickAfterNext(action)`: runs the task after one more tick.
    /// Returns the id of the first stage, as KOReader returns the wrapper.
    /// Unscheduling that id before the first stage runs cancels the task. After
    /// the first stage runs, the inner task has its own id and is not cancelled.
    pub fn tick_after_next(&mut self, task: Task) -> TaskId {
        let wrapper: Task = Box::new(move |ctx: &mut Ctx| {
            ctx.scheduler.next_tick(task);
        });
        self.next_tick(wrapper)
    }

    /// `UIManager:unschedule(action)`. Returns false when the id is not queued.
    pub fn unschedule(&mut self, id: TaskId) -> bool {
        let key = self.queue.keys().copied().find(|(_, raw)| *raw == id.0);
        match key {
            Some(key) => {
                self.queue.remove(&key);
                true
            }
            None => false,
        }
    }

    /// `UIManager:_checkTasks()`: removes and returns every task due at `now`,
    /// in due-time order. Tasks scheduled after this call are left for the next call.
    pub fn run_due(&mut self, now: Duration) -> Vec<Task> {
        let limit = self.next_id;
        let due: Vec<Key> = self
            .queue
            .range(..=(now, u64::MAX))
            .filter(|(key, _)| key.1 < limit)
            .map(|(key, _)| *key)
            .collect();
        due.into_iter()
            .filter_map(|key| self.queue.remove(&key))
            .collect()
    }

    /// The earliest due time, for sizing the wait in a loop. `UIManager:getNextTaskTime`.
    pub fn next_due(&self) -> Option<Duration> {
        self.queue.keys().next().map(|(due, _)| *due)
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use std::cell::RefCell;

    fn ctx(clock: &ManualClock) -> Ctx {
        Ctx::new(Box::new(clock.clone()), Rect::new(0, 0, 600, 800))
    }

    fn log_task(log: &Rc<RefCell<Vec<&'static str>>>, name: &'static str) -> Task {
        let log = Rc::clone(log);
        Box::new(move |_ctx: &mut Ctx| log.borrow_mut().push(name))
    }

    #[test]
    fn tasks_run_in_due_order_and_equal_times_are_fifo() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        ctx.scheduler.schedule_in(2.0, log_task(&log, "late"));
        ctx.scheduler.schedule_in(1.0, log_task(&log, "first"));
        ctx.scheduler.schedule_in(1.0, log_task(&log, "second"));
        ctx.scheduler.schedule_in(0.5, log_task(&log, "early"));

        assert_eq!(ctx.run_due(Duration::from_secs(5)), 4);
        assert_eq!(*log.borrow(), vec!["early", "first", "second", "late"]);
        assert!(ctx.scheduler.is_empty());
    }

    #[test]
    fn nothing_runs_before_due() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        ctx.scheduler.schedule_in(1.0, log_task(&log, "later"));
        assert_eq!(ctx.run_due(Duration::from_millis(999)), 0);
        assert_eq!(ctx.scheduler.next_due(), Some(Duration::from_secs(1)));
        assert_eq!(ctx.run_due(Duration::from_secs(1)), 1);
        assert_eq!(*log.borrow(), vec!["later"]);
    }

    #[test]
    fn unschedule_removes_only_that_task() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        let a = ctx.scheduler.schedule_in(1.0, log_task(&log, "a"));
        let _b = ctx.scheduler.schedule_in(1.0, log_task(&log, "b"));
        assert!(ctx.scheduler.unschedule(a));
        assert!(!ctx.scheduler.unschedule(a));
        assert_eq!(ctx.run_due(Duration::from_secs(1)), 1);
        assert_eq!(*log.borrow(), vec!["b"]);
    }

    #[test]
    fn next_tick_waits_for_the_next_loop_iteration() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&log);
        let inner = Rc::clone(&log);
        // The first task schedules a tick. The clock is frozen, so the tick is
        // due at the same instant, but it must not run in the same run_due call.
        ctx.scheduler.schedule_in(
            0.0,
            Box::new(move |ctx: &mut Ctx| {
                outer.borrow_mut().push("first");
                ctx.scheduler.next_tick(log_task(&inner, "tick"));
            }),
        );
        assert_eq!(ctx.run_due(clock.now()), 1);
        assert_eq!(*log.borrow(), vec!["first"]);
        assert_eq!(ctx.run_due(clock.now()), 1);
        assert_eq!(*log.borrow(), vec!["first", "tick"]);
    }

    #[test]
    fn tick_after_next_runs_after_one_extra_loop() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        ctx.scheduler.tick_after_next(log_task(&log, "after"));
        let mut loops = 0;
        while !ctx.scheduler.is_empty() {
            loops += 1;
            ctx.run_due(clock.now());
        }
        // Stage one runs in the first loop and schedules stage two, which runs in the second.
        assert_eq!(loops, 2);
        assert_eq!(*log.borrow(), vec!["after"]);
    }

    #[test]
    fn unschedule_cancels_tick_after_next_before_first_stage() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        let id = ctx.scheduler.tick_after_next(log_task(&log, "never"));
        assert!(ctx.scheduler.unschedule(id));
        assert!(ctx.scheduler.is_empty());
    }

    #[test]
    fn bad_delays_are_clamped() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        ctx.scheduler.schedule_in(-3.0, log_task(&log, "negative"));
        ctx.scheduler.schedule_in(f64::NAN, log_task(&log, "nan"));
        ctx.scheduler.schedule_in(f64::INFINITY, log_task(&log, "never"));
        assert_eq!(ctx.run_due(Duration::ZERO), 2);
        assert_eq!(ctx.scheduler.len(), 1);
    }

    #[test]
    fn system_clock_moves_forward() {
        let clock = SystemClock::new();
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }
}
