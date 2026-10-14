//! The Kaia app shell: the main loop, navigation, services and the `kaia` binary.
//!
//! See `docs/kaia/specs/UI_TOOLKIT.md` ("App loop contract") and
//! `docs/kaia/specs/APP_UX.md`, and the README in this crate for the layout.
//!
//! - [`App`] runs the loop over a [`Backend`]: a display that also yields input.
//! - [`Router`] keeps the stack of screens. The root is Home, Library, Notes or
//!   Settings. Pushed screens sit above it.
//! - [`Surface`] is one screen. Each one lives in `surfaces/<name>/` and is
//!   registered in [`surfaces::create`].
//! - [`Command`] parses the global action ids (`nav:*`, `open_book:*`, `back`, `quit`).
//!   Surfaces parse their own ids.
//! - [`Services`] holds the platform, settings, library index, paths, theme and assets.
//! - [`HeadlessBackend`] and [`ScriptInput`] drive the loop without a window.

pub mod app;
pub mod backend;
pub mod command;
pub mod covers;
pub mod i18n;
pub mod refresh;
pub mod router;
pub mod scan;
pub mod services;
pub mod slider;
pub mod surface;
pub mod surfaces;
pub mod sync;

pub use app::{App, AppOptions, Flow, FrameStats};
pub use backend::{Backend, HeadlessBackend, ScriptError, ScriptInput};
pub use command::{BookRef, Command};
pub use router::Router;
pub use scan::{LibraryScan, ScanMessage};
pub use services::{resources_dir, Services, ServicesError, RESOURCES_ENV};
pub use surface::{Nav, Outcome, Surface, SurfaceEnv};
pub use surfaces::Route;

mod durable;

mod sync_queue;

mod book_index;

#[cfg(feature = "lua")]
pub mod plugins;

pub mod nickel;

pub mod lua_import;
