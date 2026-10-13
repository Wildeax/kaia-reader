//! Embedded LuaJIT and the KOReader compatibility layer (R7, stage L1).
//!
//! [`LuaHost`] runs KOReader's own Lua modules in one LuaJIT state. The host
//! sets the module paths and the startup globals that KOReader's modules need,
//! and nothing that starts a UI. The stage plan is in
//! `docs/kaia/specs/LUA_COMPAT.md`.
//!
//! [`LuaUi`] runs KOReader widgets in Kaia windows, on top of the host.
//!
//! Linking: the host uses KOReader's `libs/libluajit.dylib` (or `.so`), the
//! library KOReader's native modules bind to. See `build.rs`, and run
//! `. rust/kaia-lua/env.sh` before cargo.

mod convert;
mod error;
mod host;
mod paths;
mod reader;
mod ui;

pub use convert::MAX_DEPTH;
pub use error::Error;
pub use host::LuaHost;
pub use paths::LuaPaths;
pub use reader::{LibraryHost, ReaderHost};
pub use ui::{LuaUi, PluginOptions, PluginStatus, UiConfig};

/// The LuaJIT version that `build.rs` read from KOReader's `luajit.h`, for
/// example "LuaJIT 2.1.1783773675". A test compares it with `jit.version`.
pub const LUAJIT_VERSION: &str = env!("KAIA_LUAJIT_VERSION");
