//! The Kaia app shell: the main loop, navigation, services and the `kaia` binary.
//!
//! See `docs/kaia/specs/UI_TOOLKIT.md` ("App loop contract") and
//! `docs/kaia/specs/APP_UX.md`.
//!
//! - [`App`] runs the loop over a [`Backend`]: a display that also yields input.
//! - [`Router`] keeps the route stack. The root is Home, Library, Notes or Settings.
//!   Reader, reader overlay, book details and settings sections are pushed above it.
//! - [`Command`] parses the action ids that widgets send (`nav:library`,
//!   `open_book:<id>`, `back`, ...).
//! - [`Services`] holds the platform, settings, library index, paths, theme and assets.
//! - [`surfaces`] builds the screens. They are placeholders until the composite
//!   components land.
//! - [`HeadlessBackend`] and [`ScriptInput`] drive the loop without a window.

pub mod app;
pub mod backend;
pub mod command;
pub mod refresh;
pub mod router;
pub mod scan;
pub mod services;
pub mod surfaces;

pub use app::{App, AppOptions, Flow, FrameStats};
pub use backend::{Backend, HeadlessBackend, ScriptError, ScriptInput};
pub use command::{BookRef, Command, PageStep};
pub use router::{Route, Router};
pub use scan::{LibraryScan, ScanMessage};
pub use services::{resources_dir, Services, ServicesError, RESOURCES_ENV};
