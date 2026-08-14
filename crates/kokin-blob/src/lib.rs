//! Content-addressed, encrypted blob storage.
//!
//! Large binary evidence does not live in the case database (ADR-0003). It goes
//! here: hashed on write, encrypted under a per-blob key, and stored under a
//! name that reveals nothing.
//!
//! ```text
//! MyCase.kokincase/blobs/ab/cd/abcd… .blob
//! ```
//!
//! # Three properties worth stating plainly
//!
//! **Hash-on-write, streaming.** The plaintext is hashed and encrypted in one
//! pass, in bounded chunks. Nothing is ever fully buffered, because "evidence"
//! includes multi-gigabyte disk images and video.
//!
//! **The on-disk name is not the content hash.** Blobs are addressed internally
//! by `BLAKE3(plaintext)`, which is what makes deduplication work — but that
//! value is *derived from the content*, so publishing it as a filename would let
//! anyone with filesystem access confirm a guess ("is this exact image in this
//! case?"). The stored name is instead `BLAKE3_keyed(cmk, content_hash)`, which
//! is meaningless without the case key. See [`storage_name`].
//!
//! **Per-blob keys make crypto-shredding possible.** Each blob is encrypted
//! under its own key, wrapped by the case master key and stored as metadata. To
//! destroy one piece of evidence irreversibly, destroy that wrapped key: the
//! hash, size, and lineage survive, so the record that it *existed* is intact.
//! The honest cost is that its integrity can then never be verified again, and
//! any export containing it verifies as `PARTIAL`, never `OK`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use kokin_keys::{CaseMasterKey, Secret, KEY_LEN, NONCE_LEN};

/// Plaintext bytes encrypted per chunk.
///
/// 64 KiB balances AEAD overhead (16 bytes per chunk) against memory: a 1 GiB
/// blob costs 16 KiB of tags and one 64 KiB buffer, not 1 GiB of RAM.
pub const CHUNK_SIZE: usize = 64 * 1024;

/// AEAD tag length for XChaCha20-Poly1305.
const TAG_LEN: usize = 16;

/// Default ceiling on a single blob. Callers doing bulk import raise it
/// deliberately; the point is that there is always *a* limit.
pub const DEFAULT_MAX_BLOB_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("key error: {0}")]
    Key(#[from] kokin_keys::KeyError),

    #[error("blob exceeds the {limit} byte limit")]
    TooLarge { limit: u64 },

    #[error("blob {hash} is not in this store")]
    NotFound { hash: String },

    #[error(
        "blob {hash} failed authentication at chunk {chunk} - the file was modified or truncated"
    )]
    Corrupt { hash: String, chunk: u64 },

    #[error("stored blob does not match its content hash: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("crypto error: {0}")]
    Crypto(String),
}

pub type Result<T> = std::result::Result<T, BlobError>;

/// What the caller must persist to read a blob back.
///
/// `wrapped_key` is the per-blob key sealed under the case master key.
/// **Destroying it is the crypto-shred operation** — everything else here
/// survives, so the record that this evidence existed is preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    /// `BLAKE3(plaintext)`, lowercase hex. The deduplication key.
    pub content_hash: String,
    /// Plaintext length.
    pub size: u64,
    pub wrapped_key: kokin_keys::WrappedKey,
}

/// The result of a [`BlobStore::put`].
///
/// Deduplication has to be explicit rather than silent. Each blob is encrypted
/// under its own random key, so identical content written twice produces two
/// different ciphertexts. If `put` quietly discarded the second copy and handed
/// back a ref carrying the *new* key, that ref could not decrypt the *stored*
/// file - which is precisely the bug this enum was introduced to fix.
///
/// On `AlreadyPresent` the caller looks up the `BlobRef` it already holds for
/// that `content_hash` and reuses it. That is the correct behaviour anyway: the
/// point of dedupe is that one set of bytes is stored once and referenced by
/// many pieces of provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutOutcome {
    /// New content, now stored. Persist this ref.
    Stored(BlobRef),
    /// These bytes are already in the store. Reuse the existing ref.
    AlreadyPresent { content_hash: String, size: u64 },
}

