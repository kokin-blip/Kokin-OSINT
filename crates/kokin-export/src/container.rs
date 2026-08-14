//! The package container: a flat, sequential file with four kinds of entry.
//!
//! # Why not tar or zip
//!
//! Every general archive format's headline features are the ones that have to be
//! defended against here. Arbitrary paths bring zip-slip; symlinks and hardlinks
//! bring writes outside the destination; compression brings decompression bombs,
//! which this project already wrote a limiter for once (increment 4). A package
//! written by this crate contains exactly four kinds of entry with names this
//! crate chooses, so all of that machinery would be imported solely to be
//! restricted again.
//!
//! What replaces it is a whitelist (D-035, A-035). [`EntryName`] is a closed enum, parsed from
//! the wire by exact match or by a strict 64-hex blob address, and it is the only
//! way to name an entry. There is no path type anywhere in this module: `..`,
//! `/`, a drive letter and an absolute path are not values [`EntryName`] can
//! hold, so extraction cannot be tricked into writing outside the destination
//! because it never composes a path from untrusted bytes at all.
//!
//! Nothing is compressed. The payload is AEAD ciphertext and a SQLCipher
//! database; both are incompressible, so compression would cost CPU, buy nothing,
//! and reintroduce the one attack the format is otherwise immune to.
//!
//! # Layout
//!
//! ```text
//! "KOKINPKG1"       9 bytes, magic and format version
//! entry_count       u32 LE
//! per entry:
//!   name_len        u16 LE
//!   name            name_len bytes of ASCII
//!   size            u64 LE
//!   data            size bytes
//! ```
//!
//! The manifest is written first so [`crate::verify`] can read what it is
//! checking before it reads anything it is checking against.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::{ExportError, Result};

pub const MAGIC: &[u8; 9] = b"KOKINPKG1";

/// The largest entry name this format can express, used to reject a length field
/// before it is trusted enough to allocate against.
const MAX_NAME_LEN: u16 = 128;

/// A hard ceiling on entry count, so a corrupt header cannot ask for an
/// allocation before a single entry has been read.
const MAX_ENTRIES: u32 = 5_000_000;

/// Every name a package may contain.
///
/// Closed on purpose. A package holding anything else is not a package this
/// build wrote, and [`EntryName::parse`] is the only constructor from bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryName {
    Manifest,
    Header,
    Database,
    /// A blob, addressed by its keyed storage name — 64 lowercase hex
    /// characters. Not the content hash: `kokin_blob::storage_name` keys the
    /// name with the case master key so a directory listing leaks no membership,
    /// and that property has to survive the trip.
    Blob(String),
}

pub const MANIFEST_NAME: &str = "manifest.json";
pub const HEADER_NAME: &str = "header.json";
pub const DATABASE_NAME: &str = "case.db";
pub const BLOB_PREFIX: &str = "blobs/";

impl EntryName {
    pub fn as_str(&self) -> String {
        match self {
            Self::Manifest => MANIFEST_NAME.to_string(),
            Self::Header => HEADER_NAME.to_string(),
            Self::Database => DATABASE_NAME.to_string(),
            Self::Blob(name) => format!("{BLOB_PREFIX}{name}"),
        }
    }

    /// The only way to build an `EntryName` from untrusted bytes.
    ///
    /// Rejects anything not on the list, and a blob address that is not exactly
    /// 64 lowercase hex characters. Uppercase is refused rather than folded: two
    /// spellings of one address would be two entries on a case-sensitive
    /// filesystem and one on Windows, and that difference is a way to smuggle a
    /// second file past a manifest that lists the address once.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            MANIFEST_NAME => return Ok(Self::Manifest),
            HEADER_NAME => return Ok(Self::Header),
            DATABASE_NAME => return Ok(Self::Database),
            _ => {}
        }

        let Some(address) = raw.strip_prefix(BLOB_PREFIX) else {
            return Err(ExportError::IllegalEntryName {
                raw: raw.to_string(),
            });
        };
        let legal = address.len() == 64
            && address
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !legal {
            return Err(ExportError::IllegalEntryName {
                raw: raw.to_string(),
            });
        }
        Ok(Self::Blob(address.to_string()))
    }
}

