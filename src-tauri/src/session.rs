//! The one open case, and everything the interface is allowed to know about it.
//!
//! # Why this is not in a domain crate
//!
//! The view types here derive `Serialize`, and the domain crates deliberately
//! do not depend on `serde`. If `OpenCase` or `CaseHeader` were serialisable,
//! the wire format would become the domain model by accident, and the first
//! field added for the UI's convenience would be a field the storage layer now
//! carries forever. Defining the views here keeps the two free to differ, and
//! makes "what crosses the boundary" a list somebody can read in one screen.
//!
//! More sharply: `OpenCase` holds the case master key. It must never gain
//! `Serialize`, and the surest way to guarantee that is for the crate defining
//! it not to know what `Serialize` is.
//!
//! # One case at a time
//!
//! The session holds `Option<OpenCase>` behind a mutex. Opening a second case
//! while one is open is refused rather than performed (D-027): silently
//! swapping would leave every open view in the interface showing a case that is
//! no longer the one commands act on, and an analyst acting on the wrong case
//! is the failure this product exists to prevent.

use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;

use crate::error::{CommandError, ErrorCode};

/// The application's one mutable piece of state.
#[derive(Default)]
pub struct Session {
    case: Mutex<Option<kokin_store::OpenCase>>,
}

/// What the interface may know about an open case.
///
/// Everything here is already visible to whoever unlocked the case. There is no
/// key material, no derived key material, and no handle that could be used to
/// reach any: commands find the case through the session, never through a value
/// the interface hands back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseView {
    pub case_id: String,
    /// Displayed in the title bar, so the analyst can see which case this is.
    pub root: String,
    pub schema_version: i64,
    /// Whether the case can still be opened with a recovery key. False means a
    /// forgotten passphrase is permanent, which the interface should say out
    /// loud rather than leave to be discovered.
    pub has_recovery_key: bool,
}

/// A newly created case, and the one thing that will never be shown again.
///
/// `recovery_key` is the single value in this entire layer that carries key
/// material across the IPC boundary, and it does so exactly once (D-026). It is
/// not stored in usable form anywhere — only its wrap of the case master key is
/// — so if the interface does not put it in front of the user now, it is gone
/// and a forgotten passphrase becomes permanent.
///
/// There is deliberately no command that returns it later. A "show me my
/// recovery key again" command would have to either keep it in memory for the
/// session or re-derive it, and neither is a thing this design permits.
#[derive(Debug, Clone, Serialize)]
pub struct NewCaseView {
    pub case: CaseView,
    pub recovery_key: String,
}

/// Whether anything is open, for an interface deciding what to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionView {
    pub case: Option<CaseView>,
}

impl Session {
    /// Run `f` against the open case, or fail with `NoCaseOpen`.
    ///
    /// Every read and write goes through here, so there is one place that
    /// decides what happens when no case is open and one place that takes the
    /// lock.
    pub fn with_case<T>(
        &self,
        f: impl FnOnce(&mut kokin_store::OpenCase) -> Result<T, CommandError>,
    ) -> Result<T, CommandError> {
        let mut guard = self.lock()?;
        let case = guard.as_mut().ok_or_else(CommandError::no_case_open)?;
        f(case)
    }

    /// Create a case and make it the open one.
    pub fn create(
        &self,
        root: &Path,
        case_id: &str,
        passphrase: &str,
    ) -> Result<NewCaseView, CommandError> {
        let mut guard = self.lock()?;
        if guard.is_some() {
            return Err(already_open());
        }

        // Checked before create_case rather than relying on its error, so that
        // "there is already something here" is never reported as a malformed
        // header — a message that reads like the user's data is damaged.
        if root.exists() {
            return Err(CommandError::new(
                ErrorCode::CaseExists,
                format!("{} already exists", root.display()),
            ));
        }

        let (open, recovery) = kokin_store::create_case(root, case_id, passphrase)?;
        let view = describe(&open, case_id)?;
        // The rendered key lives exactly as long as this function. Zeroizing
        // wraps it up to the point it becomes the response, which is as far as
        // this layer can carry it.
        let recovery_key = kokin_keys::to_phrase(&recovery).to_string();
        *guard = Some(open);

        Ok(NewCaseView {
            case: view,
            recovery_key,
        })
    }