impl PutOutcome {
    /// The content hash, whichever outcome this is.
    pub fn content_hash(&self) -> &str {
        match self {
            Self::Stored(b) => &b.content_hash,
            Self::AlreadyPresent { content_hash, .. } => content_hash,
        }
    }
}

/// A content-addressed blob directory.
#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
    max_blob_bytes: u64,
}

impl BlobStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_path_buf(),
            max_blob_bytes: DEFAULT_MAX_BLOB_BYTES,
        }
    }

    /// Raise or lower the per-blob ceiling.
    pub fn with_max_blob_bytes(mut self, limit: u64) -> Self {
        self.max_blob_bytes = limit;
        self
    }

    /// Write a blob, hashing and encrypting in one streaming pass.
    ///
    /// The size limit is enforced **as bytes are read**, not after, so a hostile
    /// or runaway stream is stopped at the limit rather than after it has
    /// already been written to disk (attack catalogue A-011).
    ///
    /// Writing content that is already stored returns
    /// [`PutOutcome::AlreadyPresent`]; see that type for why dedupe cannot be
    /// silent here.
    pub fn put(&self, mut source: impl Read, cmk: &CaseMasterKey) -> Result<PutOutcome> {
        let blob_key = Secret::generate()?;
        let cipher = XChaCha20Poly1305::new(&Key::from(*blob_key.expose()));

        // One random base nonce per blob; each chunk uses base ^ counter in the
        // trailing bytes. The 192-bit nonce is wide enough that a random base
        // needs no bookkeeping across blobs.
        let mut base_nonce = [0u8; NONCE_LEN];
        getrandom_fill(&mut base_nonce)?;

        let temp_path = self
            .root
            .join(format!(".incoming-{}", hex(&base_nonce[..8])));
        std::fs::create_dir_all(&self.root)?;

        let mut hasher = blake3::Hasher::new();
        let mut total: u64 = 0;
        let mut chunk_index: u64 = 0;
        let mut buf = vec![0u8; CHUNK_SIZE];

        // Scoped so the file is closed before the rename below.
        {
            let mut out = std::fs::File::create(&temp_path)?;
            out.write_all(&base_nonce)?;

            loop {
                let n = read_full(&mut source, &mut buf)?;
                if n == 0 {
                    break;
                }

                total += n as u64;
                if total > self.max_blob_bytes {
                    drop(out);
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(BlobError::TooLarge {
                        limit: self.max_blob_bytes,
                    });
                }

                hasher.update(&buf[..n]);

                let nonce = chunk_nonce(&base_nonce, chunk_index);
                let sealed = cipher
                    .encrypt(
                        &XNonce::from(nonce),
                        Payload {
                            msg: &buf[..n],
                            // The chunk index is authenticated, so chunks cannot
                            // be reordered or dropped without detection.
                            aad: &chunk_index.to_le_bytes(),
                        },
                    )
                    .map_err(|e| BlobError::Crypto(e.to_string()))?;

                out.write_all(&(sealed.len() as u32).to_le_bytes())?;
                out.write_all(&sealed)?;
                chunk_index += 1;
            }

            out.flush()?;
        }

        let content_hash = hasher.finalize().to_hex().to_string();
        let final_path = self.path_for(&content_hash, cmk);

        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if final_path.exists() {
            // These bytes are already stored, under a different per-blob key.
            // Discard what we just wrote and tell the caller to reuse the ref it
            // already has: overwriting would silently break every existing ref
            // to this content, and returning a ref carrying our new key would
            // hand back something that cannot decrypt the stored file.
            let _ = std::fs::remove_file(&temp_path);
            return Ok(PutOutcome::AlreadyPresent {
                content_hash,
                size: total,
            });
        }

        std::fs::rename(&temp_path, &final_path)?;

        let wrapped_key = kokin_keys::wrap_cmk(&blob_key, cmk, content_hash.as_bytes())?;

        Ok(PutOutcome::Stored(BlobRef {
            content_hash,
            size: total,
            wrapped_key,
        }))
    }

    /// Read a blob back, verifying every chunk and the whole-content hash.
    ///
    /// Returns the plaintext. A caller streaming very large evidence should use
    /// [`BlobStore::read_to`] instead.
    pub fn get(&self, blob: &BlobRef, cmk: &CaseMasterKey) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(blob.size as usize);
        self.read_to(blob, cmk, &mut out)?;
        Ok(out)
    }

    /// Stream a blob into `sink`, verifying as it goes.
    ///
    /// The content hash is checked at the end. A truncated file is caught by the
    /// hash even though every surviving chunk authenticates, which is why both
    /// checks exist.
    pub fn read_to(
        &self,
        blob: &BlobRef,
        cmk: &CaseMasterKey,
        sink: &mut impl Write,
    ) -> Result<()> {
        let path = self.path_for(&blob.content_hash, cmk);
        if !path.exists() {
            return Err(BlobError::NotFound {
                hash: blob.content_hash.clone(),
            });
        }

        let blob_key =
            kokin_keys::unwrap_cmk(&blob.wrapped_key, cmk, blob.content_hash.as_bytes())?;
        let cipher = XChaCha20Poly1305::new(&Key::from(*blob_key.expose()));

        let mut file = std::fs::File::open(&path)?;
        let mut base_nonce = [0u8; NONCE_LEN];
        file.read_exact(&mut base_nonce)?;

        let mut hasher = blake3::Hasher::new();
        let mut chunk_index: u64 = 0;

        loop {
            let mut len_bytes = [0u8; 4];
            match file.read_exact(&mut len_bytes) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(BlobError::Io(e)),
            }

            let sealed_len = u32::from_le_bytes(len_bytes) as usize;
            // A length outside this range means the framing is damaged, and
            // acting on it would mean allocating whatever a corrupt or hostile
            // file claims.
            if !(TAG_LEN..=CHUNK_SIZE + TAG_LEN).contains(&sealed_len) {
                return Err(BlobError::Corrupt {
                    hash: blob.content_hash.clone(),
                    chunk: chunk_index,
                });
            }

            let mut sealed = vec![0u8; sealed_len];
            file.read_exact(&mut sealed)?;

            let nonce = chunk_nonce(&base_nonce, chunk_index);
            let plain = cipher
                .decrypt(
                    &XNonce::from(nonce),
                    Payload {
                        msg: &sealed,
                        aad: &chunk_index.to_le_bytes(),
                    },
                )
                .map_err(|_| BlobError::Corrupt {
                    hash: blob.content_hash.clone(),
                    chunk: chunk_index,
                })?;

            hasher.update(&plain);
            sink.write_all(&plain)?;
            chunk_index += 1;
        }

        let actual = hasher.finalize().to_hex().to_string();
        if actual != blob.content_hash {
            return Err(BlobError::HashMismatch {
                expected: blob.content_hash.clone(),
                actual,
            });
        }

        Ok(())
    }

    /// True if this content is already stored.
    pub fn contains(&self, content_hash: &str, cmk: &CaseMasterKey) -> bool {
        self.path_for(content_hash, cmk).exists()
    }

    /// Remove a blob's ciphertext.
    ///
    /// This is **not** the crypto-shred operation on its own. Shredding destroys
    /// the wrapped key that the caller holds; this only reclaims disk. Deleting
    /// the file without destroying the key leaves the evidence recoverable from
    /// any backup, which is exactly the false assurance to avoid.
    pub fn remove(&self, content_hash: &str, cmk: &CaseMasterKey) -> Result<()> {
        let path = self.path_for(content_hash, cmk);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn path_for(&self, content_hash: &str, cmk: &CaseMasterKey) -> PathBuf {
        self.root
            .join(relative_path(&storage_name(content_hash, cmk)))
    }
}

