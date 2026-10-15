//! Composite components: screens and windows built from the core, the
//! content components and the controls. See `docs/kaia/specs/UI_TOOLKIT.md`.
//!
//! Components that build child widgets from tokens take the [`Theme`] in
//! their constructor. Components that only draw from tokens take nothing.

mod cover;
mod dialog;
mod header;
mod list_row;
mod menu;
mod navbar;
mod paged;
mod scroll_list;
mod settings_dialog;
mod sheet;
mod toast;

pub use cover::{trim_blank_margins, BookInfo, BookTile, CoverImage};
pub use dialog::{Dialog, DialogButton};
pub use header::Header;
pub use list_row::{ListRow, Trailing};
pub use menu::Menu;
pub use navbar::{NavBar, NavItem};
pub use paged::{Grid, PageLabels, PagedList};
pub use scroll_list::ScrollList;
pub use settings_dialog::{SettingsDialog, SettingsIndexRow};
pub use sheet::SheetPanel;
pub use toast::Toast;