/// Writes entries in the order they are given.
pub struct Writer {
    out: BufWriter<File>,
    entries: u32,
    count_at: u64,
}

impl Writer {
    pub fn create(path: &Path) -> Result<Self> {
        let file = File::create(path)?;
        let mut out = BufWriter::new(file);
        out.write_all(MAGIC)?;
        let count_at = MAGIC.len() as u64;
        // Backfilled by `finish`, because the count is not known until the caller
        // has finished streaming and holding every blob in memory to count them
        // first is exactly what this format exists to avoid.
        out.write_all(&0u32.to_le_bytes())?;
        Ok(Self {
            out,
            entries: 0,
            count_at,
        })
    }

    /// Open an entry and return a sink for its bytes.
    ///
    /// The streaming form. `assemble` needs it because the container's other
    /// direction hands bytes to a [`Write`], and bridging the two by buffering
    /// would hold an entry - which may be the entire case database - in memory.
    pub fn begin_entry(&mut self, name: &EntryName, size: u64) -> Result<EntryWriter<'_>> {
        let raw = name.as_str();
        let name_len = u16::try_from(raw.len())
            .map_err(|_| ExportError::IllegalEntryName { raw: raw.clone() })?;
        self.out.write_all(&name_len.to_le_bytes())?;
        self.out.write_all(raw.as_bytes())?;
        self.out.write_all(&size.to_le_bytes())?;
        self.entries += 1;

        Ok(EntryWriter {
            out: &mut self.out,
            hasher: blake3::Hasher::new(),
            name: raw,
            declared: size,
            written: 0,
        })
    }

    /// Stream one entry in, returning the BLAKE3 of what was actually written.
    ///
    /// The hash is computed here rather than by the caller so the manifest can
    /// only ever describe the bytes that went into the file. A caller that hashed
    /// its own buffer and then wrote a different one would produce a package that
    /// verifies against a lie.
    pub fn write_entry(
        &mut self,
        name: &EntryName,
        size: u64,
        mut data: impl Read,
    ) -> Result<String> {
        let mut entry = self.begin_entry(name, size)?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = data.read(&mut buf)?;
            if n == 0 {
                break;
            }
            entry.write_all(&buf[..n])?;
        }
        entry.finish()
    }

    pub fn finish(mut self) -> Result<()> {
        self.out.flush()?;
        let mut file = self.out.into_inner().map_err(std::io::Error::from)?;
        file.seek(SeekFrom::Start(self.count_at))?;
        file.write_all(&self.entries.to_le_bytes())?;
        file.sync_all()?;
        Ok(())
    }
}

/// An open entry. Bytes written here are hashed as they go.
pub struct EntryWriter<'a> {
    out: &'a mut BufWriter<File>,
    hasher: blake3::Hasher,
    name: String,
    declared: u64,
    written: u64,
}

impl EntryWriter<'_> {
    /// Close the entry, returning the BLAKE3 of what was written.
    ///
    /// A short write leaves the container structurally broken - every following
    /// entry would be parsed at the wrong offset - so it fails here, while the
    /// only thing lost is a package nobody has yet been handed.
    pub fn finish(self) -> Result<String> {
        if self.written != self.declared {
            return Err(ExportError::SizeChangedDuringWrite {
                name: self.name,
                declared: self.declared,
                written: self.written,
            });
        }
        Ok(self.hasher.finalize().to_hex().to_string())
    }
}

impl Write for EntryWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.out.write_all(buf)?;
        self.hasher.update(buf);
        self.written += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// One entry's position and shape, without its bytes.
#[derive(Debug, Clone)]
pub struct EntryHeader {
    pub name: EntryName,
    pub size: u64,
    /// Offset of the entry's data within the package file.
    pub offset: u64,
}

