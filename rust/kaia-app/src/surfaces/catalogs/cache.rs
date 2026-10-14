//! The Calibre metadata file in the downloads folder, for the offline search and browse
//! views. A tap opens a book when its file is in the library index.

use std::path::{Path, PathBuf};

use kaia_calibre::metadata::{self, Field, Metadata};
use kaia_i18n::tr;
use kaia_library::Library;

/// What the Calibre view knows about the metadata file.
#[derive(Default)]
pub enum Cache {
    /// The folder has no metadata file. The view offers only the wireless connection.
    #[default]
    Missing,
    Loaded {
        root: PathBuf,
        metadata: Metadata,
    },
    /// The file exists but cannot be used. The text says why.
    Failed(String),
}

impl Cache {
    /// Reads the metadata file in `root`. The reader reads at most `MAX_BYTES` and
    /// refuses a file it cannot parse, so a bad file costs one bounded read.
    pub fn load(root: &Path) -> Self {
        match metadata::load(root) {
            Ok(Some(metadata)) => Self::Loaded {
                root: root.to_path_buf(),
                metadata,
            },
            Ok(None) => Self::Missing,
            Err(metadata::Error::TooLarge) => {
                Self::Failed(tr("The Calibre metadata file is too large to search."))
            }
            Err(_) => Self::Failed(tr("Cannot read the Calibre metadata file.")),
        }
    }

    pub fn metadata(&self) -> Option<&Metadata> {
        match self {
            Self::Loaded { metadata, .. } => Some(metadata),
            _ => None,
        }
    }

    /// The library id of the book at `index` in the metadata, or None when its file is
    /// not in the library. The library resolves links in the path before it looks up
    /// the book.
    pub fn library_id(&self, library: &Library, index: usize) -> Option<i64> {
        let Self::Loaded { root, metadata } = self else {
            return None;
        };
        let book = metadata.books().get(index)?;
        library
            .book_by_path(&root.join(&book.lpath))
            .ok()
            .flatten()
            .map(|found| found.id)
    }
}

/// The name of a browse field, for the view title and the row text.
pub fn field_title(field: Field) -> String {
    match field {
        Field::Author => tr("Browse by author"),
        Field::Series => tr("Browse by series"),
        Field::Tag => tr("Browse by tag"),
    }
}

/// The key that the browse rows use in their action names.
pub fn field_key(field: Field) -> &'static str {
    match field {
        Field::Author => "author",
        Field::Series => "series",
        Field::Tag => "tag",
    }
}

pub fn parse_field(key: &str) -> Option<Field> {
    match key {
        "author" => Some(Field::Author),
        "series" => Some(Field::Series),
        "tag" => Some(Field::Tag),
        _ => None,
    }
}
