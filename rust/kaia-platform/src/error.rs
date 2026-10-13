//! The error type for device services.

use std::fmt;
use std::io;

/// Errors from device services.
#[derive(Debug)]
pub enum PlatformError {
    /// A device file could not be read or written.
    Io {
        /// The device path or command involved.
        context: String,
        /// The underlying error.
        source: io::Error,
    },
    /// A Kobo whose codename has no profile in this crate.
    UnsupportedModel(String),
    /// The Kobo codename could not be determined.
    UnknownModel,
    /// The operation does not exist on this platform (for example, Wi-Fi scan on desktop).
    NotSupported(&'static str),
    /// A command from a plan failed.
    Command(String),
    /// A value was rejected before it was sent to the device.
    InvalidInput(String),
    /// wpa_supplicant control socket failure or unexpected reply.
    Wpa(String),
    /// Wi-Fi did not associate in time.
    ConnectTimeout,
    /// wpa_supplicant reported an authentication failure.
    AuthFailed,
    /// No home directory is set, so desktop paths cannot be built.
    NoHome,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlatformError::Io { context, source } => write!(f, "{context}: {source}"),
            PlatformError::UnsupportedModel(codename) => {
                write!(f, "unsupported Kobo model codename `{codename}`")
            }
            PlatformError::UnknownModel => write!(f, "cannot determine the Kobo model codename"),
            PlatformError::NotSupported(what) => write!(f, "{what} is not supported on this platform"),
            PlatformError::Command(text) => write!(f, "command failed: {text}"),
            PlatformError::InvalidInput(text) => write!(f, "invalid input: {text}"),
            PlatformError::Wpa(text) => write!(f, "wpa_supplicant: {text}"),
            PlatformError::ConnectTimeout => write!(f, "Wi-Fi did not connect in time"),
            PlatformError::AuthFailed => write!(f, "Wi-Fi authentication failed"),
            PlatformError::NoHome => write!(f, "HOME is not set"),
        }
    }
}

impl std::error::Error for PlatformError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PlatformError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl PlatformError {
    /// Wraps an I/O error with the device path or command it came from.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        PlatformError::Io {
            context: context.into(),
            source,
        }
    }
}
