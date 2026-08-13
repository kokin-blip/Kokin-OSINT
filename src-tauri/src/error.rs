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
        use kokin_store::StoreError as S;
        let code = match &e {
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
