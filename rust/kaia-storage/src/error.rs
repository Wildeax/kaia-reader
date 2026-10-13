use kaia_core::ValidationError;

/// Errors from the store. Each variant maps to one ABI error code.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A caller passed a malformed argument, such as a bad id or hash.
    #[error("invalid argument: {0}")]
    InvalidArg(String),
    /// The sticker, document, asset or asset file does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// A file operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// A SQLite operation failed.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    /// The database was written by a newer build. It was not modified.
    #[error("database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew { found: u32, supported: u32 },
    /// A record breaks a rule in `STORAGE.md`.
    #[error(transparent)]
    Validation(#[from] ValidationError),
    /// An image file was refused: wrong type, too large, or unreadable header.
    #[error("asset rejected: {0}")]
    AssetRejected(String),
}

impl StoreError {
    /// The ABI error code for this error (`KAIA_E_*` in `docs/kaia/specs/ABI.md`).
    pub fn code(&self) -> i32 {
        match self {
            StoreError::InvalidArg(_) => -1,
            StoreError::NotFound(_) => -3,
            StoreError::Io(_) => -4,
            StoreError::Db(_) => -5,
            StoreError::SchemaTooNew { .. } => -6,
            StoreError::Validation(_) => -7,
            StoreError::AssetRejected(_) => -8,
        }
    }
}
