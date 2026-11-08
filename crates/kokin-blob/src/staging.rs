//! Durable no-overwrite publication. Fault implementations exist only in tests.
use super::*;
trait Durable: Write {
    fn sync(&mut self) -> std::io::Result<()>;
}
impl Durable for std::fs::File {
    fn sync(&mut self) -> std::io::Result<()> {
        self.sync_all()
    }
}
trait Io {
    fn create(&self, path: &Path) -> std::io::Result<Box<dyn Durable>>;
    fn publish(&self, from: &Path, to: &Path) -> std::io::Result<bool>;
    fn remove(&self, path: &Path) -> std::io::Result<()>;
}
pub(super) struct Filesystem;
impl Io for Filesystem {
    fn create(&self, path: &Path) -> std::io::Result<Box<dyn Durable>> {
        Ok(Box::new(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?,
        ))
    }
    fn publish(&self, from: &Path, to: &Path) -> std::io::Result<bool> {
        match std::fs::hard_link(from, to) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    }
    fn remove(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }
}
#[allow(private_bounds)]
pub(super) fn put(
    store: &BlobStore,
    mut source: impl Read,
    cmk: &CaseMasterKey,
    io: &impl Io,
) -> Result<PutOutcome> {
    let blob_key = Secret::generate()?;
    let cipher = XChaCha20Poly1305::new(&Key::from(*blob_key.expose()));
    let mut base_nonce = [0u8; NONCE_LEN];
    getrandom_fill(&mut base_nonce)?;
    std::fs::create_dir_all(&store.root)?;
    let temp = store
        .root
        .join(format!(".incoming-{}", hex(&base_nonce[..8])));
    let mut output = io.create(&temp)?;
    let result = (|| -> Result<PutOutcome> {
        let mut hasher = blake3::Hasher::new();
        let mut total = 0u64;
        let mut index = 0u64;
        let mut buffer = vec![0u8; CHUNK_SIZE];
        output.write_all(&base_nonce)?;
        loop {
            let length = read_full(&mut source, &mut buffer)?;
            if length == 0 {
                break;
            }
            total += length as u64;
            if total > store.max_blob_bytes {
                return Err(BlobError::TooLarge {
                    limit: store.max_blob_bytes,
                });
            }
            hasher.update(&buffer[..length]);
            let sealed = cipher
                .encrypt(
                    &XNonce::from(chunk_nonce(&base_nonce, index)),
                    Payload {
                        msg: &buffer[..length],
                        aad: &index.to_le_bytes(),
                    },
                )
                .map_err(|e| BlobError::Crypto(e.to_string()))?;
            output.write_all(&(sealed.len() as u32).to_le_bytes())?;
            output.write_all(&sealed)?;
            index += 1;
        }
        output.flush()?;
        output.sync()?;
        let content_hash = hasher.finalize().to_hex().to_string();
        let wrapped_key = kokin_keys::wrap_cmk(&blob_key, cmk, content_hash.as_bytes())?;
        let destination = store.path_for(&content_hash, cmk);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if io.publish(&temp, &destination)? {
            Ok(PutOutcome::Stored(BlobRef {
                content_hash,
                size: total,
                wrapped_key,
            }))
        } else {
            Ok(PutOutcome::AlreadyPresent {
                content_hash,
                size: total,
            })
        }
    })();
    drop(output);
    match io.remove(&temp) {
        Ok(()) => result,
        Err(cleanup) => Err(BlobError::Cleanup {
            operation: result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_else(|| "completed blob publication".into()),
            cleanup,
        }),
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    enum Fault {
        Short,
        Full,
        Publish,
        Cleanup,
    }
    struct Injected {
        fault: Fault,
    }
    struct Writer {
        file: std::fs::File,
        fail: bool,
    }
    impl Write for Writer {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.fail {
                Err(std::io::Error::from_raw_os_error(28))
            } else {
                self.file.write(&b[..b.len().min(3)])
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.file.flush()
        }
    }
    impl Durable for Writer {
        fn sync(&mut self) -> std::io::Result<()> {
            self.file.sync_all()
        }
    }
    impl Io for Injected {
        fn create(&self, p: &Path) -> std::io::Result<Box<dyn Durable>> {
            Ok(Box::new(Writer {
                file: std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(p)?,
                fail: matches!(self.fault, Fault::Full),
            }))
        }
        fn publish(&self, a: &Path, b: &Path) -> std::io::Result<bool> {
            if matches!(self.fault, Fault::Publish) {
                Err(std::io::Error::other("injected publication failure"))
            } else {
                Filesystem.publish(a, b)
            }
        }
        fn remove(&self, p: &Path) -> std::io::Result<()> {
            if matches!(self.fault, Fault::Cleanup) {
                Err(std::io::Error::other("injected cleanup failure"))
            } else {
                Filesystem.remove(p)
            }
        }
    }
    struct Interrupted(bool);
    impl Read for Interrupted {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if !self.0 {
                self.0 = true;
                buffer.fill(7);
                return Ok(buffer.len());
            }
            Err(std::io::Error::other("interrupted import"))
        }
    }
    fn fixture() -> (BlobStore, CaseMasterKey) {
        let key = CaseMasterKey::generate().unwrap();
        let mut nonce = [0u8; 16];
        getrandom_fill(&mut nonce).unwrap();
        (
            BlobStore::new(std::env::temp_dir().join(format!("kokin-fault-{}", hex(&nonce)))),
            key,
        )
    }
    #[test]
    fn short_writes_are_completed_and_existing_evidence_is_not_overwritten() {
        let (store, key) = fixture();
        let data = b"original source bytes";
        let first = match put(
            &store,
            &data[..],
            &key,
            &Injected {
                fault: Fault::Short,
            },
        )
        .unwrap()
        {
            PutOutcome::Stored(b) => b,
            _ => panic!("expected new blob"),
        };
        assert_eq!(store.get(&first, &key).unwrap(), data);
        assert!(matches!(
            put(&store, &data[..], &key, &Filesystem).unwrap(),
            PutOutcome::AlreadyPresent { .. }
        ));
        assert_eq!(store.get(&first, &key).unwrap(), data);
        std::fs::remove_dir_all(store.root).unwrap();
    }
    #[test]
    fn interrupted_full_disk_and_publication_failures_never_leave_incoming_files() {
        for fault in [Fault::Full, Fault::Publish] {
            let (store, key) = fixture();
            assert!(put(&store, &b"source"[..], &key, &Injected { fault }).is_err());
            assert!(!std::fs::read_dir(&store.root).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".incoming-")));
            std::fs::remove_dir_all(store.root).unwrap();
        }
        let (store, key) = fixture();
        assert!(put(&store, Interrupted(false), &key, &Filesystem).is_err());
        assert_eq!(std::fs::read_dir(&store.root).unwrap().count(), 0);
        std::fs::remove_dir_all(store.root).unwrap();
    }
    #[test]
    fn cleanup_failure_is_an_error_even_after_complete_publication() {
        let (store, key) = fixture();
        assert!(matches!(
            put(
                &store,
                &b"source"[..],
                &key,
                &Injected {
                    fault: Fault::Cleanup
                }
            ),
            Err(BlobError::Cleanup { .. })
        ));
        std::fs::remove_dir_all(store.root).unwrap();
    }
}
