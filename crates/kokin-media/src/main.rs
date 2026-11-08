//! Exactly one framed request per sandboxed process. Diagnostics never include source bytes.
use std::io::{Read, Write};
fn work() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = std::io::stdin().lock();
    let mut header = [0u8; 4];
    input.read_exact(&mut header)?;
    let length = u32::from_le_bytes(header) as usize;
    if length > 4096 {
        return Err("invalid media request length".into());
    }
    let mut json = vec![0; length];
    input.read_exact(&mut json)?;
    let request: kokin_media::Request = serde_json::from_slice(&json)?;
    let mut bytes = Vec::new();
    input
        .take(kokin_media::MAX_INPUT as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > kokin_media::MAX_INPUT {
        return Err("source exceeds media limit".into());
    }
    let native = std::env::var_os("KOKIN_MEDIA_NATIVE").ok_or("native engine path is absent")?;
    let ocr = std::env::var_os("KOKIN_OCR_DATA");
    let result = kokin_media::engine::process(
        &bytes,
        &request,
        &std::path::PathBuf::from(native).join("pdfium/libpdfium.dylib"),
        ocr.as_deref().map(std::path::Path::new),
    )?;
    let json = serde_json::to_vec(&result)?;
    std::io::stdout().lock().write_all(&json)?;
    Ok(())
}
fn main() {
    if let Err(error) = work() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
