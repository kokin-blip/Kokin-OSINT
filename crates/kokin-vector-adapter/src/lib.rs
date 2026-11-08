//! The sole audited SQLite extension initialization boundary (ADR-0018).
use libsqlite3_sys::{sqlite3, sqlite3_api_routines};
use std::ffi::{c_char, c_int};
use std::sync::OnceLock;
// Force the statically linked upstream implementation into this adapter.
use sqlite_vec as _;
type Initializer =
    unsafe extern "C" fn(*mut sqlite3, *mut *mut c_char, *const sqlite3_api_routines) -> c_int;
#[allow(unsafe_code)]
mod boundary {
    use super::*;
    extern "C" {
        // Typed ABI declarations avoid the upstream example's transmute.
        fn sqlite3_vec_init(
            db: *mut sqlite3,
            error: *mut *mut c_char,
            api: *const sqlite3_api_routines,
        ) -> c_int;
        fn sqlite3_auto_extension(init: Option<Initializer>) -> c_int;
    }
    pub(super) fn initialize() -> Result<(), i32> {
        // SAFETY: both declarations match SQLite's extension entrypoint ABI. The
        // statically linked function lives for the process lifetime; SQLite owns all
        // callback arguments. Only this fixed callback is registered, once, before
        // opening a case. No user library or pointer can enter this function.
        let code = unsafe { sqlite3_auto_extension(Some(sqlite3_vec_init)) };
        if code == 0 {
            Ok(())
        } else {
            Err(code)
        }
    }
}
pub fn initialize() -> Result<(), i32> {
    static RESULT: OnceLock<Result<(), i32>> = OnceLock::new();
    *RESULT.get_or_init(boundary::initialize)
}
