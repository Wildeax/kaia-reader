//! Return codes, the thread-local last error, and the guard every export runs in.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};

use kaia_core::ValidationError;
use kaia_storage::StoreError;

pub const KAIA_OK: i32 = 0;
pub const KAIA_E_INVALID_ARG: i32 = -1;
pub const KAIA_E_BAD_HANDLE: i32 = -2;
pub const KAIA_E_NOT_FOUND: i32 = -3;
pub const KAIA_E_IO: i32 = -4;
pub const KAIA_E_DB: i32 = -5;
pub const KAIA_E_SCHEMA_TOO_NEW: i32 = -6;
pub const KAIA_E_VALIDATION: i32 = -7;
pub const KAIA_E_ASSET_REJECTED: i32 = -8;
pub const KAIA_E_PANIC: i32 = -99;

/// An error on its way to Lua: a return code and the message for `kaia_last_error`.
#[derive(Debug)]
pub struct Failure {
    pub code: i32,
    pub msg: String,
}

impl Failure {
    pub fn new(code: i32, msg: impl Into<String>) -> Self {
        Failure {
            code,
            msg: msg.into(),
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::new(KAIA_E_INVALID_ARG, msg)
    }

    pub fn bad_handle() -> Self {
        Self::new(KAIA_E_BAD_HANDLE, "unknown or closed store handle")
    }
}

impl From<StoreError> for Failure {
    fn from(err: StoreError) -> Self {
        Failure::new(err.code(), err.to_string())
    }
}

impl From<ValidationError> for Failure {
    fn from(err: ValidationError) -> Self {
        Failure::new(KAIA_E_VALIDATION, err.to_string())
    }
}

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Records the message of an error on the calling thread.
pub fn set_last_error(msg: &str) {
    let _ = LAST_ERROR.try_with(|slot| *slot.borrow_mut() = Some(msg.to_owned()));
}

/// The most recent error message on the calling thread.
pub fn last_error() -> Option<String> {
    LAST_ERROR.try_with(|slot| slot.borrow().clone()).ok().flatten()
}

/// Runs an export body. Errors become their return code. A panic becomes
/// `KAIA_E_PANIC`. Either way the message is recorded as the last error.
pub fn guard(body: impl FnOnce() -> Result<(), Failure>) -> i32 {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => KAIA_OK,
        Ok(Err(failure)) => {
            set_last_error(&failure.msg);
            failure.code
        }
        Err(payload) => {
            set_last_error(&format!("panic: {}", panic_text(payload.as_ref())));
            KAIA_E_PANIC
        }
    }
}

fn panic_text(payload: &(dyn Any + Send)) -> &str {
    if let Some(text) = payload.downcast_ref::<&str>() {
        text
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.as_str()
    } else {
        "unknown panic"
    }
}
