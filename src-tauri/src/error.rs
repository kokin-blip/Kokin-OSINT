//! What a failed command tells the interface.
//!
//! Every command returns `Result<T, CommandError>`, and a `CommandError` is a
//! **code** plus a message. The split is the whole point: the code is what the
//! interface branches on, and the message is what it shows a person. Returning
//! a bare string — which is what this crate did until now — forces the UI to
//! match on prose, and prose is exactly the thing that gets reworded. A
//! wrong-passphrase dialog that stops appearing because someone improved an
//! error message is a bug nobody sees in review.
//!
//! Codes are a closed enum. Adding one is a deliberate act with a compile error
//! attached, and `Internal` is not a place to put things that deserve a code.
//!
//! # What a message may not contain
//!
//! Anything derived from key material, and anything from inside a case that the
//! caller has not already been shown. Store errors are mapped by variant here
//! rather than stringified wholesale, so a new variant carrying something
//! sensitive cannot reach the interface by inheritance.

use serde::Serialize;

/// The closed set of failures the interface can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The command needs an open case and there is none. Not an error the user
    /// caused: it is the state after a close, or after a failed open.
    NoCaseOpen,

    /// A case is already open. Opening a second one is refused rather than
    /// performed, because "which case am I looking at" is the question this
    /// product may never get wrong.
    CaseAlreadyOpen,

    /// The passphrase did not unlock the case. Deliberately indistinguishable
    /// from a damaged wrapped key, as in `StoreError`: telling an attacker
    /// which one they hit is an oracle.
    WrongPassphrase,

    /// The recovery key as typed is not a recovery key — a wrong character, or
    /// the wrong number of them. Caught before any derivation runs.
    MistypedRecoveryKey,

    /// The path exists and is not a Kokin case, or its header is damaged.
    NotACase,

    /// Something is already at the path a new case would occupy. Never
    /// overwritten.
    CaseExists,

    /// The case opened but the database refused the operation. Distinct from
    /// the codes above because none of the user's obvious remedies apply.
    Storage,

    /// No row with that id in this case. A stale link, a deleted row, or an
    /// interface holding an id from a case that is no longer open — all of which
    /// the UI can act on, and none of which mean the case is damaged.
    NotFound,

    /// The full-text index no longer matches the case. The remedy is a rebuild,
    /// and it is nothing like the other storage failures: the case's own data is
    /// intact and only the derived index is wrong, so an interface can offer to
    /// fix it rather than telling the analyst their case is broken.
    SearchIndexCorrupt,

    /// The write cited nothing. Every analytical row in this product hangs on
    /// evidence, and the remedy is specific and available: pick the observation
    /// this claim comes from. An interface receiving this must open the evidence
    /// picker, which is a different response from any other failure here.
    EvidenceRequired,

    /// The write recorded a judgement and gave no reason for it. This layer's own
    /// rule (D-033), not the schema's — see [`crate::write`].
    ExplanationRequired,

    /// These two entities are already one. Not a failure of anything: the case
    /// moved on, or the interface was looking at a stale list. The interface can
    /// act on it by opening the entity they resolve to, which is why it is not
    /// folded in with the refusal below.
    AlreadyMerged,

    /// This entity was never merged into anything, so there is no merge to
    /// undo. Nothing for the interface to offer, which is exactly why it is a
    /// different code from [`ErrorCode::AlreadyMerged`].
    NotMerged,

    /// The value is not on the scale, or not on the version of the scale this
    /// case holds. Usually an interface offering options it read before the case
    /// was migrated; the remedy is to re-read the scale, not to retry.
    InvalidValue,

    /// A `rule:` or `ai:` actor tried to do something only a person may do
    /// (ADR-0008). The product working, not failing.
    ///
    /// **No command in this crate can produce this**, because no command lets its
    /// caller name the actor — see [`crate::write::ACTOR`]. It is mapped because
    /// the mapping must be right for the in-process callers that will come, and
    /// because routing a refusal through `Storage` is how a deliberate boundary
    /// comes to look like a database fault.
    MachineMayNotAct,

    /// The file the user chose could not be read. Theirs to fix: a different
    /// file, or a permission on this one.
    FileUnreadable,

    /// The document is in the case and the extractor could not read it — wrong
    /// media type, malformed markup, or past a parser limit. The artifact and its
    /// bytes are untouched, and that distinction is the whole content of this
    /// code: nothing was lost, one derivation did not happen.
    ExtractionFailed,

    /// The bytes this operation needed are not available — shredded, or with no
    /// blob row at all. Distinct from `NotFound`, which is about a row id the
    /// interface is holding; here the row is fine and the evidence behind it is
    /// not.
    EvidenceUnavailable,

    /// A bug in this layer. If a user ever sees this, the code above is
    /// missing a case.
    Internal,
}

