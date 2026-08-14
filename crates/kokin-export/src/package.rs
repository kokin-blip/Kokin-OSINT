//! Making a package, and turning one back into a case.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use kokin_keys::{derive_kek, generate_salt, wrap_cmk, KdfProfile};
use kokin_store::header::{read_header, CaseHeader, WrappedKeyRecord, HEADER_FORMAT_VERSION};
use kokin_store::{CasePaths, OpenCase};
use rusqlite::Connection;

use crate::container::{EntryName, Reader, Writer};
use crate::manifest::{Entry, Manifest, Omission, OmissionReason, MANIFEST_FORMAT_VERSION};
use crate::{ExportError, Result};

/// What to leave out, and under whose authority.
#[derive(Debug, Clone, Default)]
pub struct PackageOptions {
    /// Content hashes the analyst has chosen to withhold from this package.
    ///
    /// The rows that reference them travel anyway. See the note on
    /// [`OmissionReason::Excluded`]: a package that silently dropped both would
    /// hand over a case whose lineage points at documents that were never
    /// mentioned, and the recipient would have no way to tell that from a bug.
    pub exclude: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct PackageReport {
    pub case_id: String,
    pub blobs_written: usize,
    pub omissions: Vec<Omission>,
    pub bytes: u64,
}

/// Write a case to a single transport file.
///
/// `passphrase` protects the package and is unrelated to the one protecting the
/// case. Nothing is decrypted: the database and every blob are copied as the
/// ciphertext they already are.
pub fn package(
    open: &OpenCase,
    dest: &Path,
    passphrase: &str,
    options: &PackageOptions,
) -> Result<PackageReport> {
    if dest.exists() {
        return Err(ExportError::DestinationExists(dest.display().to_string()));
    }

    // A-036. Everything written since the last checkpoint lives in `case.db-wal`, not in
    // `case.db`. Copying the database file without this produces a package that
    // is internally consistent, hashes correctly, verifies `Ok`, and is missing
    // whatever the analyst did most recently - which is the work they are most
    // likely to be exporting. The package deliberately carries no `-wal`
    // sidecar; a checkpoint folds it in, so the package stays one file that
    // needs no recovery step to be complete.
    checkpoint(&open.conn)?;

    let source_header = read_header(&open.paths)?;
    let export_header = rewrap_for_export(&source_header, open, passphrase)?;
    let header_json = serde_json::to_vec_pretty(&export_header).map_err(ExportError::Encode)?;

    let (blobs, omissions) = survey_blobs(&open.conn, open, options)?;

    // Two passes over the entries: the manifest has to describe hashes that only
    // exist once the bytes have been written, and it is the first entry so that
    // verification reads the claim before the evidence. So the package is built
    // to a temporary file, then rewritten with the manifest in front.
    //
    // The alternative - hashing every blob first, then writing - reads each blob
    // twice from disk, and this way reads it once and writes it twice. For a case
    // whose blobs are the bulk of it, the second is the cheaper trade and neither
    // holds a case in memory.
    let staging = dest.with_extension("kokinpkg-staging");
    let _ = std::fs::remove_file(&staging);
    let mut entries = Vec::new();

    let mut writer = Writer::create(&staging)?;
    entries.push(write_bytes(&mut writer, EntryName::Header, &header_json)?);
    entries.push(write_file(
        &mut writer,
        EntryName::Database,
        &open.paths.database(),
    )?);
    for storage_name in &blobs {
        let path = open
            .paths
            .blobs()
            .join(kokin_blob::relative_path(storage_name));
        entries.push(write_file(
            &mut writer,
            EntryName::Blob(storage_name.clone()),
            &path,
        )?);
    }
    writer.finish()?;

    let manifest = Manifest {
        format_version: MANIFEST_FORMAT_VERSION,
        case_id: source_header.case_id.clone(),
        exported_utc: kokin_store::now_utc_rfc3339(),
        audit_head: audit_head(&open.conn)?,
        entries,
        omissions: omissions.clone(),
    };

    let bytes = assemble(&staging, dest, &manifest)?;
    let _ = std::fs::remove_file(&staging);

    Ok(PackageReport {
        case_id: source_header.case_id,
        blobs_written: blobs.len(),
        omissions,
        bytes,
    })
}

/// The export header: same case, same CMK, new passphrase, no recovery wrap.
///
/// The aad is unchanged because it binds the case id and the KDF profile id, and
/// neither moves — a package is the same case, and a package that claimed a
/// different case id would be a different case with the same contents, which is
/// not a thing this product should be able to produce by accident.
fn rewrap_for_export(source: &CaseHeader, open: &OpenCase, passphrase: &str) -> Result<CaseHeader> {
    let profile = KdfProfile::from_id(source.kdf_profile_id).map_err(ExportError::Keys)?;
    let salt = generate_salt().map_err(ExportError::Keys)?;

    let mut header = CaseHeader {
        format_version: HEADER_FORMAT_VERSION,
        case_id: source.case_id.clone(),
        created_utc: source.created_utc.clone(),
        kdf_profile_id: source.kdf_profile_id,
        salt_hex: hex(&salt),
        cmk_wrapped_by_passphrase: WrappedKeyRecord {
            nonce_hex: String::new(),
            ciphertext_hex: String::new(),
        },
        cmk_wrapped_by_recovery: None,
    };

    let aad = aad_for(&header);
    let kek = derive_kek(passphrase, &salt, profile).map_err(ExportError::Keys)?;
    let wrapped = wrap_cmk(&open.cmk, &kek, &aad).map_err(ExportError::Keys)?;
    header.cmk_wrapped_by_passphrase = WrappedKeyRecord {
        nonce_hex: hex(&wrapped.nonce),
        ciphertext_hex: hex(&wrapped.ciphertext),
    };
    Ok(header)
}

/// The same bytes `CaseHeader::aad` builds.
///
/// Duplicated rather than exposed, because making it public would invite callers
/// outside `kokin-store` to construct wraps, and this is the only one that needs
/// to. `an_exported_case_opens_under_its_new_passphrase` fails immediately if
/// these two ever disagree.
fn aad_for(header: &CaseHeader) -> Vec<u8> {
    format!(
        "kokin:case:{}|kdf:{}",
        header.case_id, header.kdf_profile_id
    )
    .into_bytes()
}

/// Which blobs travel, and what to say about the ones that do not.
///
/// Walks the `blob` table rather than the blobs directory: the database is the
/// authority on what this case believes it holds, and a file in `blobs/` with no
/// row is not part of the case. That choice is what makes [`OmissionReason::Missing`]
/// detectable at all — the opposite walk cannot notice something that is absent.
fn survey_blobs(
    conn: &Connection,
    open: &OpenCase,
    options: &PackageOptions,
) -> Result<(Vec<String>, Vec<Omission>)> {
    let mut statement =
        conn.prepare("SELECT content_hash, shredded_utc FROM blob ORDER BY content_hash")?;
    let rows = statement.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;

    let mut travelling = Vec::new();
    let mut omissions = Vec::new();
    for row in rows {
        let (content_hash, shredded_utc) = row?;

        if shredded_utc.is_some() {
            omissions.push(Omission {
                content_hash,
                reason: OmissionReason::Shredded,
            });
            continue;
        }
        if options.exclude.contains(&content_hash) {
            omissions.push(Omission {
                content_hash,
                reason: OmissionReason::Excluded,
            });
            continue;
        }

        let storage_name = kokin_blob::storage_name(&content_hash, &open.cmk);
        if !open
            .paths
            .blobs()
            .join(kokin_blob::relative_path(&storage_name))
            .exists()
        {
            // A-032: the row and its wrapped key are intact and the ciphertext is
            // gone. Nobody decided this, and calling it shredded would say
            // somebody did.
            omissions.push(Omission {
                content_hash,
                reason: OmissionReason::Missing,
            });
            continue;
        }
        travelling.push(storage_name);
    }

    Ok((travelling, omissions))
}

/// Fold the write-ahead log into the database file.
///
/// TRUNCATE rather than PASSIVE: PASSIVE checkpoints what it can and reports
/// success regardless of how much it left behind, which is the same silent
/// partial copy with an extra step. TRUNCATE also resets the log to zero length,
/// so a package built immediately afterwards cannot pick up a stale tail.
fn checkpoint(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
    Ok(())
}

fn audit_head(conn: &Connection) -> Result<Option<String>> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM audit_event", [], |r| r.get(0))?;
    if count == 0 {
        return Ok(None);
    }
    let head = kokin_store::audit::tip_hash(conn)?;
    Ok(Some(
        head.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    ))
}

