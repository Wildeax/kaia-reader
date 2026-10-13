//! Replay scripts: one touch or poll step per line, for gesture tests.
//!
//! Format. Blank lines and lines starting with `#` are ignored. Times are whole
//! milliseconds, and each command is case-sensitive:
//!
//! ```text
//! down 0 500 800 0      # slot x y time_ms: first touch of a contact
//! move 0 540 800 100    # slot x y time_ms: move of a contact
//! up   0 540 800 120    # slot x y time_ms: lift of a contact
//! poll 900              # fire timers due at time_ms
//! ```

use kaia_gfx::Point;

use crate::detector::{GestureConfig, GestureDetector};
use crate::event::{Phase, TouchEvent};
use crate::gesture::Gesture;

/// One step of a replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Touch(TouchEvent),
    /// Fire timers due at this time, in microseconds.
    Poll(u64),
}

/// A malformed script line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    /// 1-based line number.
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "script line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ScriptError {}

/// Parses a replay script into steps.
pub fn parse_script(script: &str) -> Result<Vec<Step>, ScriptError> {
    let mut steps = Vec::new();
    for (index, raw) in script.lines().enumerate() {
        let line = index + 1;
        let text = raw.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let err = |message: &str| ScriptError {
            line,
            message: message.to_string(),
        };
        let fields: Vec<&str> = text.split_whitespace().collect();
        let step = match fields.as_slice() {
            ["poll", time] => Step::Poll(ms_field(time).ok_or_else(|| err("bad poll time"))?),
            [cmd @ ("down" | "move" | "up"), slot, x, y, time] => {
                let slot = slot
                    .parse::<u8>()
                    .map_err(|_| err("bad slot, expected 0 to 255"))?;
                let x = x.parse::<i32>().map_err(|_| err("bad x"))?;
                let y = y.parse::<i32>().map_err(|_| err("bad y"))?;
                let time_us = ms_field(time).ok_or_else(|| err("bad time"))?;
                let phase = match *cmd {
                    "down" => Phase::Down,
                    "move" => Phase::Move,
                    _ => Phase::Up,
                };
                Step::Touch(TouchEvent {
                    slot,
                    phase,
                    pos: Point::new(x, y),
                    time_us,
                })
            }
            _ => return Err(err("expected down, move, up or poll with the right fields")),
        };
        steps.push(step);
    }
    Ok(steps)
}

fn ms_field(text: &str) -> Option<u64> {
    text.parse::<u64>().ok().map(|ms| ms * 1000)
}

/// Runs steps through a detector and returns every gesture in order.
pub fn run(detector: &mut GestureDetector, steps: &[Step]) -> Vec<Gesture> {
    let mut out = Vec::new();
    for step in steps {
        match *step {
            Step::Touch(ev) => out.extend(detector.feed(ev)),
            Step::Poll(now_us) => out.extend(detector.poll(now_us)),
        }
    }
    out
}

/// Replays a script with the default configuration.
///
/// Panics on a malformed script. Use [`parse_script`] to handle the error instead.
pub fn replay(script: &str) -> Vec<Gesture> {
    replay_with(GestureConfig::default(), script)
}

/// Replays a script with the given configuration. Panics on a malformed script.
pub fn replay_with(config: GestureConfig, script: &str) -> Vec<Gesture> {
    let steps = parse_script(script).unwrap_or_else(|e| panic!("{e}"));
    run(&mut GestureDetector::new(config), &steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_touch_and_poll_lines() {
        let steps = parse_script("# comment\n\ndown 0 500 800 0\nup 0 500 800 120\npoll 900\n").unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps[0],
            Step::Touch(TouchEvent {
                slot: 0,
                phase: Phase::Down,
                pos: Point::new(500, 800),
                time_us: 0,
            })
        );
        assert_eq!(
            steps[1],
            Step::Touch(TouchEvent {
                slot: 0,
                phase: Phase::Up,
                pos: Point::new(500, 800),
                time_us: 120_000,
            })
        );
        assert_eq!(steps[2], Step::Poll(900_000));
    }

    #[test]
    fn reports_the_line_number_of_a_bad_command() {
        let err = parse_script("down 0 1 1 0\nsideways 0 1 1 0\n").unwrap_err();
        assert_eq!(err.line, 2);
        let err = parse_script("down 300 1 1 0").unwrap_err();
        assert_eq!(err.line, 1);
        assert!(err.to_string().starts_with("script line 1:"));
    }
}