/// A failure, in the two parts an interface needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub code: ErrorCode,
    pub message: String,
}

impl CommandError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn no_case_open() -> Self {
        Self::new(
            ErrorCode::NoCaseOpen,
            "no case is open — create or open one first",
        )
    }
}

impl From<kokin_store::StoreError> for CommandError {
    /// Mapped variant by variant, on purpose.
    ///
    /// A blanket `to_string()` would mean that any variant added to
    /// `StoreError` later arrives at the interface as `Storage` with whatever
    /// text it happened to carry. Listing them makes a new variant a decision
    /// someone makes here, in the layer that knows what the user can do about
    /// it.
    fn from(e: kokin_store::StoreError) -> Self {
        Self::from_store_ref(&e)
    }
}

impl CommandError {
    /// The `StoreError` mapping, by reference.
    ///
    /// Exists because a `GraphError::Store` holds one and must produce the same
    /// code it would have produced on its own. A store failure that changes
    /// meaning depending on which crate it passed through is exactly the kind of
    /// thing an interface cannot be expected to reason about.
    fn from_store_ref(e: &kokin_store::StoreError) -> Self {
        use kokin_store::StoreError as S;
        let code = match e {
            S::WrongPassphraseOrNotACase => ErrorCode::WrongPassphrase,
            S::NotACase(_) | S::MalformedHeader(_) | S::UnsupportedHeaderVersion { .. } => {
                ErrorCode::NotACase
            }
            S::Key(kokin_keys::KeyError::WrongKey) => ErrorCode::WrongPassphrase,
            S::Key(kokin_keys::KeyError::MistypedRecoveryKey) => ErrorCode::MistypedRecoveryKey,
            S::Sqlite(_)
            | S::Io(_)
            | S::Key(_)
            | S::NoRecoveryKey
            | S::UnsupportedCipherConfig
            | S::SchemaFromTheFuture { .. } => ErrorCode::Storage,
        };
        Self::new(code, e.to_string())
    }
}

impl From<kokin_search::SearchError> for CommandError {
    fn from(e: kokin_search::SearchError) -> Self {
        use kokin_search::SearchError as S;
        let code = match &e {
            S::IndexCorrupt(_) => ErrorCode::SearchIndexCorrupt,
            S::Sqlite(_) => ErrorCode::Storage,
        };
        Self::new(code, e.to_string())
    }
}

impl From<kokin_graph::GraphError> for CommandError {
    /// Variant by variant, and the refusals are the reason.
    ///
    /// Increment 16 mapped most of these to `Storage` because no command could
    /// reach them; the write commands reach nearly all of them. A refusal
    /// arriving as "the database refused the operation" tells an analyst their
    /// case is broken when what actually happened is that the product declined to
    /// record an unsupported claim — and half of these are not failures at all,
    /// they are the model holding.
    fn from(e: kokin_graph::GraphError) -> Self {
        use kokin_graph::GraphError as G;
        let code = match &e {
            G::SubjectMissing { .. } | G::UnknownEntityType { .. } | G::UnknownScale { .. } => {
                ErrorCode::NotFound
            }
            G::Ungrounded { .. } => ErrorCode::EvidenceRequired,
            G::UnknownScaleValue { .. } => ErrorCode::InvalidValue,
            G::MachineMayNotAssign { .. } | G::MachineMayNotMerge { .. } => {
                ErrorCode::MachineMayNotAct
            }
            G::SelfMerge { .. } | G::AlreadyMerged { .. } => ErrorCode::AlreadyMerged,
            G::NotAbsorbed { .. } => ErrorCode::NotMerged,
            G::Store(inner) => return Self::from_store_ref(inner),
            // A scale row this case cannot parse is a damaged case, not a bad
            // request, and there is nothing the analyst can do about it.
            G::Sqlite(_) | G::Random(_) | G::MalformedScale { .. } => ErrorCode::Storage,
        };
        Self::new(code, e.to_string())
    }
}

