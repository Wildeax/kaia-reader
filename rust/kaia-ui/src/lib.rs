//! Retained widget toolkit for the Kaia UI. See `docs/kaia/specs/UI_TOOLKIT.md`.
//!
//! A window owns one tree of widgets. Each widget lays out, paints and reacts
//! to events through the contexts in [`widget`]. [`Window`] puts a tree on the
//! `kaia_events` window stack and turns dirty rectangles into refresh requests.
//! Everything runs on one thread and draws into `kaia_gfx` pixmaps, so the same
//! code renders headless for golden tests.

pub mod assets;
pub mod components;
pub mod composite;
pub mod controls;
pub mod foreign;
pub mod geometry;
pub mod harness;
mod pod;
pub mod theme;
pub mod widget;
pub mod window;

pub use assets::{AssetError, Assets, IconCache, IconError, SharedAssets};
pub use geometry::{Alignment, Constraints, Insets};
pub use harness::{render, Harness};
pub use pod::{WidgetId, WidgetPod};
pub use theme::{FontSpec, Scale, Theme, Tone};
pub use widget::{AppAction, EventCx, Handled, LayoutCx, Out, PaintCx, PaintHint, UiEvent, UpdateCx, Widget};
pub use window::Window;
