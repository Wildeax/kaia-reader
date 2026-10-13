//! Errors from the Lua host.

use std::fmt;

/// An error from the Lua host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The host could not start: a path is missing, or the startup script failed.
    Init(String),
    /// Lua raised an error, or a chunk did not compile. The text includes the Lua
    /// traceback, or the message of the Rust callback that failed.
    Lua(String),
    /// A value cannot be converted between Lua and Kaia values. See `convert`.
    Convert(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Init(msg) => write!(f, "lua host init: {msg}"),
            Error::Lua(msg) => write!(f, "lua error: {msg}"),
            Error::Convert(msg) => write!(f, "value conversion: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn lua_error(err: mlua::Error) -> Error {
    Error::Lua(err.to_string())
}