impl From<kokin_ingest::IngestError> for CommandError {
    /// The network variants are mapped to `Internal` on purpose.
    ///
    /// No command constructs an `HttpCapability`, and the
    /// workspace holds no live HTTP client at all — `ReplayHttp` replays
    /// fixtures. So `Http`, `NotSuccessful` and `Url` cannot arise from anything
    /// this crate calls, and if one ever does, the bug is here rather than in the
    /// user's request. When `ingest_url` ships they need codes of their own, and
    /// `Internal` is the mapping most likely to be noticed at that point.
    fn from(e: kokin_ingest::IngestError) -> Self {
        use kokin_ingest::IngestError as I;
        let code = match &e {
            I::FileUnreadable { .. } => ErrorCode::FileUnreadable,
            I::BlobOrphaned { .. } => ErrorCode::EvidenceUnavailable,
            I::Store(inner) => return Self::from_store_ref(inner),
            I::Sqlite(_) | I::Blob(_) | I::Random(_) => ErrorCode::Storage,
            I::Http(_) | I::NotSuccessful { .. } | I::Url(_) => ErrorCode::Internal,
        };
        Self::new(code, e.to_string())
    }
}

impl From<kokin_extract::ExtractError> for CommandError {
    /// A document that defeats the extractor has not damaged the case.
    ///
    /// Every variant under `ExtractionFailed` leaves the artifact, its bytes and
    /// its provenance exactly as they were; one derivation did not happen. That
    /// is a materially different thing from a storage failure, and the coverage
    /// figures already report it honestly as an artifact this case cannot search.
    fn from(e: kokin_extract::ExtractError) -> Self {
        use kokin_extract::ExtractError as X;
        let code = match &e {
            X::ArtifactMissing { .. } => ErrorCode::NotFound,
            X::UnsupportedMediaType { .. }
            | X::Malformed { .. }
            | X::TooManyObservations { .. }
            | X::ParserMemory { .. }
            | X::ParserPanicked { .. } => ErrorCode::ExtractionFailed,
            X::BlobShredded { .. } | X::BlobMissing { .. } => ErrorCode::EvidenceUnavailable,
            X::Store(inner) => return Self::from_store_ref(inner),
            X::Sqlite(_) | X::Blob(_) | X::Keys(_) | X::Random(_) => ErrorCode::Storage,
        };
        Self::new(code, e.to_string())
    }
}

impl From<kokin_blob::BlobError> for CommandError {
    /// Only the failures that are not about a *specific* document.
    ///
    /// `NotFound`, `Corrupt` and `HashMismatch` are deliberately absent from any
    /// call site that would use this: they are answers about one document, not
    /// failures of the command, and they travel as
    /// [`crate::views::DocumentContent`] variants instead. Routing them through
    /// here would render "this document was destroyed six months ago" in the
    /// same red box as "the disk is failing".
    fn from(e: kokin_blob::BlobError) -> Self {
        Self::new(ErrorCode::Storage, e.to_string())
    }
}

impl From<kokin_keys::KeyError> for CommandError {
    fn from(e: kokin_keys::KeyError) -> Self {
        let code = match &e {
            kokin_keys::KeyError::MistypedRecoveryKey | kokin_keys::KeyError::BadLength { .. } => {
                ErrorCode::MistypedRecoveryKey
            }
            kokin_keys::KeyError::WrongKey => ErrorCode::WrongPassphrase,
            _ => ErrorCode::Storage,
        };
        Self::new(code, e.to_string())
    }
}
