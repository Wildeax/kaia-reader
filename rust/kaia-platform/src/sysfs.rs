//! Device file access and command execution.
//!
//! Paths passed around this crate are device-absolute (`/sys/power/state`).
//! [`Root`] maps them under a base directory, so tests can build a fake
//! device tree in a temporary directory. On the device the base is `/`.
//!
//! Writes follow KOReader's `ffiUtil.writeToSysfs` (`base/ffi/util.lua:526`),
//! which opens the file without creating it and writes the value without a
//! trailing newline. Truncation is enabled here. The kernel ignores it on
//! sysfs attributes, and it keeps fake files in tests correct.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ioctl;

/// A base directory that device-absolute paths are resolved against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    base: PathBuf,
}

impl Root {
    /// Creates a root at `base`. Use `/` on a device.
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Root { base: base.into() }
    }

    /// The base directory.
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Maps a device-absolute path to a path under the base.
    pub fn path(&self, device_path: &str) -> PathBuf {
        self.base.join(device_path.trim_start_matches('/'))
    }

    /// Whether the device path exists under the base.
    pub fn exists(&self, device_path: &str) -> bool {
        self.path(device_path).exists()
    }

    /// The first line of a file, without its line ending. `None` when the file
    /// cannot be read. Mirrors `BasePowerD:read_str_file` (`frontend/device/generic/powerd.lua:199`).
    pub fn read_line(&self, device_path: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.path(device_path)).ok()?;
        Some(text.lines().next().unwrap_or("").trim_end().to_string())
    }

    /// The whole file as text. `None` when the file cannot be read.
    pub fn read_text(&self, device_path: &str) -> Option<String> {
        std::fs::read_to_string(self.path(device_path)).ok()
    }

    /// The leading integer of a file. Mirrors `BasePowerD:read_int_file`
    /// (`frontend/device/generic/powerd.lua:177`), but returns `None` instead of 0.
    pub fn read_int(&self, device_path: &str) -> Option<i64> {
        let line = self.read_line(device_path)?;
        line.split_whitespace().next()?.parse().ok()
    }
}

/// Writes a value to a device file. Tests implement this to record writes.
///
/// The ioctl methods are the faster or more capable paths that KOReader prefers
/// over sysfs (RTC alarm, system clock, `ntx_io` frontlight). They are default
/// methods that report [`io::ErrorKind::Unsupported`], so a writer that only
/// handles files still works and callers fall back to sysfs. [`SystemIo`]
/// implements them with the calls in [`crate::ioctl`]. Tests implement them to
/// record the calls.
pub trait SysfsWriter {
    /// Records a device-plan step. Fake writers can capture the sequence.
    fn trace(&mut self, _message: &str) {}

    /// Writes `value` to the device path. Errors mean the kernel refused the write.
    fn write(&mut self, device_path: &str, value: &str) -> io::Result<()>;

    /// Sets the RTC alarm to `epoch` (UTC) and enables it, with `RTC_WKALM_SET`
    /// on the RTC device node `device_path`. Overwrites an alarm that is set.
    fn rtc_set_alarm(&mut self, device_path: &str, epoch: u64) -> io::Result<()> {
        let _ = (device_path, epoch);
        Err(unsupported())
    }

    /// Turns the RTC alarm interrupt off (`RTC_AIE_OFF`).
    fn rtc_disable_alarm(&mut self, device_path: &str) -> io::Result<()> {
        let _ = device_path;
        Err(unsupported())
    }

    /// Copies the RTC time to the system clock (`RTC:HCToSys`).
    fn rtc_to_system_clock(&mut self, device_path: &str) -> io::Result<()> {
        let _ = device_path;
        Err(unsupported())
    }

    /// Whether the `ntx_io` node at `device_path` opens, which means the
    /// brightness ioctl can be used.
    fn ntx_available(&mut self, device_path: &str) -> bool {
        let _ = device_path;
        false
    }

    /// Sets the frontlight brightness (0 to 100) with ioctl 241 on the `ntx_io`
    /// node at `device_path`.
    fn ntx_set_brightness(&mut self, device_path: &str, value: u8) -> io::Result<()> {
        let _ = (device_path, value);
        Err(unsupported())
    }
}

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "no ioctl access in this writer")
}

/// Runs external programs. Tests implement this to record commands.
pub trait Runner {
    /// Runs a program and returns whether it exited with status 0.
    fn run(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> io::Result<bool>;

    /// Runs a program and returns its trimmed standard output. Standard error is discarded.
    fn output(&mut self, program: &str, args: &[&str]) -> io::Result<String>;
}

/// The real device I/O: sysfs writes under a [`Root`] and host processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemIo {
    root: Root,
}

impl SystemIo {
    /// Creates the real I/O for `root`. Commands always run on the host.
    pub fn new(root: Root) -> Self {
        SystemIo { root }
    }

    /// The root that writes are resolved against.
    pub fn root(&self) -> &Root {
        &self.root
    }
}

/// Writes a power diagnostic to the launcher's Rust log and syncs that file.
/// A terminal or pipe has no file to sync. Logging must not prevent recovery.
pub fn power_trace(message: &str) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "kaia: power: {message}");
    let _ = stderr.flush();
    #[cfg(target_os = "linux")]
    if let Ok(file) = OpenOptions::new().write(true).open("/proc/self/fd/2") {
        if file.metadata().is_ok_and(|m| m.is_file()) {
            let _ = file.sync_all();
        }
    }
}

impl SysfsWriter for SystemIo {
    fn trace(&mut self, message: &str) {
        if self.root.base() == Path::new("/") {
            power_trace(message);
        }
    }

    fn write(&mut self, device_path: &str, value: &str) -> io::Result<()> {
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(self.root.path(device_path))?;
            file.write_all(value.as_bytes())
        })();
        if let Err(error) = &result {
            self.trace(&format!("write {device_path}={value}: {error}"));
        }
        result
    }

    fn rtc_set_alarm(&mut self, device_path: &str, epoch: u64) -> io::Result<()> {
        ioctl::rtc_set_alarm(&self.root.path(device_path), epoch)
    }

    fn rtc_disable_alarm(&mut self, device_path: &str) -> io::Result<()> {
        ioctl::rtc_disable_alarm(&self.root.path(device_path))
    }

    fn rtc_to_system_clock(&mut self, device_path: &str) -> io::Result<()> {
        ioctl::set_system_clock_from_rtc(&self.root.path(device_path))
    }

    fn ntx_available(&mut self, device_path: &str) -> bool {
        ioctl::ntx_available(&self.root.path(device_path))
    }

    fn ntx_set_brightness(&mut self, device_path: &str, value: u8) -> io::Result<()> {
        ioctl::ntx_set_brightness(&self.root.path(device_path), value)
    }
}

impl Runner for SystemIo {
    fn run(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> io::Result<bool> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        Ok(cmd.status()?.success())
    }

    fn output(&mut self, program: &str, args: &[&str]) -> io::Result<String> {
        let out = Command::new(program).args(args).stderr(Stdio::null()).output()?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}
