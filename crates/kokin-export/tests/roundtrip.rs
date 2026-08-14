//! What a package carries, what it admits it does not, and what it refuses.
//!
//! These build a real case through the real pipeline — ingest, extract, then L2
//! rows grounded in the observations that produced them — because the claims
//! under test are about a case, and a hand-written `blob` row would not exercise
//! the keyed storage names, the audit chain, or the lineage the manifest is
//! supposed to preserve.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_export::manifest::OmissionReason;
use kokin_export::{import, package, verify, PackageOptions, Verdict};
use kokin_store::OpenCase;

static COUNTER: AtomicU32 = AtomicU32::new(0);

const CASE_PASSPHRASE: &str = "correct horse battery staple";
const EXPORT_PASSPHRASE: &str = "a different phrase entirely";
const PAGE: &[u8] =
    b"<html><title>Tungsten Holdings</title><body><p>Contact press@tungsten.example.</p></body></html>";
const SECOND: &[u8] = b"<html><title>Ridgeway Trust</title><body><p>Filed 2019.</p></body></html>";

struct Fixture {
    dir: PathBuf,
    case: PathBuf,
    open: OpenCase,
    /// Content hashes of the two documents, in ingest order.
    hashes: Vec<String>,
    entity: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("kokin-export-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let case = dir.join("case.kokincase");
    let created = kokin_store::create_case(&case, "case-export", CASE_PASSPHRASE).unwrap();
    drop(created);
    let mut open = kokin_store::open_case(&case, CASE_PASSPHRASE).unwrap();

    let blobs = kokin_blob::BlobStore::new(open.paths.blobs());
    let mut hashes = Vec::new();
    for (i, bytes) in [PAGE, SECOND].into_iter().enumerate() {
        let path = dir.join(format!("page{i}.html"));
        std::fs::write(&path, bytes).unwrap();
        let ingested = kokin_ingest::ingest_file(
            &mut open.conn,
            &blobs,
            &open.cmk,
            &path,
            &kokin_ingest::IngestContext {
                connector: "file_import",
                job_id: "test",
            },
        )
        .unwrap();
        kokin_extract::extract_artifact(
            &mut open.conn,
            &blobs,
            &open.cmk,
            &ingested.artifact_id,
            kokin_extract::Limits::default(),
        )
        .unwrap();
        hashes.push(ingested.content_hash);
    }

    let observation: String = open
        .conn
        .query_row(
            "SELECT id FROM observation ORDER BY rowid LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let entity = kokin_graph::create_entity(
        &mut open.conn,
        kokin_graph::NewEntity {
            type_key: "person",
            display_name: "A. Mercer",
            notes: "",
        },
        &[kokin_graph::Grounding {
            kind: kokin_graph::EvidenceKind::Observation,
            id: &observation,
            role: kokin_graph::EvidenceRole::Supports,
        }],
    )
    .unwrap();

    Fixture {
        dir,
        case,
        open,
        hashes,
        entity,
    }
}

fn shred(open: &OpenCase, content_hash: &str) {
    // What crypto-shredding is: the wrapped key goes, the row stays. The bytes on
    // disk are left exactly where they are and become permanently unreadable.
    open.conn
        .execute(
            "UPDATE blob SET wrapped_key = NULL, wrapped_nonce = NULL,
                             shredded_utc = '2026-08-14T00:00:00Z'
              WHERE content_hash = ?1",
            [content_hash],
        )
        .unwrap();
}

// ---------------------------------------------------------------------------
// The round trip
// ---------------------------------------------------------------------------

/// The whole point: a case survives the trip, and opens under a passphrase that
/// was never the case's.
#[test]
fn an_exported_case_opens_under_its_new_passphrase() {
    let f = fixture("roundtrip");
    let pkg = f.dir.join("out.kokinpkg");

    let report = package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();
    assert_eq!(report.blobs_written, 2);
    assert!(report.omissions.is_empty());

    let dest = f.dir.join("restored.kokincase");
    let imported = import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap();
    assert_eq!(imported.blobs_restored, 2);

    let restored = kokin_store::open_case(&dest, EXPORT_PASSPHRASE).unwrap();

    // The analytical layer arrived intact, not merely the bytes.
    let name: String = restored
        .conn
        .query_row(
            "SELECT display_name FROM entity WHERE id = ?1",
            [&f.entity],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "A. Mercer");

    let counts = |conn: &rusqlite::Connection| -> (i64, i64) {
        conn.query_row(
            "SELECT (SELECT COUNT(*) FROM observation),
                    (SELECT COUNT(*) FROM evidence_link)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    assert_eq!(counts(&restored.conn), counts(&f.open.conn));

    // The audit chain is deliberately *not* compared for equality. Opening the
    // restored case appends `case.opened`, exactly as opening any case does, so
    // an equal count would mean the copy had stopped recording. What must hold is
    // that the history arrived and was continued rather than restarted.
    let (before, after): (i64, i64) = (
        f.open
            .conn
            .query_row("SELECT COUNT(*) FROM audit_event", [], |r| r.get(0))
            .unwrap(),
        restored
            .conn
            .query_row("SELECT COUNT(*) FROM audit_event", [], |r| r.get(0))
            .unwrap(),
    );
    assert_eq!(
        after,
        before + 1,
        "the restored case did not record its open"
    );

    // And the evidence itself decrypts, which is the assertion the blob store's
    // keyed storage names would break first if the CMK had not travelled.
    let blobs = kokin_blob::BlobStore::new(restored.paths.blobs());
    let access = kokin_store::blob_access(&restored.conn, &f.hashes[0]).unwrap();
    let kokin_store::BlobAccess::Readable(blob_ref) = access else {
        panic!("the first document did not come back readable: {access:?}");
    };
    assert_eq!(blobs.get(&blob_ref, &restored.cmk).unwrap(), PAGE);
}

/// Work done since the last checkpoint is in the package.
///
/// This is the bug this crate shipped with for one afternoon, and it is worth a
/// test of its own because of how it presented: the case is in WAL mode, so
/// recent writes live in `case.db-wal` and not in `case.db`. Packaging copied
/// only `case.db`, and the result was a package that was internally consistent,
/// hashed correctly, verified `Ok`, imported cleanly, and was missing whatever
/// the analyst had done most recently — which is precisely the work someone is
/// most likely to be exporting.
///
/// Nothing about the package could reveal it. The check has to be that a row
/// written immediately before the export is on the other side.
#[test]
fn work_done_immediately_before_the_export_is_in_the_package() {
    let mut f = fixture("wal");

    let observation: String = f
        .open
        .conn
        .query_row(
            "SELECT id FROM observation ORDER BY rowid LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Written with no close and no checkpoint between this and the export, which
    // is what an analyst exporting the case they are working in looks like.
    let latest = kokin_graph::create_entity(
        &mut f.open.conn,
        kokin_graph::NewEntity {
            type_key: "person",
            display_name: "Written Last",
            notes: "",
        },
        &[kokin_graph::Grounding {
            kind: kokin_graph::EvidenceKind::Observation,
            id: &observation,
            role: kokin_graph::EvidenceRole::Supports,
        }],
    )
    .unwrap();

    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let dest = f.dir.join("restored.kokincase");
    import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap();
    let restored = kokin_store::open_case(&dest, EXPORT_PASSPHRASE).unwrap();

    let name: String = restored
        .conn
        .query_row(
            "SELECT display_name FROM entity WHERE id = ?1",
            [&latest],
            |r| r.get(0),
        )
        .expect("the last thing written before the export is not in the package");
    assert_eq!(name, "Written Last");
}

/// The audit chain survives as a chain, not as rows that happen to be present.
#[test]
fn the_audit_chain_still_verifies_after_the_trip() {
    let f = fixture("audit");
    let pkg = f.dir.join("out.kokinpkg");
    let report = package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let dest = f.dir.join("restored.kokincase");
    import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap();
    let restored = kokin_store::open_case(&dest, EXPORT_PASSPHRASE).unwrap();

    assert!(matches!(
        kokin_store::verify_chain(&restored.conn).unwrap(),
        kokin_store::ChainStatus::Intact { .. }
    ));

    // The manifest's head ties the package to the state of the case at the moment
    // it was made, so it must be *in* the restored chain — and not at the end of
    // it, because opening the restored case appended `case.opened` on top. A test
    // asserting it was the tip would have been asserting that the copy is inert.
    let packaged_head = verify(&pkg).unwrap().audit_head.expect("no head recorded");
    let head_bytes: Vec<u8> = (0..packaged_head.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&packaged_head[i..i + 2], 16).unwrap())
        .collect();

    let position: i64 = restored
        .conn
        .query_row(
            "SELECT COUNT(*) FROM audit_event WHERE id <= (
                 SELECT id FROM audit_event WHERE event_hash = ?1
             )",
            [&head_bytes],
            |r| r.get(0),
        )
        .unwrap();
    let total: i64 = restored
        .conn
        .query_row("SELECT COUNT(*) FROM audit_event", [], |r| r.get(0))
        .unwrap();
    assert!(
        position > 0,
        "the packaged head is not in the restored chain"
    );
    assert_eq!(
        position,
        total - 1,
        "the packaged head should be the event before the restored case's own open"
    );
    let _ = report;
}

/// A package holds no plaintext, and is not a database anybody can open.
#[test]
fn the_package_carries_no_plaintext() {
    let f = fixture("plaintext");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let bytes = std::fs::read(&pkg).unwrap();

    // Content from the documents, the entity name, and the passphrases. The
    // entity name is the interesting one: it lives in the database rather than a
    // blob, so it is the check that would catch a package built from an
    // unencrypted export of the case.
    for needle in [
        &b"Tungsten Holdings"[..],
        b"press@tungsten.example",
        b"Ridgeway Trust",
        b"A. Mercer",
        CASE_PASSPHRASE.as_bytes(),
        EXPORT_PASSPHRASE.as_bytes(),
    ] {
        assert!(
            !contains(&bytes, needle),
            "the package contains {:?} in plaintext",
            String::from_utf8_lossy(needle)
        );
    }

    // SQLite's magic header, which SQLCipher replaces with ciphertext. Its
    // presence anywhere would mean an unencrypted database went into the package.
    assert!(
        !contains(&bytes, b"SQLite format 3\0"),
        "the package contains an unencrypted SQLite database"
    );
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------------------
// Verdicts
// ---------------------------------------------------------------------------

#[test]
fn a_complete_package_verifies_ok_without_a_passphrase() {
    let f = fixture("ok");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    // No passphrase anywhere in this call. That is the requirement, not an
    // omission in the test.
    let report = verify(&pkg).unwrap();
    assert_eq!(report.verdict, Verdict::Ok);
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert!(report.omissions.is_empty());
    assert!(report.entries.iter().all(|e| e.matched));
    assert_eq!(report.case_id, "case-export");
}

/// A shredded blob is an omission, permanently — never `Ok`, and never `Failed`.
#[test]
fn a_package_holding_a_shredded_document_can_never_verify_ok() {
    let f = fixture("shredded");
    shred(&f.open, &f.hashes[1]);

    let pkg = f.dir.join("out.kokinpkg");
    let report = package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();
    assert_eq!(report.blobs_written, 1);

    let verified = verify(&pkg).unwrap();
    assert_eq!(verified.verdict, Verdict::Partial);
    assert!(
        verified.problems.is_empty(),
        "a shred was reported as damage: {:?}",
        verified.problems
    );
    assert!(verified.holds_shredded_evidence());
    assert_eq!(verified.omissions.len(), 1);
    assert_eq!(verified.omissions[0].content_hash, f.hashes[1]);
    assert_eq!(verified.omissions[0].reason, OmissionReason::Shredded);
}

/// Excluding evidence removes the bytes and keeps the record of the removal.
#[test]
fn an_excluded_document_leaves_its_row_and_its_absence_on_the_record() {
    let f = fixture("excluded");
    let pkg = f.dir.join("out.kokinpkg");

    let options = PackageOptions {
        exclude: BTreeSet::from([f.hashes[0].clone()]),
    };
    let report = package(&f.open, &pkg, EXPORT_PASSPHRASE, &options).unwrap();
    assert_eq!(report.blobs_written, 1);

    let verified = verify(&pkg).unwrap();
    assert_eq!(verified.verdict, Verdict::Partial);
    assert_eq!(verified.omissions[0].reason, OmissionReason::Excluded);
    assert!(!verified.holds_shredded_evidence());

    let dest = f.dir.join("restored.kokincase");
    import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap();
    let restored = kokin_store::open_case(&dest, EXPORT_PASSPHRASE).unwrap();

    // The row survives with its hash and its lineage: the recipient can see that
    // a document exists, what derived from it, and that they were not given it.
    let (rows, derivations): (i64, i64) = restored
        .conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM blob WHERE content_hash = ?1),
                    (SELECT COUNT(*) FROM derivation)",
            [&f.hashes[0]],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(rows, 1, "the excluded document's row did not travel");
    assert!(derivations > 0, "lineage was dropped along with the bytes");

    // And the bytes are genuinely gone.
    let blobs = kokin_blob::BlobStore::new(restored.paths.blobs());
    assert!(!blobs.contains(&f.hashes[0], &restored.cmk));
    assert!(blobs.contains(&f.hashes[1], &restored.cmk));
}

/// A blob the case lost is not reported as a blob somebody destroyed (A-032).
#[test]
fn a_document_the_case_lost_is_not_filed_as_a_deliberate_deletion() {
    let f = fixture("lost");

    // Delete the ciphertext and leave the row and its wrapped key intact — which
    // is exactly what an undetected storage failure looks like from the database.
    let storage_name = kokin_blob::storage_name(&f.hashes[0], &f.open.cmk);
    std::fs::remove_file(
        f.open
            .paths
            .blobs()
            .join(kokin_blob::relative_path(&storage_name)),
    )
    .unwrap();

    let pkg = f.dir.join("out.kokinpkg");
    let report = package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();
    assert_eq!(report.blobs_written, 1);

    let verified = verify(&pkg).unwrap();
    assert_eq!(verified.verdict, Verdict::Partial);
    assert_eq!(verified.omissions.len(), 1);
    assert_eq!(
        verified.omissions[0].reason,
        OmissionReason::Missing,
        "a document the case lost was reported as one somebody chose to destroy"
    );
    assert!(
        !verified.holds_shredded_evidence(),
        "a lost document was counted as shredded evidence"
    );
}

/// A modified package is an accusation, and names the file.
#[test]
fn a_modified_package_fails_and_says_which_entry() {
    let f = fixture("tampered");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    assert_eq!(verify(&pkg).unwrap().verdict, Verdict::Ok);

    // Flip one bit in the last quarter of the file, which is blob territory. Size
    // is unchanged, so only the hash can catch this.
    let mut bytes = std::fs::read(&pkg).unwrap();
    let at = bytes.len() - 8;
    bytes[at] ^= 0x01;
    std::fs::write(&pkg, &bytes).unwrap();

    let verified = verify(&pkg).unwrap();
    assert_eq!(verified.verdict, Verdict::Failed);
    assert!(
        verified.problems.iter().any(|p| p.contains("blobs/")),
        "the report did not name the entry that changed: {:?}",
        verified.problems
    );
    assert!(verified.entries.iter().any(|e| !e.matched));
}

/// A modified package does not become a case.
#[test]
fn importing_a_modified_package_leaves_nothing_behind() {
    let f = fixture("tampered-import");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let mut bytes = std::fs::read(&pkg).unwrap();
    let at = bytes.len() - 8;
    bytes[at] ^= 0x01;
    std::fs::write(&pkg, &bytes).unwrap();

    let dest = f.dir.join("restored.kokincase");
    let err = import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap_err();
    assert!(
        err.to_string().contains("modified") || err.to_string().contains("damaged"),
        "{err}"
    );
    assert!(
        !dest.exists(),
        "a half-written case directory was left on disk"
    );
}

/// Truncation is reported as truncation rather than as a hash mismatch.
#[test]
fn a_truncated_package_is_reported_as_truncated() {
    let f = fixture("truncated");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let bytes = std::fs::read(&pkg).unwrap();
    std::fs::write(&pkg, &bytes[..bytes.len() - 32]).unwrap();

    let err = verify(&pkg).unwrap_err();
    assert!(
        matches!(err, kokin_export::ExportError::Truncated { .. }),
        "{err}"
    );
}

/// Not every file is a package, and one from a later build says so by name.
#[test]
fn a_file_that_is_not_a_package_is_refused_before_anything_is_read() {
    let f = fixture("notapackage");
    let path = f.dir.join("random.bin");
    std::fs::write(&path, b"this is not a package, it is 44 bytes of text").unwrap();

    assert!(matches!(
        verify(&path).unwrap_err(),
        kokin_export::ExportError::NotAPackage
    ));
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// The export passphrase is the only one that opens a package.
#[test]
fn the_case_passphrase_does_not_open_a_package_made_from_it() {
    let f = fixture("passphrase");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let dest = f.dir.join("restored.kokincase");
    let err = import(&pkg, &dest, CASE_PASSPHRASE).unwrap_err();
    assert!(
        matches!(err, kokin_export::ExportError::Store(_)),
        "the wrong passphrase did not fail closed on the AEAD tag: {err}"
    );
}

/// A package carries no recovery path, deliberately (D-034).
#[test]
fn a_package_has_no_recovery_wrap() {
    let f = fixture("norecovery");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let dest = f.dir.join("restored.kokincase");
    import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap();

    let header = kokin_store::header::read_header(&kokin_store::CasePaths::new(&dest)).unwrap();
    assert!(
        header.cmk_wrapped_by_recovery.is_none(),
        "the source case's recovery key would open every package ever made from it"
    );

    // The source case still has its own, which is the half that must not change.
    let source = kokin_store::header::read_header(&kokin_store::CasePaths::new(&f.case)).unwrap();
    assert!(source.cmk_wrapped_by_recovery.is_some());

    // A fresh salt, so the two headers are not derived from related material.
    assert_ne!(header.salt_hex, source.salt_hex);
}

/// Neither call overwrites something that is already there.
#[test]
fn an_existing_destination_is_never_overwritten() {
    let f = fixture("exists");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    assert!(matches!(
        package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap_err(),
        kokin_export::ExportError::DestinationExists(_)
    ));

    let dest = f.dir.join("restored.kokincase");
    import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap();
    assert!(matches!(
        import(&pkg, &dest, EXPORT_PASSPHRASE).unwrap_err(),
        kokin_export::ExportError::DestinationExists(_)
    ));
}

/// Packaging leaves no working files beside the package.
#[test]
fn packaging_cleans_up_after_itself() {
    let f = fixture("staging");
    let pkg = f.dir.join("out.kokinpkg");
    package(&f.open, &pkg, EXPORT_PASSPHRASE, &PackageOptions::default()).unwrap();

    let strays: Vec<PathBuf> = std::fs::read_dir(&f.dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.to_string_lossy().contains("staging"))
        })
        .collect();
    assert!(strays.is_empty(), "left behind: {strays:?}");
    assert!(is_file(&pkg));
}

fn is_file(path: &Path) -> bool {
    path.metadata().map(|m| m.is_file()).unwrap_or(false)
}
