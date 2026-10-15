//! The PIN flows as a state machine: the digits typed so far, the step, the notice
//! and the wrong-try rule. The pad shows it and acts on what it returns. The
//! state holds no clock. Callers pass the Unix time in seconds, so tests can use
//! a fake one.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::store::{self, Attempts, MAX_DIGITS, MIN_DIGITS};

/// Wrong tries that make one round. Each round ends with a wait.
pub const ROUND: u32 = 5;
/// The wait after the first round, in seconds. It doubles after each later round.
pub const FIRST_WAIT: u64 = 30;
/// The longest wait, in seconds.
pub const MAX_WAIT: u64 = 15 * 60;

/// The wait in seconds that follows `wrong` wrong tries in total. Zero when no wait is due.
pub fn wait_after(wrong: u32) -> u64 {
    if wrong == 0 || !wrong.is_multiple_of(ROUND) {
        return 0;
    }
    let round = wrong / ROUND;
    FIRST_WAIT
        .checked_shl(round - 1)
        .unwrap_or(u64::MAX)
        .min(MAX_WAIT)
}

/// The current Unix time in seconds.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What the pad is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// The lock after a wake or a start. The right PIN unlocks.
    Unlock,
    /// Set a PIN for the first time. No old PIN is asked.
    Set,
    /// Change the PIN. The old PIN is asked first.
    Change,
    /// Turn the lock off. The PIN is asked first.
    Off,
}

/// The step the pad is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Check the digits against the stored PIN.
    Verify,
    /// Choose a new PIN.
    New,
    /// Type the new PIN again.
    Confirm,
}

/// A line the pad shows under its title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    /// A wrong PIN. `tries_left` counts the tries before the next wait.
    Wrong { tries_left: u32 },
    /// A wait runs. `seconds` is what is left.
    Waiting { seconds: u64 },
    /// The digits are shorter than the shortest PIN.
    TooShort,
    /// The second entry does not match the first.
    Mismatch,
}

/// A flow that ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    /// The right PIN was typed at the lock.
    Unlocked,
    /// A new PIN was stored.
    Set,
    /// The PIN was changed.
    Changed,
    /// The lock was turned off and its files removed.
    Cleared,
}

/// One PIN flow. It holds the digits typed so far, which are never written out.
/// Do not add a `Debug` derive: it would print them.
pub struct Session {
    dir: PathBuf,
    purpose: Purpose,
    step: Step,
    digits: String,
    first: String,
    notice: Option<Notice>,
    attempts: Attempts,
}

impl Session {
    /// A flow in `dir`. It starts with the wait that is still running, if any.
    pub fn new(dir: &Path, purpose: Purpose, now: u64) -> Self {
        let mut session = Self {
            dir: dir.to_path_buf(),
            purpose,
            step: Step::Verify,
            digits: String::new(),
            first: String::new(),
            notice: None,
            attempts: Attempts::default(),
        };
        session.reload(now);
        session
    }

    /// Reads the wrong tries again and drops the digits typed so far. The lock calls
    /// it on each wake.
    pub fn reload(&mut self, now: u64) {
        self.attempts = store::load_attempts(&self.dir);
        self.digits.clear();
        self.first.clear();
        self.step = if self.purpose == Purpose::Set {
            Step::New
        } else {
            Step::Verify
        };
        let left = self.wait_left(now);
        self.notice = (left > 0).then_some(Notice::Waiting { seconds: left });
    }

    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    pub fn step(&self) -> Step {
        self.step
    }

    /// How many digits are typed.
    pub fn digit_count(&self) -> usize {
        self.digits.len()
    }

    pub fn notice(&self) -> Option<Notice> {
        self.notice
    }

    /// Seconds left in the wait at `now`. Zero when no wait runs. A wait longer than
    /// `MAX_WAIT` is cut to it, so a clock that moved back cannot hold the lock for
    /// longer than one wait.
    pub fn wait_left(&self, now: u64) -> u64 {
        if self.step != Step::Verify {
            return 0;
        }
        self.attempts.until.saturating_sub(now).min(MAX_WAIT)
    }

    /// Adds a digit. Presses past the longest PIN are ignored. During a wait the press
    /// only updates the notice with the time left.
    pub fn press(&mut self, digit: char, now: u64) {
        if !digit.is_ascii_digit() {
            return;
        }
        let left = self.wait_left(now);
        if left > 0 {
            self.notice = Some(Notice::Waiting { seconds: left });
            return;
        }
        if self.digits.len() >= MAX_DIGITS {
            return;
        }
        self.digits.push(digit);
        self.notice = None;
    }

    /// Removes the last digit.
    pub fn delete(&mut self) {
        self.digits.pop();
    }

