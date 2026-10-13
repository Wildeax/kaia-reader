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
pub trait SysfsWriter {
    /// Writes `value` to the device path. Errors mean the kernel refused the write.
    fn write(&mut self, device_path: &str, value: &str) -> io::Result<()>;
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

impl SysfsWriter for SystemIo {
    fn write(&mut self, device_path: &str, value: &str) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(self.root.path(device_path))?;
        file.write_all(value.as_bytes())
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