/// Where a blob with this storage name sits, relative to the store root.
///
/// Public because `kokin-export` has to find blob files on disk from a storage
/// name and put them back in the same place on the other side. It could compute
/// `ab/cd/name.blob` itself, and then the two crates would hold the same layout
/// rule in two places — so changing the fan-out here would silently produce
/// packages that import into a case whose blobs are all unfindable, with every
/// hash still matching. One function, one rule.
///
/// Panics if `storage_name` is shorter than four characters, which cannot happen
/// for a value from [`storage_name`] — it is always 64 hex characters.
pub fn relative_path(storage_name: &str) -> PathBuf {
    // Two levels of fan-out: 256 x 256 directories keeps any one directory
    // small enough that filesystem listing stays fast at scale.
    PathBuf::from(&storage_name[0..2])
        .join(&storage_name[2..4])
        .join(format!("{storage_name}.blob"))
}

/// The on-disk name for a blob: `BLAKE3_keyed(cmk, content_hash)`.
///
/// Deliberately *not* the content hash. A content hash is derived from the
/// bytes, so an attacker with filesystem access and a candidate file could
/// confirm its presence in a case simply by hashing it and looking for the
/// name — the case is encrypted, but the directory listing would leak
/// membership. Keying the name defeats that: without the case master key the
/// filenames are indistinguishable from random.
pub fn storage_name(content_hash: &str, cmk: &CaseMasterKey) -> String {
    blake3::keyed_hash(cmk.expose(), content_hash.as_bytes())
        .to_hex()
        .to_string()
}