fn write_bytes(writer: &mut Writer, name: EntryName, bytes: &[u8]) -> Result<Entry> {
    let hash = writer.write_entry(&name, bytes.len() as u64, bytes)?;
    Ok(Entry {
        name: name.as_str(),
        size: bytes.len() as u64,
        blake3: hash,
    })
}

fn write_file(writer: &mut Writer, name: EntryName, path: &Path) -> Result<Entry> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let hash = writer.write_entry(&name, size, BufReader::new(file))?;
    Ok(Entry {
        name: name.as_str(),
        size,
        blake3: hash,
    })
}

/// Rewrite the staged package with the manifest as its first entry.
fn assemble(staging: &Path, dest: &Path, manifest: &Manifest) -> Result<u64> {
    let jcs = manifest.to_jcs()?;

    let mut out = Writer::create(dest)?;
    out.write_entry(&EntryName::Manifest, jcs.len() as u64, jcs.as_bytes())?;

    let mut staged = Reader::open(staging)?;
    while let Some(entry) = staged.next_entry()? {
        // Streamed through, not buffered: the database entry is the whole case.
        let mut sink = EntrySink {
            writer: &mut out,
            name: entry.name.clone(),
            size: entry.size,
            buffer: Vec::new(),
        };
        staged.read_entry(&entry, &mut sink)?;
        sink.flush_entry()?;
    }
    out.finish()?;

    Ok(std::fs::metadata(dest)?.len())
}

