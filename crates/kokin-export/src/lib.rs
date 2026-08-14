//! Case packaging, JCS manifests, integrity verification, import/restore.
//!
//! # What an export is
//!
//! ADR-0016 made a case a directory: a plaintext `header.json` describing how to
//! unwrap the case master key, a SQLCipher `case.db` keyed by the raw CMK, and a
//! `blobs/` directory of individually encrypted evidence. It also said the
//! working format is optimised for the application and the export format for
//! transport, and left the second one unwritten. This is it.
//!
//! The shape of the problem is set by one fact from ADR-0015: **the database is
//! keyed by the CMK itself, and only the header says how to reach the CMK.** So a
//! package can re-wrap the same CMK under a fresh passphrase and copy the
//! database and blobs across untouched — the same property that lets
//! `rotate_passphrase` change a passphrase without re-encrypting a case. Nothing
//! is ever decrypted to make a package, so no plaintext exists at any point, not
//! even briefly in a temporary file.
//!
//! The blob filenames survive too. `kokin_blob::storage_name` keys them with the
//! CMK precisely so a directory listing leaks no membership, and since the CMK
//! does not change, the names do not either.
//!
//! # The export passphrase is a new one, and the recovery wrap is dropped
//!
//! The package's `header.json` carries a fresh salt and the CMK wrapped under a
//! KEK derived from a passphrase given at export time (D-034). It does **not**
//! carry the source case's recovery wrap. Keeping it would mean every package
//! ever made is retroactively openable if that recovery key is ever exposed —
//! and the recovery key is the one piece of this system users are told to write
//! on paper.
//!
//! The honest consequence, in the other direction: a package has no recovery
//! path at all. Lose the export passphrase and that package is gone. That is
//! correct for a copy and would be indefensible for a case, which is why the two
//! headers differ here and nowhere else.
//!
//! It is also worth being plain about what re-wrapping does *not* buy. The CMK is
//! the same key, so anyone holding the source case's header and passphrase can
//! decrypt a package made from it. Key separation would mean re-encrypting every
//! blob under a new CMK; that is a different feature with a different cost, and
//! this is not it.

pub mod container;
pub mod manifest;

mod package;
mod verify;

pub use package::{import, package, ImportReport, PackageOptions, PackageReport};
pub use verify::{verify, EntryStatus, Verdict, VerifyReport};

#[doc = include_str!("../README.md")]
#[cfg(doc)]
pub struct ReadmeDoctests;

/// What can go wrong packaging, verifying or importing.
///
/// Note what is *not* here: a shredded blob, an excluded one and a hash mismatch
/// are all reported through [`VerifyReport`], not as errors. A package that is
/// missing evidence on purpose is a package working correctly, and a package that
/// has been modified is a finding to be presented rather than an exception to be
/// caught.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("store error: {0}")]
    Store(#[from] kokin_store::StoreError),

    #[error("blob error: {0}")]
    Blob(#[from] kokin_blob::BlobError),

    #[error("key error: {0}")]
    Keys(#[from] kokin_keys::KeyError),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("could not encode the manifest: {0}")]
    Encode(serde_json::Error),

    #[error("this file is not a Kokin case package")]
    NotAPackage,

    #[error("package format {found} is not readable by this build (it reads {supported})")]
    UnsupportedPackageVersion { found: u32, supported: u32 },

    #[error("the package is malformed: {0}")]
    Malformed(String),

    #[error("the package ends part-way through {name}")]
    Truncated { name: String },

    #[error("'{raw}' is not a name a case package may contain")]
    IllegalEntryName { raw: String },

    #[error("{name} changed size while being packaged: declared {declared}, wrote {written}")]
    SizeChangedDuringWrite {
        name: String,
        declared: u64,
        written: u64,
    },

    #[error("the destination {0} already exists")]
    DestinationExists(String),

    #[error("the package contains no manifest")]
    NoManifest,
}

pub type Result<T> = std::result::Result<T, ExportError>;
