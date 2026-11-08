//! Test-only, thread-local I/O failures. No production flags or commands.
use std::{cell::Cell, io};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    ShortWrite,
    DiskFull,
    Publish,
    Cleanup,
}
thread_local! {static ACTIVE:Cell<Option<Fault>>=const {Cell::new(None)};}
pub(crate) fn write(data: &[u8]) -> io::Result<&[u8]> {
    ACTIVE.with(|a| match a.get() {
        Some(Fault::DiskFull) => Err(io::Error::from_raw_os_error(28)),
        Some(Fault::ShortWrite) => Ok(&data[..data.len().min(3)]),
        _ => Ok(data),
    })
}
pub(crate) fn point(fault: Fault) -> io::Result<()> {
    ACTIVE.with(|a| {
        if a.get() == Some(fault) {
            Err(io::Error::other("injected package staging failure"))
        } else {
            Ok(())
        }
    })
}
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|a| a.set(None));
    }
}
#[test]
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn failed_package_writes_cleanup_and_never_publish_and_short_writes_roundtrip() {
    for fault in [
        Fault::DiskFull,
        Fault::Publish,
        Fault::Cleanup,
        Fault::ShortWrite,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (case, _) = kokin_store::create_case(
            directory.path().join("case.kokincase"),
            "package-fault",
            "synthetic package case credential",
        )
        .unwrap();
        let destination = directory.path().join("case.kokinpkg");
        ACTIVE.with(|a| a.set(Some(fault)));
        let reset = Reset;
        let result = crate::package(
            &case,
            &destination,
            "synthetic package credential",
            &crate::PackageOptions::default(),
        );
        drop(reset);
        if fault == Fault::ShortWrite {
            result.unwrap();
            assert_eq!(
                crate::verify(&destination).unwrap().verdict,
                crate::Verdict::Ok
            );
        } else {
            assert!(result.is_err());
            assert!(!destination.exists());
        }
        assert!(std::fs::read_dir(directory.path()).unwrap().all(|e| {
            let name = e.unwrap().file_name();
            name == "case.kokincase" || name == "case.kokinpkg"
        }));
    }
}
