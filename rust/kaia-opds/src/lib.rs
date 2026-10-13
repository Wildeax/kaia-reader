//! OPDS catalog browsing, search and downloads.
//!
//! This crate ports KOReader's `plugins/opds.koplugin`. It reads OPDS 1.x
//! (Atom) and OPDS 2.0 (JSON) feeds, OpenSearch descriptions, and OPDS-PSE
//! page links. Nothing reaches the network until a [`Client`] method is
//! called. The parsers do no I/O.
//!
//! ```no_run
//! use kaia_opds::{default_catalogs, Client};
//!
//! let catalog = &default_catalogs()[0];
//! let client = Client::for_catalog(catalog)?;
//! let feed = client.fetch(&catalog.url)?;
//! for entry in &feed.entries {
//!     println!("{}", entry.title);
//! }
//! # Ok::<(), kaia_opds::Error>(())
//! ```

mod catalog;
mod client;
mod error;
mod files;
mod model;
mod opds1;
mod opds2;
mod parse;
mod rel;
mod resolve;
mod search;
mod text;
mod xml;

pub use catalog::{default_catalogs, normalize_url, Catalog};
pub use client::{Client, DEFAULT_MAX_DOWNLOAD};
pub use error::{Error, Result};
pub use files::{file_type, is_supported_mime};
pub use model::{Acquisition, Entry, Facet, Feed, FeedKind, Link, Pse};
pub use parse::{parse_feed, parse_search_description};
