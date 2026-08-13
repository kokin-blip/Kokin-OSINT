//! Tauri command surface.
//!
//! This layer stays deliberately thin: it validates and marshals, and delegates
//! every decision to the `kokin-*` crates. Business logic here would be
//! untestable without a webview.
//!
//! That is why every `#[tauri::command]` below is one line over a method on
//! [`session::Session`]. `Session` knows nothing about Tauri, so the whole
//! lifecycle — create, open, recover, close, and every refusal — is exercised
//! by ordinary `cargo test` with no window on screen. What is not covered by
//! those tests is exactly the one line of marshalling, which is the amount of
//! untested code this arrangement is buying.
//!
//! # What crosses this boundary
//!
//! Nothing carrying key material, with one deliberate exception: the recovery
//! key, returned once by `create_case` and never again (see
//! [`session::NewCaseView`]). `kokin_store::OpenCase` holds the case master key
//! and does not implement `Serialize`; it must never gain it.

pub mod error;
pub mod session;

pub use error::{CommandError, ErrorCode};
pub use session::{CaseView, NewCaseView, Session, SessionView};

use std::path::PathBuf;

use tauri::State;

/// Reports whether the storage layer is genuinely linked against SQLCipher.
///
/// Surfaced in the UI's about panel so a mis-built binary that would silently
/// write unencrypted cases is visible to the user, not just to CI.
#[tauri::command]
fn storage_encryption_status() -> Result<String, CommandError> {
    // Queries the linked library rather than creating a throwaway case, so this
    // costs no key derivation and touches no filesystem.
    match kokin_store::cipher_backend() {
        Ok(Some(version)) => Ok(format!("sqlcipher {version}")),
        Ok(None) => Err(CommandError::new(
            ErrorCode::Storage,
            "plain sqlite — case files would NOT be encrypted",
        )),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
fn create_case(
    session: State<'_, Session>,
    path: PathBuf,
    case_id: String,
    passphrase: String,
) -> Result<NewCaseView, CommandError> {
    session.create(&path, &case_id, &passphrase)
}

#[tauri::command]
fn open_case(
    session: State<'_, Session>,
    path: PathBuf,
    passphrase: String,
) -> Result<CaseView, CommandError> {
    session.open(&path, &passphrase)
}

#[tauri::command]
fn open_case_with_recovery_key(
    session: State<'_, Session>,
    path: PathBuf,
    recovery_key: String,
) -> Result<CaseView, CommandError> {
    session.open_with_recovery_key(&path, &recovery_key)
}

#[tauri::command]
fn close_case(session: State<'_, Session>) -> Result<(), CommandError> {
    session.close()
}

#[tauri::command]
fn session_status(session: State<'_, Session>) -> Result<SessionView, CommandError> {
    session.view()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Failing to start the webview is unrecoverable and happens before any case
    // is open, so there is no case data to lose. This is the one deliberate
    // panic in the application; everything downstream returns errors instead.
    #[allow(clippy::expect_used)]
    tauri::Builder::default()
        .manage(Session::default())
        .invoke_handler(tauri::generate_handler![
            storage_encryption_status,
            create_case,
            open_case,
            open_case_with_recovery_key,
            close_case,
            session_status,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the Kokin-OSINT window");
}