    /// Open an existing case with a passphrase.
    pub fn open(&self, root: &Path, passphrase: &str) -> Result<CaseView, CommandError> {
        let mut guard = self.lock()?;
        if guard.is_some() {
            return Err(already_open());
        }
        let open = kokin_store::open_case(root, passphrase)?;
        let view = describe_from_header(&open)?;
        *guard = Some(open);
        Ok(view)
    }

    /// Open an existing case with a written-down recovery key.
    ///
    /// The phrase is parsed before anything is unlocked, so a mistyped key is
    /// reported as mistyped rather than as a wrong key — the user needs to know
    /// whether to check their typing or their notebook.
    pub fn open_with_recovery_key(
        &self,
        root: &Path,
        phrase: &str,
    ) -> Result<CaseView, CommandError> {
        let recovery = kokin_keys::from_phrase(phrase)?;
        let mut guard = self.lock()?;
        if guard.is_some() {
            return Err(already_open());
        }
        let open = kokin_store::open_case_with_recovery_key(root, &recovery)?;
        let view = describe_from_header(&open)?;
        *guard = Some(open);
        Ok(view)
    }

    /// Close the open case, dropping the connection and zeroizing the key.
    ///
    /// Idempotent: closing nothing is not an error, because the interface may
    /// call this while shutting down and a failure there has nowhere to go.
    pub fn close(&self) -> Result<(), CommandError> {
        let mut guard = self.lock()?;
        // The drop is the point of this function. `OpenCase` owns the only
        // handle to the connection and the only copy of the case master key,
        // and `CaseMasterKey` zeroizes on drop.
        *guard = None;
        Ok(())
    }

    /// What is open, if anything.
    pub fn view(&self) -> Result<SessionView, CommandError> {
        let guard = self.lock()?;
        let case = match guard.as_ref() {
            Some(open) => Some(describe_from_header(open)?),
            None => None,
        };
        Ok(SessionView { case })
    }

    fn lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<kokin_store::OpenCase>>, CommandError> {
        // A poisoned lock means a command panicked while holding an open case.
        // The case is not known to be damaged - every write is a transaction -
        // but this layer cannot tell, so it refuses rather than guessing.
        self.case.lock().map_err(|_| {
            CommandError::new(
                ErrorCode::Internal,
                "the case session was left in an unknown state by a previous failure; \
                 close and reopen the case",
            )
        })
    }
}

fn already_open() -> CommandError {
    CommandError::new(
        ErrorCode::CaseAlreadyOpen,
        "a case is already open — close it first",
    )
}

/// Describe a case whose id is already known, without re-reading the header.
fn describe(open: &kokin_store::OpenCase, case_id: &str) -> Result<CaseView, CommandError> {
    Ok(CaseView {
        case_id: case_id.to_string(),
        root: open.paths.root.display().to_string(),
        schema_version: schema_version(&open.conn)?,
        has_recovery_key: header(open)?.cmk_wrapped_by_recovery.is_some(),
    })
}

fn describe_from_header(open: &kokin_store::OpenCase) -> Result<CaseView, CommandError> {
    let header = header(open)?;
    Ok(CaseView {
        case_id: header.case_id,
        root: open.paths.root.display().to_string(),
        schema_version: schema_version(&open.conn)?,
        has_recovery_key: header.cmk_wrapped_by_recovery.is_some(),
    })
}

/// Re-read the plaintext header rather than caching it on the session.
///
/// It is a small file, this is not a hot path, and a cached copy would be a
/// second source of truth for something a passphrase rotation rewrites.
fn header(open: &kokin_store::OpenCase) -> Result<kokin_store::CaseHeader, CommandError> {
    Ok(kokin_store::read_header(&open.paths)?)
}

fn schema_version(conn: &rusqlite::Connection) -> Result<i64, CommandError> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| CommandError::new(ErrorCode::Storage, e.to_string()))
}