/// Bridges the reader's `Write` sink to the writer's `Read` source.
///
/// The container writes an entry from a reader and reads one into a writer, and
/// this is the only place both are needed at once. It buffers, which is
/// acceptable only because `assemble` is a copy of already-written entries and
/// could be replaced by an in-place seek if a case ever outgrows it — recorded as
/// a limitation rather than pretended away.
struct EntrySink<'a> {
    writer: &'a mut Writer,
    name: EntryName,
    size: u64,
    buffer: Vec<u8>,
}

impl EntrySink<'_> {
    fn flush_entry(&mut self) -> Result<()> {
        self.writer
            .write_entry(&self.name, self.size, self.buffer.as_slice())?;
        Ok(())
    }
}

impl std::io::Write for EntrySink<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ImportReport {
    pub case_id: String,
    pub root: std::path::PathBuf,
    pub blobs_restored: usize,
    pub omissions: Vec<Omission>,
}

/// Turn a package back into a case directory.
///
/// Deliberately does **not** verify first. [`crate::verify`] is a separate call
/// that needs no passphrase, and folding it in would mean either verifying twice
/// or offering an import that quietly skipped it. What import does enforce is
/// per-entry: every entry's hash is checked against the manifest as it is
/// written, so a modified package cannot land on disk as a case.
pub fn import(source: &Path, dest: &Path, passphrase: &str) -> Result<ImportReport> {
    if dest.exists() {
        return Err(ExportError::DestinationExists(dest.display().to_string()));
    }

    let mut reader = Reader::open(source)?;
    let Some(first) = reader.next_entry()? else {
        return Err(ExportError::NoManifest);
    };
    if first.name != EntryName::Manifest {
        return Err(ExportError::NoManifest);
    }
    let mut manifest_bytes = Vec::new();
    reader.read_entry(&first, &mut manifest_bytes)?;
    let manifest = Manifest::from_slice(&manifest_bytes)?;

    let paths = CasePaths::new(dest);
    std::fs::create_dir_all(&paths.root)?;
    std::fs::create_dir_all(paths.blobs())?;

    let mut blobs_restored = 0usize;
    while let Some(entry) = reader.next_entry()? {
        // The destination is chosen from a closed enum, never composed from the
        // name on the wire. See the container module note.
        let target = match &entry.name {
            EntryName::Manifest => {
                return Err(ExportError::Malformed(
                    "the package holds a second manifest".into(),
                ))
            }
            EntryName::Header => paths.header(),
            EntryName::Database => paths.database(),
            EntryName::Blob(address) => {
                blobs_restored += 1;
                paths.blobs().join(kokin_blob::relative_path(address))
            }
        };

        // The blob store shards two levels deep, so the parent may not exist yet.
        // `target` is built from a 64-hex address out of a closed enum, never
        // from bytes on the wire, so this creates nothing a package can name.
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::io::BufWriter::new(File::create(&target)?);
        let actual = reader.read_entry(&entry, &mut file)?;
        std::io::Write::flush(&mut file)?;

        let expected = manifest
            .entries
            .iter()
            .find(|e| e.name == entry.name.as_str());
        match expected {
            Some(e) if e.blake3 == actual && e.size == entry.size => {}
            _ => {
                // Removed rather than left behind: a half-written case directory
                // that opens is worse than none, and this one would.
                let _ = std::fs::remove_dir_all(&paths.root);
                return Err(ExportError::Malformed(format!(
                    "{} does not match the manifest; the package was modified \
                     or is damaged",
                    entry.name.as_str()
                )));
            }
        }
    }

    // Proves the header parses and the passphrase opens it, before the caller is
    // told the import succeeded. Without it a wrong passphrase would surface much
    // later, from a case that looks fine on disk.
    let header = read_header(&paths)?;
    kokin_store::header::unlock_with_passphrase(&header, passphrase)?;

    Ok(ImportReport {
        case_id: manifest.case_id,
        root: paths.root,
        blobs_restored,
        omissions: manifest.omissions,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
