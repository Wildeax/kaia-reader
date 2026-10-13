//! Offline StarDict dictionary lookup.
//!
//! Reads the StarDict files that KOReader's `sdcv` reads: `.ifo`, `.idx` or `.idx.gz`,
//! `.dict` or dictzip `.dict.dz`, and an optional `.syn`. The index and synonyms load into
//! memory. Definitions are read from disk per lookup, and dictzip files are read by chunk,
//! so a lookup never decompresses the whole data file.
//!
//! ```no_run
//! # fn run() -> Result<(), kaia_dict::Error> {
//! let library = kaia_dict::Library::open_dir("data/dict")?;
//! for entry in library.lookup("example") {
//!     println!("{}: {}", entry.dict_name, entry.definition.plain_text());
//! }
//! # Ok(())
//! # }
//! ```

mod dict;
mod dictzip;
mod error;
mod html;
mod ifo;
mod index;
mod lookup;
#[cfg(test)]
mod testgen;
#[cfg(test)]
mod tests;

pub use dict::Dictionary;
pub use error::Error;
pub use html::strip_html;
pub use ifo::Info;
pub use lookup::{Definition, DefinitionKind, Entry, Library, LookupOptions, Match};
