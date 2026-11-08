#![allow(clippy::unwrap_used)]
use kokin_osint_lib::resources::verify;
use std::sync::atomic::AtomicBool;
#[test]
fn local_resource_identity_size_and_cancellation_are_mandatory() {
    use sha2::{Digest, Sha256};
    let path = std::env::temp_dir().join(format!("kokin-resource-check-{}", std::process::id()));
    std::fs::write(&path, b"synthetic pinned resource").unwrap();
    let hash = format!("{:x}", Sha256::digest(b"synthetic pinned resource"));
    let bytes = std::fs::metadata(&path).unwrap().len();
    assert!(verify(&path, bytes, &hash, &AtomicBool::new(false)).is_ok());
    assert!(verify(&path, bytes + 1, &hash, &AtomicBool::new(false)).is_err());
    assert!(verify(&path, bytes, &"f".repeat(64), &AtomicBool::new(false)).is_err());
    assert!(verify(&path, bytes, &hash, &AtomicBool::new(true)).is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(verify(&path, bytes, &hash, &AtomicBool::new(false)).is_err());
}
