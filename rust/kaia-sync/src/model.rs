//! Values that cross the wire: credentials, reading positions and metadata.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::doc_id::md5_hex;
use crate::error::{Error, InvalidField};

/// A username and the MD5 hex digest of the password.
///
/// The plugin never sends the password. It hashes it once at login, stores
/// the digest as `userkey`, and sends the digest in `x-auth-key` and as the
/// password field at registration. Kaia keeps the same digest, so a stored
/// login works without the password.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    username: String,
    userkey: String,
}

impl Credentials {
    /// Checks and hashes a password typed by the user. The username is
    /// trimmed. Both fields need at least one non-blank character, as in
    /// `validateUser` in `main.lua`.
    pub fn new(username: &str, password: &str) -> Result<Self, Error> {
        let username = username.trim();
        let user_ok = !username.is_empty();
        match (user_ok, userkey_for(password)) {
            (true, Some(userkey)) => Ok(Self {
                username: username.to_owned(),
                userkey,
            }),
            (false, None) => Err(Error::InvalidCredentials(InvalidField::Both)),
            (false, Some(_)) => Err(Error::InvalidCredentials(InvalidField::Username)),
            (true, None) => Err(Error::InvalidCredentials(InvalidField::Password)),
        }
    }

    /// Restores a login from a stored username and digest, without the password.
    pub fn from_stored(username: &str, userkey: &str) -> Result<Self, Error> {
        let username = username.trim();
        let user_ok = !username.is_empty();
        let key_ok = !userkey.trim().is_empty();
        match (user_ok, key_ok) {
            (true, true) => Ok(Self {
                username: username.to_owned(),
                userkey: userkey.trim().to_owned(),
            }),
            (false, false) => Err(Error::InvalidCredentials(InvalidField::Both)),
            (false, true) => Err(Error::InvalidCredentials(InvalidField::Username)),
            (true, false) => Err(Error::InvalidCredentials(InvalidField::Password)),
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    /// The MD5 hex digest of the password. Store this, not the password.
    pub fn userkey(&self) -> &str {
        &self.userkey
    }
}

/// The user key for a password as typed: its MD5 hex digest. KOReader stores this
/// value as `userkey` (`doLogin` in `main.lua`). None for a blank password, which
/// `validateUser` rejects. Store the key, never the password.
pub fn userkey_for(password: &str) -> Option<String> {
    let filled = password.chars().any(|c| !c.is_whitespace());
    filled.then(|| md5_hex(password.as_bytes()))
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("userkey", &"<redacted>")
            .finish()
    }
}

/// Document details the plugin sends when "Send document metadata" is on.
/// The official server ignores them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<String>,
}

/// A reading position for one document, as the server stores it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    /// The document id: [`partial_md5`](crate::partial_md5) or
    /// [`filename_md5`](crate::filename_md5), depending on the checksum setting.
    pub document: String,
    /// The position as text. A page number for fixed layout, an xpointer for
    /// reflowable books.
    pub progress: String,
    /// Fraction read, from 0.0 to 1.0. Rounded down to four decimals on the wire.
    pub percentage: f64,
    /// Device name: the hostname if the user set one, otherwise the model.
    pub device: String,
    /// Stable id of this installation.
    pub device_id: String,
    /// Server time in seconds. `None` from servers that do not send one.
    pub timestamp: Option<i64>,
    /// Only sent by [`SyncClient::push`](crate::SyncClient::push), and only
    /// when set. Pulled progress has `None`.
    pub metadata: Option<Metadata>,
}
