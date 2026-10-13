//! A single-threaded task queue mirroring `UIManager`'s scheduling
//! (`frontend/ui/uimanager.lua`): `schedule`, `scheduleIn`, `nextTick`,
//! `tickAfterNext`, `unschedule`, `_checkTasks`.
//!
//! Ordering guarantees, as in `UIManager`:
//! - Tasks run in order of due time.
//! - Tasks with the same due time run in the order they were scheduled.
//! - A task scheduled while due tasks run can run in the same pass when due.
//!
//! Tasks are boxed closures that receive `&mut Ctx`. `run_due` removes the due
//! tasks from the queue and returns them. `Ctx::run_due` pops one task at a
//! time so a task can unschedule another due task.

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

    /// Returns at most one due task. The vector return type keeps existing
    /// callers source compatible. Call again after running the task, since it
    /// may change the queue.
    pub fn run_due(&mut self, now: Duration) -> Vec<Task> {
        self.pop_due(now).into_iter().collect()
    }

    /// Removes the next due task. Call again after running it, since that task
    /// may change the queue.
    pub fn pop_due(&mut self, now: Duration) -> Option<Task> {
        let key = self.queue.range(..=(now, u64::MAX)).next().map(|(key, _)| *key)?;
        self.queue.remove(&key)
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
    fn due_task_can_unschedule_another_due_task() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        let b_id = Rc::new(Cell::new(None));
        let a_log = Rc::clone(&log);
        let a_b_id = Rc::clone(&b_id);
        ctx.scheduler.schedule_at(
            Duration::ZERO,
            Box::new(move |ctx| {
                a_log.borrow_mut().push("a");
                assert!(ctx.scheduler.unschedule(a_b_id.get().unwrap()));
            }),
        );
        b_id.set(Some(
            ctx.scheduler.schedule_at(Duration::ZERO, log_task(&log, "b")),
        ));
        assert_eq!(ctx.run_due(Duration::ZERO), 1);
        assert_eq!(*log.borrow(), vec!["a"]);
        assert!(!ctx.scheduler.unschedule(b_id.get().unwrap()));
    }

    #[test]
    fn scheduler_run_due_leaves_later_due_tasks_queued() {
        let mut scheduler = Scheduler::new(Box::new(ManualClock::new()));
        scheduler.schedule_at(Duration::ZERO, Box::new(|_| {}));
        let second = scheduler.schedule_at(Duration::ZERO, Box::new(|_| {}));
        assert_eq!(scheduler.run_due(Duration::ZERO).len(), 1);
        assert!(scheduler.unschedule(second));
        assert!(scheduler.is_empty());
    }

    #[test]
    fn task_scheduled_due_during_a_pass_runs_in_that_pass() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&log);
        let inner = Rc::clone(&log);
        // The frozen clock makes the new task due during the same pass.
        ctx.scheduler.schedule_in(
            0.0,
            Box::new(move |ctx: &mut Ctx| {
                outer.borrow_mut().push("first");
                ctx.scheduler.next_tick(log_task(&inner, "tick"));
            }),
        );
        assert_eq!(ctx.run_due(clock.now()), 2);
        assert_eq!(*log.borrow(), vec!["first", "tick"]);
    }

    #[test]
    fn tick_after_next_runs_when_its_wrapper_is_due() {
        let clock = ManualClock::new();
        let mut ctx = ctx(&clock);
        let log = Rc::new(RefCell::new(Vec::new()));
        ctx.scheduler.tick_after_next(log_task(&log, "after"));
        let mut loops = 0;
        while !ctx.scheduler.is_empty() {
            loops += 1;
            ctx.run_due(clock.now());
        }
        assert_eq!(loops, 1);
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