/// Reads entries in file order.
pub struct Reader {
    input: BufReader<File>,
    remaining: u32,
    position: u64,
}

impl Reader {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mut input = BufReader::new(file);

        let mut magic = [0u8; MAGIC.len()];
        input
            .read_exact(&mut magic)
            .map_err(|_| ExportError::NotAPackage)?;
        if &magic != MAGIC {
            return Err(ExportError::NotAPackage);
        }

        let count = read_u32(&mut input)?;
        if count > MAX_ENTRIES {
            return Err(ExportError::Malformed(format!(
                "the package claims {count} entries"
            )));
        }

        Ok(Self {
            input,
            remaining: count,
            position: MAGIC.len() as u64 + 4,
        })
    }

    /// Read the next entry's header, leaving the reader positioned at its data.
    pub fn next_entry(&mut self) -> Result<Option<EntryHeader>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;

        let name_len = read_u16(&mut self.input)?;
        if name_len == 0 || name_len > MAX_NAME_LEN {
            return Err(ExportError::Malformed(format!(
                "entry name length {name_len} is out of range"
            )));
        }
        let mut name_bytes = vec![0u8; name_len as usize];
        self.input.read_exact(&mut name_bytes)?;
        let raw = String::from_utf8(name_bytes)
            .map_err(|_| ExportError::Malformed("entry name is not UTF-8".into()))?;
        let name = EntryName::parse(&raw)?;

        let size = read_u64(&mut self.input)?;
        self.position += 2 + name_len as u64 + 8;
        let offset = self.position;
        self.position += size;

        Ok(Some(EntryHeader { name, size, offset }))
    }

    /// Stream the current entry's bytes into `sink`, hashing as it goes.
    ///
    /// Must be called exactly once per `next_entry`, before the next call — the
    /// reader is sequential, and skipping is [`Self::skip`].
    pub fn read_entry(&mut self, entry: &EntryHeader, mut sink: impl Write) -> Result<String> {
        let mut hasher = blake3::Hasher::new();
        let mut left = entry.size;
        let mut buf = vec![0u8; 64 * 1024];
        while left > 0 {
            let want = buf.len().min(left as usize);
            // read_exact, not read: the size came from the file and a truncated
            // package must be reported as truncated rather than silently
            // producing a short entry whose hash then fails for the wrong reason.
            self.input
                .read_exact(&mut buf[..want])
                .map_err(|_| ExportError::Truncated {
                    name: entry.name.as_str(),
                })?;
            hasher.update(&buf[..want]);
            sink.write_all(&buf[..want])?;
            left -= want as u64;
        }
        Ok(hasher.finalize().to_hex().to_string())
    }

    pub fn skip(&mut self, entry: &EntryHeader) -> Result<()> {
        self.read_entry(entry, std::io::sink()).map(|_| ())
    }
}

fn read_u16(input: &mut impl Read) -> Result<u16> {
    let mut b = [0u8; 2];
    input.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32(input: &mut impl Read) -> Result<u32> {
    let mut b = [0u8; 4];
    input.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(input: &mut impl Read) -> Result<u64> {
    let mut b = [0u8; 8];
    input.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_not_on_the_list_is_refused() {
        for raw in [
            "../escape",
            "blobs/../escape",
            "/etc/passwd",
            "C:\\windows\\system32",
            "blobs/short",
            // Uppercase hex: the same address in a spelling that is a second
            // file on Linux and the same file on Windows.
            "blobs/AAAA111122223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF",
            "blobs/",
            "manifest.json.bak",
            "",
        ] {
            assert!(
                EntryName::parse(raw).is_err(),
                "{raw} was accepted as an entry name"
            );
        }
    }

    #[test]
    fn the_names_this_format_writes_round_trip() {
        let blob = "a".repeat(64);
        for name in [
            EntryName::Manifest,
            EntryName::Header,
            EntryName::Database,
            EntryName::Blob(blob),
        ] {
            assert_eq!(EntryName::parse(&name.as_str()).unwrap(), name);
        }
    }
}
