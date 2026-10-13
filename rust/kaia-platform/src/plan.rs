//! Step lists for device sequences.
//!
//! KOReader runs its suspend, standby and Wi-Fi sequences as chains of
//! sysfs writes, sleeps and shell calls, with UIManager scheduling the
//! sleeps. Kaia builds the same chain as a [`Plan`] of [`Step`]s. Tests check
//! the plan. [`execute`] runs it against any [`SysfsWriter`] and [`Runner`].
//!
//! Each step names the KOReader line it comes from in `source`. No plan has
//! run on a Libra Colour yet, so every step is unverified on hardware. The
//! doc comment of each builder says which parts differ from KOReader.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::sysfs::{Runner, SysfsWriter};

/// One effect in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write `value` to a device path.
    Write { path: String, value: String },
    /// Sleep for the given time.
    Sleep(Duration),
    /// Run a program and wait for it.
    Run {
        /// Program path or name (looked up on `PATH` when it has no slash).
        program: String,
        /// Arguments.
        args: Vec<String>,
        /// Working directory for the child process.
        cwd: Option<PathBuf>,
        /// Extra environment variables for the child process.
        env: Vec<(String, String)>,
    },
}

/// What happens when a step fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnError {
    /// Continue with the next step. KOReader ignores the result.
    Ignore,
    /// Stop the plan. KOReader returns early and retries later.
    Abort,
    /// Run these steps, then stop the plan. Used for the state-extended reset.
    Recover(Vec<Step>),
}

/// One step with its failure rule and its KOReader source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The effect.
    pub action: Action,
    /// The failure rule.
    pub on_error: OnError,
    /// KOReader file and line this step is ported from, for example `frontend/device/kobo/device.lua:1395`.
    pub source: &'static str,
}

impl Step {
    /// A sysfs write that ignores failures.
    pub fn write(path: &str, value: &str, source: &'static str) -> Self {
        Step {
            action: Action::Write {
                path: path.to_string(),
                value: value.to_string(),
            },
            on_error: OnError::Ignore,
            source,
        }
    }

    /// A sleep.
    pub fn sleep(duration: Duration, source: &'static str) -> Self {
        Step {
            action: Action::Sleep(duration),
            on_error: OnError::Ignore,
            source,
        }
    }

    /// A program run that ignores failures.
    pub fn run(
        program: &str,
        args: &[&str],
        cwd: Option<&Path>,
        env: &[(String, String)],
        source: &'static str,
    ) -> Self {
        Step {
            action: Action::Run {
                program: program.to_string(),
                args: args.iter().map(|a| a.to_string()).collect(),
                cwd: cwd.map(Path::to_path_buf),
                env: env.to_vec(),
            },
            on_error: OnError::Ignore,
            source,
        }
    }

    /// Replaces the failure rule.
    pub fn on_error(mut self, rule: OnError) -> Self {
        self.on_error = rule;
        self
    }
}

/// An ordered list of steps, or a reason why nothing will run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// The steps, in order.
    pub steps: Vec<Step>,
    /// Set when the sequence is skipped. KOReader names the same cases in comments.
    pub skipped: Option<&'static str>,
}

impl Plan {
    /// A plan with these steps.
    pub fn new(steps: Vec<Step>) -> Self {
        Plan { steps, skipped: None }
    }

    /// A plan that does nothing and says why.
    pub fn skipped(reason: &'static str) -> Self {
        Plan {
            steps: Vec::new(),
            skipped: Some(reason),
        }
    }

    /// Appends the steps of `other`. Skipped plans stay skipped.
    pub fn then(mut self, other: Plan) -> Self {
        if self.skipped.is_none() {
            self.skipped = other.skipped;
        }
        self.steps.extend(other.steps);
        self
    }

    /// Whether the plan was skipped.
    pub fn is_skipped(&self) -> bool {
        self.skipped.is_some()
    }
}

/// The result of running a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Every step ran, or failures were ignored.
    Completed,
    /// The plan was skipped before running.
    Skipped(&'static str),
    /// A step with `Abort` or `Recover` failed.
    Failed {
        /// Index of the failing step in the plan.
        step: usize,
        /// Whether a `Recover` list ran.
        recovered: bool,
    },
}

struct Failure {
    index: usize,
    recovered: bool,
}

/// Runs a plan. `sleep` receives every sleep step, so the caller can wait or record.
pub fn execute<IO>(plan: &Plan, io: &mut IO, sleep: &mut dyn FnMut(Duration)) -> Outcome
where
    IO: SysfsWriter + Runner,
{
    if let Some(reason) = plan.skipped {
        return Outcome::Skipped(reason);
    }
    match run_steps(&plan.steps, io, sleep) {
        Ok(()) => Outcome::Completed,
        Err(f) => Outcome::Failed {
            step: f.index,
            recovered: f.recovered,
        },
    }
}

fn run_steps<IO>(steps: &[Step], io: &mut IO, sleep: &mut dyn FnMut(Duration)) -> Result<(), Failure>
where
    IO: SysfsWriter + Runner,
{
    for (index, step) in steps.iter().enumerate() {
        if perform(&step.action, io, sleep) {
            continue;
        }
        match &step.on_error {
            OnError::Ignore => {}
            OnError::Abort => {
                return Err(Failure {
                    index,
                    recovered: false,
                })
            }
            OnError::Recover(recovery) => {
                // Recovery steps are plain writes and runs. Their failures are ignored.
                let _ = run_steps(recovery, io, sleep);
                return Err(Failure {
                    index,
                    recovered: true,
                });
            }
        }
    }
    Ok(())
}

/// Returns true when the action succeeded.
fn perform<IO>(action: &Action, io: &mut IO, sleep: &mut dyn FnMut(Duration)) -> bool
where
    IO: SysfsWriter + Runner,
{
    match action {
        Action::Write { path, value } => io.write(path, value).is_ok(),
        Action::Sleep(duration) => {
            sleep(*duration);
            true
        }
        Action::Run {
            program,
            args,
            cwd,
            env,
        } => matches!(io.run(program, args, cwd.as_deref(), env), Ok(true)),
    }
}
