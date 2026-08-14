//! Checking a package without being able to open it.
//!
//! Verification takes no passphrase, and that is a requirement rather than a
//! convenience. Integrity and confidentiality are different questions: a courier,
//! an archivist or a recipient who has not yet been given the passphrase should
//! still be able to establish that a file arrived intact. Everything needed for
//! that is plaintext by design — the manifest, the entry sizes, and BLAKE3 over
//! ciphertext that stays ciphertext throughout.

use std::collections::BTreeMap;
use std::path::Path;

use crate::container::{EntryName, Reader};
use crate::manifest::{Manifest, Omission, OmissionReason};
use crate::{ExportError, Result};

/// What a package is worth.
///
/// Three, not two. The distinction between [`Verdict::Partial`] and
/// [`Verdict::Failed`] is the same one A-032 forced on `artifact_document`: a
/// document that is absent because somebody decided so, and a document that is
/// absent because something went wrong, are different facts about a case, and
/// collapsing them reports a deliberate act where there was an undetected
/// failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Every entry the manifest lists is present with the hash it claims, and the
    /// package withholds nothing.
    Ok,

    /// Structurally sound and hash-clean, and evidence is missing by record: a
    /// crypto-shredded blob, an analyst's exclusion, or a blob the source case
    /// had already lost.
    ///
    /// **A package containing a shredded blob can never verify `Ok` again.** The
    /// key is destroyed, so those bytes can never be checked against their
    /// content hash by anyone, ever. That is permanent and is the honest price of
    /// crypto-shredding rather than a defect in this check.
    Partial,

    /// An entry's hash does not match, an entry is absent that the manifest lists,
    /// or the file holds something the manifest never mentioned.
    ///
    /// This is an accusation and reads as one. Nothing in the package's own
    /// bookkeeping explains the discrepancy, which leaves modification,
    /// truncation, or storage that is failing.
    Failed,
}

#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub verdict: Verdict,
    pub case_id: String,
    pub exported_utc: String,
    pub audit_head: Option<String>,
    pub entries: Vec<EntryStatus>,
    /// What the package says it is not carrying, and why.
    pub omissions: Vec<Omission>,
    /// Human-readable findings behind a `Failed`. Empty for `Ok` and `Partial`.
    pub problems: Vec<String>,
}

impl VerifyReport {
    /// Whether any omission is a shred, which is the one that can never be undone.
    pub fn holds_shredded_evidence(&self) -> bool {
        self.omissions
            .iter()
            .any(|o| o.reason == OmissionReason::Shredded)
    }
}

#[derive(Debug, Clone)]
pub struct EntryStatus {
    pub name: String,
    pub size: u64,
    pub matched: bool,
}

/// Check a package against its own manifest. No passphrase, nothing decrypted.
pub fn verify(path: &Path) -> Result<VerifyReport> {
    let mut reader = Reader::open(path)?;

    let Some(first) = reader.next_entry()? else {
        return Err(ExportError::NoManifest);
    };
    if first.name != EntryName::Manifest {
        return Err(ExportError::NoManifest);
    }
    let mut manifest_bytes = Vec::new();
    reader.read_entry(&first, &mut manifest_bytes)?;
    let manifest = Manifest::from_slice(&manifest_bytes)?;

    // Claimed entries, by name. A duplicate name in the manifest is itself a
    // finding: it means one of the two is describing bytes nobody will check.
    let mut claimed: BTreeMap<&str, &crate::manifest::Entry> = BTreeMap::new();
    let mut problems = Vec::new();
    for entry in &manifest.entries {
        if claimed.insert(entry.name.as_str(), entry).is_some() {
            problems.push(format!("the manifest lists {} twice", entry.name));
        }
    }

    let mut entries = Vec::new();
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();

    while let Some(entry) = reader.next_entry()? {
        let name = entry.name.as_str();
        let actual = reader.read_entry(&entry, std::io::sink())?;

        if seen.insert(name.clone(), ()).is_some() {
            problems.push(format!("{name} appears in the package twice"));
        }

        let matched = match claimed.get(name.as_str()) {
            Some(expected) => {
                let ok = expected.blake3 == actual && expected.size == entry.size;
                if !ok {
                    problems.push(format!(
                        "{name} does not match the manifest \
                         (expected {} bytes / {}, found {} bytes / {})",
                        expected.size,
                        short(&expected.blake3),
                        entry.size,
                        short(&actual)
                    ));
                }
                ok
            }
            None => {
                // An entry the manifest never mentioned. Structurally legal, and
                // exactly what an addition looks like.
                problems.push(format!("{name} is in the package and not in the manifest"));
                false
            }
        };

        entries.push(EntryStatus {
            name,
            size: entry.size,
            matched,
        });
    }

    for name in claimed.keys() {
        if !seen.contains_key(*name) {
            problems.push(format!("{name} is in the manifest and not in the package"));
        }
    }

    // A case without its database or header is not a case, however well its
    // remaining entries hash.
    for required in [EntryName::Header.as_str(), EntryName::Database.as_str()] {
        if !seen.contains_key(&required) {
            problems.push(format!("the package has no {required}"));
        }
    }

    let verdict = if !problems.is_empty() {
        Verdict::Failed
    } else if manifest.omissions.is_empty() {
        Verdict::Ok
    } else {
        Verdict::Partial
    };

    Ok(VerifyReport {
        verdict,
        case_id: manifest.case_id,
        exported_utc: manifest.exported_utc,
        audit_head: manifest.audit_head,
        entries,
        omissions: manifest.omissions,
        problems,
    })
}

fn short(hash: &str) -> String {
    hash.chars().take(12).collect()
}