    /// Acts on the digits typed. Returns the flow that ended, if any. The digits are
    /// cleared on every path. An error comes from the files and carries no digits.
    pub fn submit(&mut self, now: u64) -> io::Result<Option<Done>> {
        let digits = std::mem::take(&mut self.digits);
        match self.step {
            Step::Verify => self.verify(&digits, now),
            Step::New => {
                if !store::valid_digits(&digits) {
                    self.notice = Some(Notice::TooShort);
                    return Ok(None);
                }
                self.first = digits;
                self.step = Step::Confirm;
                self.notice = None;
                Ok(None)
            }
            Step::Confirm => {
                if digits == self.first {
                    self.first.clear();
                    store::save(&self.dir, &digits)?;
                    self.attempts = Attempts::default();
                    Ok(Some(if self.purpose == Purpose::Change {
                        Done::Changed
                    } else {
                        Done::Set
                    }))
                } else {
                    self.first.clear();
                    self.step = Step::New;
                    self.notice = Some(Notice::Mismatch);
                    Ok(None)
                }
            }
        }
    }

    fn verify(&mut self, digits: &str, now: u64) -> io::Result<Option<Done>> {
        // The wait comes first: presses during a wait are dropped, so the digits are empty then.
        let left = self.wait_left(now);
        if left > 0 {
            self.notice = Some(Notice::Waiting { seconds: left });
            return Ok(None);
        }
        if digits.len() < MIN_DIGITS {
            self.notice = Some(Notice::TooShort);
            return Ok(None);
        }
        let right = store::load(&self.dir).is_some_and(|record| record.matches(digits));
        if right {
            self.attempts = Attempts::default();
            self.notice = None;
            return match self.purpose {
                Purpose::Unlock => {
                    store::clear_attempts(&self.dir)?;
                    Ok(Some(Done::Unlocked))
                }
                Purpose::Change => {
                    store::clear_attempts(&self.dir)?;
                    self.step = Step::New;
                    Ok(None)
                }
                Purpose::Off => {
                    store::clear(&self.dir)?;
                    Ok(Some(Done::Cleared))
                }
                Purpose::Set => Ok(None),
            };
        }
        self.attempts.wrong += 1;
        let wait = wait_after(self.attempts.wrong);
        if wait > 0 {
            self.attempts.until = now + wait;
            self.notice = Some(Notice::Waiting { seconds: wait });
        } else {
            self.notice = Some(Notice::Wrong {
                tries_left: ROUND - self.attempts.wrong % ROUND,
            });
        }
        store::save_attempts(&self.dir, self.attempts)?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kaia-pin-session-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        store::save(&dir, "4826").unwrap();
        dir
    }

    fn type_and_submit(s: &mut Session, digits: &str, now: u64) -> io::Result<Option<Done>> {
        for d in digits.chars() {
            s.press(d, now);
        }
        s.submit(now)
    }

    #[test]
    fn the_wait_table_doubles_each_round_up_to_fifteen_minutes() {
        let table: Vec<(u32, u64)> = [1, 4, 5, 6, 9, 10, 15, 20, 25, 30, 35, 500]
            .iter()
            .map(|&w| (w, wait_after(w)))
            .collect();
        assert_eq!(
            table,
            vec![
                (1, 0),
                (4, 0),
                (5, 30),
                (6, 0),
                (9, 0),
                (10, 60),
                (15, 120),
                (20, 240),
                (25, 480),
                (30, 900),
                (35, 900),
                (500, 900),
            ]
        );
    }

    #[test]
    fn five_wrong_pins_wait_thirty_seconds_on_a_fake_clock() {
        let dir = dir("backoff");
        let mut s = Session::new(&dir, Purpose::Unlock, 1_000);
        for _ in 0..4 {
            assert_eq!(type_and_submit(&mut s, "1111", 1_000).unwrap(), None);
        }
        assert_eq!(s.notice(), Some(Notice::Wrong { tries_left: 1 }));
        assert_eq!(s.wait_left(1_000), 0);

        type_and_submit(&mut s, "1111", 1_000).unwrap();
        assert_eq!(s.notice(), Some(Notice::Waiting { seconds: 30 }));
        assert_eq!(s.wait_left(1_029), 1);

        // The right PIN is refused while the wait runs, and the count does not move.
        assert_eq!(type_and_submit(&mut s, "4826", 1_029).unwrap(), None);
        assert_eq!(s.notice(), Some(Notice::Waiting { seconds: 1 }));
        assert_eq!(store::load_attempts(&dir).wrong, 5);

        // After the wait the right PIN unlocks.
        assert_eq!(
            type_and_submit(&mut s, "4826", 1_030).unwrap(),
            Some(Done::Unlocked)
        );
        assert!(!dir.join(store::ATTEMPTS_FILE).exists());
    }

    #[test]
    fn each_later_round_doubles_the_wait() {
        let dir = dir("rounds");
        let mut s = Session::new(&dir, Purpose::Unlock, 0);
        let mut now = 0;
        let mut waits = Vec::new();
        for _round in 0..3 {
            for _ in 0..5 {
                type_and_submit(&mut s, "0000", now).unwrap();
                if let Some(Notice::Waiting { seconds }) = s.notice() {
                    waits.push(seconds);
                    now += seconds;
                }
            }
        }
        assert_eq!(waits, vec![30, 60, 120]);
    }

