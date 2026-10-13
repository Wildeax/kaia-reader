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
        }
    }
}

impl SysfsWriter for FakeIo {
    fn write(&mut self, device_path: &str, value: &str) -> io::Result<()> {
        self.io.write(device_path, value)
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