/// Per-chunk nonce: the base nonce with the counter mixed into the last 8 bytes.
///
/// The base is random per blob and the counter is unique per chunk, so no
/// (key, nonce) pair repeats.
fn chunk_nonce(base: &[u8; NONCE_LEN], index: u64) -> [u8; NONCE_LEN] {
    let mut nonce = *base;
    let counter = index.to_le_bytes();
    for (slot, byte) in nonce[NONCE_LEN - 8..].iter_mut().zip(counter) {
        *slot ^= byte;
    }
    nonce
}

/// Read until the buffer is full or the source ends.
///
/// `Read::read` may legally return fewer bytes than requested at any time, so
/// reading once per chunk would produce short chunks and inflate AEAD overhead
/// on a slow or chunked source such as a network stream.
fn read_full(source: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match source.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

fn getrandom_fill(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf).map_err(|e| BlobError::Crypto(e.to_string()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

const _: () = assert!(KEY_LEN == 32);

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_store(name: &str) -> (BlobStore, PathBuf) {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("kokin-blob-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        (BlobStore::new(&p), p)
    }

    fn cmk() -> CaseMasterKey {
        CaseMasterKey::generate().unwrap()
    }

    /// Most tests store new content and want the ref; a dedupe hit there would
    /// be a test bug, so it panics rather than being silently tolerated.
    fn put_new(store: &BlobStore, data: &[u8], key: &CaseMasterKey) -> BlobRef {
        match store.put(data, key).unwrap() {
            PutOutcome::Stored(b) => b,
            PutOutcome::AlreadyPresent { content_hash, .. } => {
                panic!("expected new content, but {content_hash} was already stored")
            }
        }
    }

    #[test]
    fn a_blob_round_trips() {
        let (store, dir) = temp_store("roundtrip");
        let key = cmk();
        let data = b"evidence bytes".to_vec();

        let blob = put_new(&store, &data, &key);
        assert_eq!(blob.size, data.len() as u64);
        assert_eq!(store.get(&blob, &key).unwrap(), data);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Spans many chunks, so the chunk framing and nonce counter are exercised
    /// rather than just the single-chunk path.
    #[test]
    fn a_multi_chunk_blob_round_trips() {
        let (store, dir) = temp_store("multichunk");
        let key = cmk();
        // Deliberately not a chunk multiple, so the final short chunk is covered.
        let data: Vec<u8> = (0..CHUNK_SIZE * 3 + 1234)
            .map(|i| (i % 251) as u8)
            .collect();

        let blob = put_new(&store, &data, &key);
        assert_eq!(blob.size, data.len() as u64);
        assert_eq!(store.get(&blob, &key).unwrap(), data);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_empty_blob_round_trips() {
        let (store, dir) = temp_store("empty");
        let key = cmk();

        let blob = put_new(&store, b"", &key);
        assert_eq!(blob.size, 0);
        assert_eq!(store.get(&blob, &key).unwrap(), Vec::<u8>::new());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn identical_content_hashes_identically() {
        let (store, dir) = temp_store("dedupe");
        let key = cmk();

        let a = put_new(&store, b"same bytes", &key);
        let b = store.put(&b"same bytes"[..], &key).unwrap();

        match b {
            PutOutcome::AlreadyPresent { content_hash, size } => {
                assert_eq!(content_hash, a.content_hash);
                assert_eq!(size, a.size);
            }
            PutOutcome::Stored(_) => panic!("identical content was stored twice"),
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The property that stops a directory listing leaking case membership.
    /// Regression probe: after a dedupe hit, the returned ref must actually
    /// read back.
    #[test]
    fn a_deduplicated_blob_can_still_be_read() {
        let (store, dir) = temp_store("dedupe_read");
        let key = cmk();

        let first = put_new(&store, b"same bytes", &key);

        // The second write reports a dedupe hit rather than handing back a ref
        // whose key cannot decrypt the stored ciphertext. Before PutOutcome
        // existed, it returned such a ref and this read failed.
        assert!(matches!(
            store.put(&b"same bytes"[..], &key).unwrap(),
            PutOutcome::AlreadyPresent { .. }
        ));

        assert_eq!(store.get(&first, &key).unwrap(), b"same bytes".to_vec());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_on_disk_name_is_not_the_content_hash() {
        let (store, dir) = temp_store("opaque");
        let key = cmk();

        let blob = put_new(&store, b"secret evidence", &key);

        let mut names = Vec::new();
        for a in std::fs::read_dir(&dir).unwrap() {
            let a = a.unwrap();
            if !a.file_type().unwrap().is_dir() {
                continue;
            }
            for b in std::fs::read_dir(a.path()).unwrap() {
                for f in std::fs::read_dir(b.unwrap().path()).unwrap() {
                    names.push(f.unwrap().file_name().to_string_lossy().to_string());
                }
            }
        }

        assert_eq!(names.len(), 1, "expected one stored blob, found {names:?}");
        assert!(
            !names[0].contains(&blob.content_hash),
            "the content hash appears in the filename - anyone with filesystem \
             access could confirm which files a case contains"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two cases holding identical evidence must not produce the same filename,
    /// or the keyed name would leak across cases.
    #[test]
    fn the_storage_name_differs_between_cases() {
        let a = cmk();
        let b = cmk();
        let hash = "0".repeat(64);
        assert_ne!(storage_name(&hash, &a), storage_name(&hash, &b));
    }

    #[test]
    fn the_size_limit_is_enforced_during_the_read() {
        let (store, dir) = temp_store("toolarge");
        let store = store.with_max_blob_bytes(1024);
        let key = cmk();
        let data = vec![0u8; 4096];

        match store.put(&data[..], &key) {
            Err(BlobError::TooLarge { limit: 1024 }) => {}
            other => panic!("expected TooLarge, got {other:?}"),
        }

        // Nothing partial is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".incoming"))
            .collect();
        assert!(leftovers.is_empty(), "a partial write was left on disk");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_modified_blob_fails_authentication() {
        let (store, dir) = temp_store("tamper");
        let key = cmk();
        let blob = put_new(&store, b"original evidence", &key);

        let path = store.path_for(&blob.content_hash, &key);
        let mut bytes = std::fs::read(&path).unwrap();
        // Flip a bit inside the first chunk's ciphertext, past the nonce and
        // length prefix.
        let target = NONCE_LEN + 4 + 2;
        bytes[target] ^= 0x01;
        std::fs::write(&path, bytes).unwrap();

        match store.get(&blob, &key) {
            Err(BlobError::Corrupt { chunk: 0, .. }) => {}
            other => panic!("expected Corrupt at chunk 0, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A truncated file has no damaged chunk to fail on - every surviving chunk
    /// authenticates. Only the whole-content hash catches it, which is why the
    /// final check is not redundant with per-chunk AEAD.
    #[test]
    fn a_truncated_blob_is_caught_by_the_content_hash() {
        let (store, dir) = temp_store("truncate");
        let key = cmk();
        let data: Vec<u8> = (0..CHUNK_SIZE * 3).map(|i| (i % 251) as u8).collect();
        let blob = put_new(&store, &data, &key);

        let path = store.path_for(&blob.content_hash, &key);
        let bytes = std::fs::read(&path).unwrap();
        // Drop the last chunk entirely, leaving a well-formed shorter file.
        let keep = NONCE_LEN + 2 * (4 + CHUNK_SIZE + TAG_LEN);
        std::fs::write(&path, &bytes[..keep]).unwrap();

        match store.get(&blob, &key) {
            Err(BlobError::HashMismatch { .. }) => {}
            other => panic!("expected HashMismatch, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_wrong_case_key_cannot_read_a_blob() {
        let (store, dir) = temp_store("wrongkey");
        let key = cmk();
        let other = cmk();
        let blob = put_new(&store, b"evidence", &key);

        assert!(store.get(&blob, &other).is_err());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Crypto-shredding: destroy the wrapped key and the bytes are unreadable
    /// even though the ciphertext is still on disk.
    #[test]
    fn destroying_the_wrapped_key_makes_a_blob_unreadable() {
        let (store, dir) = temp_store("shred");
        let key = cmk();
        let blob = put_new(&store, b"evidence to shred", &key);
        assert!(store.get(&blob, &key).is_ok());

        // The caller would delete this from the blob row; simulate by corrupting
        // the wrap it holds.
        let mut shredded = blob.clone();
        shredded.wrapped_key.ciphertext[0] ^= 0xff;

        assert!(store.get(&shredded, &key).is_err());
        // The ciphertext and therefore the record that it existed is still there.
        assert!(store.contains(&blob.content_hash, &key));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_blob_is_named_as_such() {
        let (store, dir) = temp_store("missing");
        let key = cmk();
        let blob = put_new(&store, b"gone", &key);
        store.remove(&blob.content_hash, &key).unwrap();

        match store.get(&blob, &key) {
            Err(BlobError::NotFound { .. }) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A source that returns one byte at a time must still produce full chunks.
    #[test]
    fn a_dribbling_source_still_produces_full_chunks() {
        struct OneByteAtATime<'a>(&'a [u8]);
        impl Read for OneByteAtATime<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() || buf.is_empty() {
                    return Ok(0);
                }
                buf[0] = self.0[0];
                self.0 = &self.0[1..];
                Ok(1)
            }
        }

        let (store, dir) = temp_store("dribble");
        let key = cmk();
        let data: Vec<u8> = (0..CHUNK_SIZE + 100).map(|i| (i % 251) as u8).collect();

        let blob = match store.put(OneByteAtATime(&data), &key).unwrap() {
            PutOutcome::Stored(b) => b,
            other => panic!("expected Stored, got {other:?}"),
        };
        assert_eq!(blob.size, data.len() as u64);
        assert_eq!(store.get(&blob, &key).unwrap(), data);

        // Two chunks, not CHUNK_SIZE+100 one-byte chunks.
        let path = store.path_for(&blob.content_hash, &key);
        let on_disk = std::fs::metadata(&path).unwrap().len();
        let expected = (NONCE_LEN + (4 + CHUNK_SIZE + TAG_LEN) + (4 + 100 + TAG_LEN)) as u64;
        assert_eq!(on_disk, expected);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
