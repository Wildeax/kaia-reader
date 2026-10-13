//! Shared test helpers: a fake device tree and a fake command runner.

#![allow(dead_code)]

use std::io;
use std::path::{Path, PathBuf};

use kaia_platform::sysfs::{Root, Runner, SysfsWriter, SystemIo};

/// Writes go into a fake device tree on disk. Commands are recorded and never run.
pub struct FakeIo {
    pub io: SystemIo,
    pub runs: Vec<Run>,
    /// Program names whose run should report failure.
    pub failing_programs: Vec<String>,
    /// Every sysfs write that was attempted, in order, including refused ones.
    pub writes: Vec<(String, String)>,
    /// Behave like the RTC sysfs file: refuse a nonzero value while a nonzero alarm is set.
    pub wakealarm_busy_while_set: bool,
    /// Whether the RTC ioctls succeed. They record their calls either way.
    pub rtc_ioctl_works: bool,
    /// Epochs passed to `rtc_set_alarm`.
    pub rtc_alarms: Vec<u64>,
    /// Number of `rtc_disable_alarm` calls.
    pub rtc_disables: u32,
    /// Number of `rtc_to_system_clock` calls.
    pub rtc_clock_syncs: u32,
    /// Whether `ntx_io` opens.
    pub ntx_open: bool,
    /// Whether the brightness ioctl succeeds. It records its calls either way.
    pub ntx_works: bool,
    /// Brightness ioctl calls as (device, value).
    pub ntx_calls: Vec<(String, u8)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

impl FakeIo {
    pub fn new(root: &Path) -> Self {
        FakeIo {
            io: SystemIo::new(Root::new(root)),
            runs: Vec::new(),
            failing_programs: Vec::new(),
            writes: Vec::new(),
            wakealarm_busy_while_set: false,
            rtc_ioctl_works: false,
            rtc_alarms: Vec::new(),
            rtc_disables: 0,
            rtc_clock_syncs: 0,
            ntx_open: false,
            ntx_works: true,
            ntx_calls: Vec::new(),
        }
    }
}

impl SysfsWriter for FakeIo {
    fn write(&mut self, device_path: &str, value: &str) -> io::Result<()> {
        self.writes.push((device_path.to_string(), value.to_string()));
        if self.wakealarm_busy_while_set && device_path == "/sys/class/rtc/rtc0/wakealarm" && value != "0" {
            let current = std::fs::read_to_string(self.io.root().path(device_path)).unwrap_or_default();
            let current = current.trim();
            if !current.is_empty() && current != "0" {
                return Err(io::Error::new(io::ErrorKind::ResourceBusy, "EBUSY"));
            }
        }
        self.io.write(device_path, value)
    }

    fn rtc_set_alarm(&mut self, _device_path: &str, epoch: u64) -> io::Result<()> {
        self.rtc_alarms.push(epoch);
        self.rtc_result()
    }

    fn rtc_disable_alarm(&mut self, _device_path: &str) -> io::Result<()> {
        self.rtc_disables += 1;
        self.rtc_result()
    }

    fn rtc_to_system_clock(&mut self, _device_path: &str) -> io::Result<()> {
        self.rtc_clock_syncs += 1;
        self.rtc_result()
    }

    fn ntx_available(&mut self, _device_path: &str) -> bool {
        self.ntx_open
    }

    fn ntx_set_brightness(&mut self, device_path: &str, value: u8) -> io::Result<()> {
        self.ntx_calls.push((device_path.to_string(), value));
        if self.ntx_works {
            Ok(())
        } else {
            Err(io::Error::other("ioctl refused"))
        }
    }
}

impl FakeIo {
    fn rtc_result(&self) -> io::Result<()> {
        if self.rtc_ioctl_works {
            Ok(())
        } else {
            Err(io::Error::other("no rtc ioctl"))
        }
    }
}

impl Runner for FakeIo {
    fn run(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> io::Result<bool> {
        self.runs.push(Run {
            program: program.to_string(),
            args: args.to_vec(),
            cwd: cwd.map(Path::to_path_buf),
            env: env.to_vec(),
        });
        Ok(!self.failing_programs.iter().any(|p| p == program))
    }

    fn output(&mut self, _program: &str, _args: &[&str]) -> io::Result<String> {
        Err(io::Error::new(io::ErrorKind::NotFound, "not in fake tree"))
    }
}

/// Writes a file under the fake root, creating parent directories.
pub fn put(root: &Path, device_path: &str, text: &str) {
    let path = root.join(device_path.trim_start_matches('/'));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Reads a file under the fake root.
pub fn get(root: &Path, device_path: &str) -> String {
    std::fs::read_to_string(root.join(device_path.trim_start_matches('/'))).unwrap()
}