    #[test]
    fn the_counter_survives_a_restart() {
        let dir = dir("restart");
        let mut first = Session::new(&dir, Purpose::Unlock, 500);
        for _ in 0..5 {
            type_and_submit(&mut first, "1234", 500).unwrap();
        }
        drop(first);

        let restarted = Session::new(&dir, Purpose::Unlock, 510);
        assert_eq!(restarted.notice(), Some(Notice::Waiting { seconds: 20 }));
        assert_eq!(restarted.wait_left(530), 0);
    }

    #[test]
    fn a_clock_that_moves_back_cannot_hold_the_lock_longer_than_one_wait() {
        let dir = dir("clock");
        // Stored: 25 wrong tries and a wait that ends at 10 480.
        store::save_attempts(
            &dir,
            Attempts {
                wrong: 25,
                until: 10_480,
            },
        )
        .unwrap();
        // The clock now reads 6 400, so the stored end is 4 080 s away.
        let s = Session::new(&dir, Purpose::Unlock, 6_400);
        assert_eq!(s.wait_left(6_400), MAX_WAIT);
    }

    #[test]
    fn set_asks_twice_and_stores_a_pin_that_then_unlocks() {
        let dir = dir("set");
        store::clear(&dir).unwrap();
        let mut s = Session::new(&dir, Purpose::Set, 0);
        assert_eq!(s.step(), Step::New);
        assert_eq!(type_and_submit(&mut s, "123", 0).unwrap(), None);
        assert_eq!(s.notice(), Some(Notice::TooShort));
        assert_eq!(type_and_submit(&mut s, "1357", 0).unwrap(), None);
        assert_eq!(s.step(), Step::Confirm);
        assert_eq!(type_and_submit(&mut s, "2468", 0).unwrap(), None);
        assert_eq!(s.step(), Step::New);
        assert_eq!(s.notice(), Some(Notice::Mismatch));
        type_and_submit(&mut s, "1357", 0).unwrap();
        assert_eq!(type_and_submit(&mut s, "1357", 0).unwrap(), Some(Done::Set));
        assert!(store::is_on(&dir));

        let mut lock = Session::new(&dir, Purpose::Unlock, 0);
        assert_eq!(
            type_and_submit(&mut lock, "1357", 0).unwrap(),
            Some(Done::Unlocked)
        );
    }

    #[test]
    fn change_needs_the_old_pin_then_stores_the_new_one() {
        let dir = dir("change");
        let mut s = Session::new(&dir, Purpose::Change, 0);
        assert_eq!(type_and_submit(&mut s, "9999", 0).unwrap(), None);
        assert_eq!(s.step(), Step::Verify);
        assert_eq!(s.notice(), Some(Notice::Wrong { tries_left: 4 }));
        assert_eq!(type_and_submit(&mut s, "4826", 0).unwrap(), None);
        assert_eq!(s.step(), Step::New);
        type_and_submit(&mut s, "11223", 0).unwrap();
        assert_eq!(type_and_submit(&mut s, "11223", 0).unwrap(), Some(Done::Changed));

        let mut old = Session::new(&dir, Purpose::Unlock, 0);
        assert_eq!(type_and_submit(&mut old, "4826", 0).unwrap(), None);
        let mut new = Session::new(&dir, Purpose::Unlock, 0);
        assert_eq!(
            type_and_submit(&mut new, "11223", 0).unwrap(),
            Some(Done::Unlocked)
        );
    }

    #[test]
    fn off_needs_the_pin_and_removes_the_files() {
        let dir = dir("off");
        let mut s = Session::new(&dir, Purpose::Off, 0);
        assert_eq!(type_and_submit(&mut s, "0001", 0).unwrap(), None);
        assert!(store::is_on(&dir));
        assert_eq!(type_and_submit(&mut s, "4826", 0).unwrap(), Some(Done::Cleared));
        assert!(!store::is_on(&dir));
        assert!(store::load(&dir).is_none());
    }

    #[test]
    fn deleting_the_pin_file_removes_the_lock() {
        let dir = dir("recover");
        std::fs::remove_file(dir.join(store::FILE)).unwrap();
        assert!(!store::is_on(&dir));
    }

    #[test]
    fn reload_drops_typed_digits_and_reads_the_wait_again() {
        let dir = dir("reload");
        let mut s = Session::new(&dir, Purpose::Unlock, 0);
        for _ in 0..5 {
            type_and_submit(&mut s, "1111", 0).unwrap();
        }
        s.reload(5);
        assert_eq!(s.notice(), Some(Notice::Waiting { seconds: 25 }));
        // A press during the wait is dropped.
        s.press('4', 5);
        assert_eq!(s.digit_count(), 0);
    }
}
