//! Reading progress sync with KOReader's sync server protocol.
//!
//! The protocol and the conflict rules come from `plugins/kosync.koplugin`:
//! `api.json` and `KOSyncClient.lua` for requests, `main.lua` for the policy.
//! The crate makes no request unless the app calls a method on [`SyncClient`].
//!
//! - [`SyncClient`] registers, logs in, pushes, pulls and checks the server.
//! - [`Credentials`], [`Progress`] and [`Metadata`] are the values on the wire.
//! - [`decide`] and [`decide_manual`] pick what to do with a remote position.
//! - [`partial_md5`], [`filename_md5`] compute the document id.
//!
//! Build without the `tls` feature for plain http only. Then https addresses
//! are refused at [`SyncClient::new`].

mod client;
mod doc_id;
mod error;
mod model;
mod policy;

#[cfg(test)]
mod test_server;

pub use client::{SyncClient, Timeout, Timeouts, DEFAULT_SERVER};
pub use doc_id::{filename_md5, md5_hex, partial_md5, round_percent};
pub use error::{Error, InvalidField};
pub use model::{Credentials, Metadata, Progress};
pub use policy::{decide, decide_manual, Decision, LocalState, Policy, Strategy};
