//! From a hash in the database back to the bytes on disk.
//!
//! Every layer above this one holds evidence by content hash: `artifact.blob_hash`,
//! `capture.content_hash`, a search hit, a link in the graph. None of those can be
//! shown to anyone without the wrapped per-blob key, which lives in the `blob`
//! table, and reassembling a [`kokin_blob::BlobRef`] from that row is three
//! nullable columns and two distinctions that are easy to collapse.
//!
//! It was written out once in `kokin-extract` and once again in a test, which is
//! twice, which is how the third copy gets the distinctions wrong.
//!
//! ## The distinctions
//!
//! **No row** means this case has no record of these bytes ever being collected.
//! Reached from an artifact row, that is a broken case.
//!
//! **A row with no key** means the bytes were deliberately destroyed
//! (`docs/limitations/deletion.md`). The record that the evidence existed is
//! intact and is supposed to be; the bytes are gone and are supposed to be. It is
//! a normal state of a healthy case, and reporting it as corruption would send
//! whoever reads the message looking for a problem that does not exist.
//!
//! **A readable ref** means the key is here. It does not mean the ciphertext is:
//! a case directory restored without its `blobs/` will get this far and fail in
//! [`kokin_blob::BlobStore::get`] with `NotFound`. That check is left where it
//! belongs rather than duplicated here, so this function never reports on a file
//! it has not opened.

use rusqlite::Connection;

use crate::Result;

/// What a case can currently do with a set of bytes it once stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobAccess {
    /// The key is present. Read with [`kokin_blob::BlobStore::get`] or `read_to`.
    Readable(kokin_blob::BlobRef),

    /// Crypto-shredded: the row survives, the key does not.
    ///
    /// `shredded_utc` is optional because the schema permits clearing the key
    /// without stamping the time — the append-only trigger enforces the key
    /// going away, not the bookkeeping around it. A caller that needs a date
    /// must handle not having one rather than assume the two always travel
    /// together.
    Shredded { shredded_utc: Option<String> },

    /// No `blob` row for this hash at all.
    Unrecorded,
}

/// A `blob` row, as read. Every column but the size is nullable, and each NULL
/// means something different, which is why they are carried this far as options
/// rather than being unwrapped at the query.
type BlobRow = (i64, Option<Vec<u8>>, Option<Vec<u8>>, Option<String>);

/// Look up what this case can still do with `content_hash`.
///
/// An `Err` here is a database failure and nothing else. The three ordinary
/// answers are all `Ok`, because "we destroyed this on purpose" and "we never
/// had it" are facts about the case, not errors in reading it.
pub fn blob_access(conn: &Connection, content_hash: &str) -> Result<BlobAccess> {
    let row: Option<BlobRow> = conn
        .query_row(
            "SELECT size_bytes, wrapped_key, wrapped_nonce, shredded_utc
               FROM blob
              WHERE content_hash = ?1",
            [content_hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;

    let Some((size, wrapped_key, wrapped_nonce, shredded_utc)) = row else {
        return Ok(BlobAccess::Unrecorded);
    };

    let (Some(ciphertext), Some(nonce)) = (wrapped_key, wrapped_nonce) else {
        return Ok(BlobAccess::Shredded { shredded_utc });
    };

    // A nonce of the wrong length cannot unwrap anything, so this reads as
    // shredded rather than as a readable ref that fails later: the bytes are
    // unreachable either way, and the honest report is the one that does not
    // promise a read that cannot happen.
    let Ok(nonce) = <[u8; kokin_keys::NONCE_LEN]>::try_from(nonce.as_slice()) else {
        return Ok(BlobAccess::Shredded { shredded_utc });
    };

    Ok(BlobAccess::Readable(kokin_blob::BlobRef {
        content_hash: content_hash.to_string(),
        size: size as u64,
        wrapped_key: kokin_keys::WrappedKey { nonce, ciphertext },
    }))
}
