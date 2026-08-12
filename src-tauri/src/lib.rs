//! Tauri command surface.
//!
//! This layer stays deliberately thin: it validates and marshals, and delegates
//! every decision to the `kokin-*` crates. Business logic here would be
//! untestable without a webview.

/// Reports whether the storage layer is genuinely linked against SQLCipher.
///
/// Surfaced in the UI's about panel so a mis-built binary that would silently
/// write unencrypted cases is visible to the user, not just to CI.
#[tauri::command]
fn storage_encryption_status() -> Result<String, String> {
    let probe = std::env::temp_dir().join("kokin-encryption-probe.kokincase");
    let result = (|| -> Result<bool, kokin_store::StoreError> {
        let conn = kokin_store::open_encrypted(&probe, "probe")?;
        kokin_store::is_sqlcipher(&conn)
    })();
    let _ = std::fs::remove_file(&probe);

    match result {
        Ok(true) => Ok("sqlcipher".to_string()),
        Ok(false) => Err("plain sqlite — case files would NOT be encrypted".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Failing to start the webview is unrecoverable and happens before any case
    // is open, so there is no case data to lose. This is the one deliberate
    // panic in the application; everything downstream returns errors instead.
    #[allow(clippy::expect_used)]
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![storage_encryption_status])
        .run(tauri::generate_context!())
        .expect("failed to start the Kokin-OSINT window");
}
