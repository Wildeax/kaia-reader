//! Reading statistics compatible with KOReader's statistics plugin.
//!
//! The database is KOReader's `statistics.sqlite3` (schema version 20221111, from
//! `plugins/statistics.koplugin/main.lua`). KOReader and the tools that read the
//! file keep working on it. Kaia writes the same tables and the same `page_stat`
//! view.
//!
//! Time is Unix seconds, passed in by the caller. Day boundaries are local
//! midnight, computed by SQLite.
//!
//! KOReader keys statistics rows by the sidecar's stored `partial_md5_checksum`
//! and `display_title`, together with authors. Pass the sidecar hash without
//! recomputing it and the document's display title to [`Stats::start_book_with_info`].
//! The recorder also checks legacy MuPDF rows with a trailing NUL in the title
//! or authors before it creates a new row.

mod error;
mod import;
mod query;
mod record;
mod schema;

pub use error::{Error, Result};
pub use import::ImportReport;
pub use query::{BookEstimate, BookSummary, BookTotals, DayTotal, Streaks, TimeLeft, Totals, Weekday};
pub use record::Stats;
pub use record::{BookInfo, StatsOptions};

/// Schema version written by KOReader (`DB_SCHEMA_VERSION` in `main.lua`).
pub const DB_SCHEMA_VERSION: i64 = 20221111;

/// Shortest stay that counts, in seconds (`DEFAULT_MIN_READ_SEC`).
pub const DEFAULT_MIN_READ_SEC: i64 = 5;

/// Longest stay that counts, in seconds (`DEFAULT_MAX_READ_SEC`). A longer stay counts this much.
pub const DEFAULT_MAX_READ_SEC: i64 = 120;
