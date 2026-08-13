//! The case lifecycle, without a window.
//!
//! `Session` knows nothing about Tauri, which is the point: everything below
//! runs under plain `cargo test`, and the only code these tests do not reach is
//! the one line of marshalling inside each `#[tauri::command]`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use kokin_osint_lib::{ErrorCode, Session};

static COUNTER: AtomicU32 = AtomicU32::new(0);

const PASSPHRASE: &str = "correct horse battery staple";

fn case_path(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!("kokin-session-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("case.kokincase")
}

fn cleanup(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

/// The acceptance criterion, through the surface the interface actually calls:
/// created, closed, reopened.
#[test]
fn a_case_can_be_created_closed_and_opened_again() {
    let path = case_path("lifecycle");
    let session = Session::default();

    let created = session.create(&path, "case-one", PASSPHRASE).unwrap();
    assert_eq!(created.case.case_id, "case-one");
    assert!(created.case.schema_version > 0);
    assert!(created.case.has_recovery_key);

    assert_eq!(
        session
            .view()
            .unwrap()
            .case
            .as_ref()
            .map(|c| c.case_id.as_str()),
        Some("case-one")
    );

    session.close().unwrap();
    assert_eq!(session.view().unwrap().case, None);

    let reopened = session.open(&path, PASSPHRASE).unwrap();
    assert_eq!(reopened, created.case);

    cleanup(&path);
}

/// The recovery key is shown once, as a string, and must work when typed back.
///
/// Until this passed, `create_case` handed the recovery key to a caller that
/// had no way to render it, so every case created through the application would
/// have had an unusable recovery path — discovered on the day it was needed.
#[test]
fn the_recovery_key_shown_at_creation_opens_the_case() {
    let path = case_path("recovery");
    let session = Session::default();

    let created = session.create(&path, "case-two", PASSPHRASE).unwrap();
    let written_down = created.recovery_key.clone();
    session.close().unwrap();

    // As a user would retype it: lowercase, spaces instead of dashes.
    let retyped = written_down.to_lowercase().replace('-', " ");
    let opened = session.open_with_recovery_key(&path, &retyped).unwrap();
    assert_eq!(opened, created.case);

    cleanup(&path);
}

/// A typo and a wrong key are different problems with different remedies.
#[test]
fn a_mistyped_recovery_key_is_told_apart_from_a_wrong_one() {
    let path = case_path("typo");
    let session = Session::default();

    let created = session.create(&path, "case-three", PASSPHRASE).unwrap();
    session.close().unwrap();

    // One character changed. Caught by the checksum, before any derivation.
    let phrase = created.recovery_key.clone();
    let idx = phrase.find(|c: char| c != '-' && c != 'Z').unwrap();
    let mut typo = phrase.clone();
    typo.replace_range(idx..idx + 1, "Z");
    assert_eq!(
        session
            .open_with_recovery_key(&path, &typo)
            .unwrap_err()
            .code,
        ErrorCode::MistypedRecoveryKey
    );

    // A different key, correctly written. Nothing is wrong with the typing;
    // it is the wrong key, and that is what the user must be told.
    let other = kokin_keys::to_phrase(&kokin_keys::Secret::generate().unwrap()).to_string();
    assert_eq!(
        session
            .open_with_recovery_key(&path, &other)
            .unwrap_err()
            .code,
        ErrorCode::WrongPassphrase
    );

    // Neither attempt left a case open.
    assert_eq!(session.view().unwrap().case, None);

    cleanup(&path);
}

/// A failed open must not leave the session half-open, and the code the
/// interface branches on must be the one that means "check your typing".
#[test]
fn a_wrong_passphrase_is_a_code_and_leaves_nothing_open() {
    let path = case_path("wrongpass");
    let session = Session::default();

    session.create(&path, "case-four", PASSPHRASE).unwrap();
    session.close().unwrap();

    let err = session.open(&path, "not it").unwrap_err();
    assert_eq!(err.code, ErrorCode::WrongPassphrase);
    assert_eq!(session.view().unwrap().case, None);

    cleanup(&path);
}

/// Refused, not performed.
///
/// A silent swap would leave every view in the interface describing a case that
/// commands no longer act on, which is the one confusion an investigation tool
/// may not create.
#[test]
fn opening_a_second_case_is_refused_and_the_first_stays_open() {
    let first = case_path("first");
    let second = case_path("second");
    let session = Session::default();

    session.create(&first, "case-first", PASSPHRASE).unwrap();

    let err = session
        .create(&second, "case-second", PASSPHRASE)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::CaseAlreadyOpen);

    assert_eq!(
        session.view().unwrap().case.map(|c| c.case_id),
        Some("case-first".to_string()),
        "the refused open changed which case was current"
    );
    // And it created nothing on disk, so retrying after a close is not blocked
    // by the debris of the attempt.
    assert!(!second.exists());

    cleanup(&first);
    cleanup(&second);
}

/// Every command that needs a case must say so with one code, from one place.
#[test]
fn commands_that_need_a_case_report_no_case_open() {
    let session = Session::default();
    let err = session.with_case(|_| Ok(())).unwrap_err();
    assert_eq!(err.code, ErrorCode::NoCaseOpen);
}

/// Never overwrite. A case directory is the evidence.
#[test]
fn creating_a_case_over_something_that_exists_is_refused() {
    let path = case_path("exists");
    let session = Session::default();

    session.create(&path, "case-five", PASSPHRASE).unwrap();
    session.close().unwrap();

    let err = session
        .create(&path, "case-five-again", PASSPHRASE)
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::CaseExists);

    // Still openable, and still the original case.
    assert_eq!(
        session.open(&path, PASSPHRASE).unwrap().case_id,
        "case-five"
    );

    cleanup(&path);
}

/// What the interface receives, field by field.
///
/// Written as an exact set rather than a contains-check so that adding a field
/// to a view is a decision someone makes on purpose, in a test that names the
/// boundary — not a convenience that ships because it compiled.
#[test]
fn the_views_carry_exactly_these_fields_and_no_key_material() {
    let path = case_path("shape");
    let session = Session::default();

    let created = session.create(&path, "case-six", PASSPHRASE).unwrap();

    let case = serde_json::to_value(&created.case).unwrap();
    let mut fields: Vec<&str> = case
        .as_object()
        .unwrap()
        .keys()
        .map(|s| s.as_str())
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        ["case_id", "has_recovery_key", "root", "schema_version"]
    );

    // The recovery key is returned by creation and by nothing else. Any later
    // view carrying it would mean the application is holding it after the one
    // moment it is supposed to exist.
    let recovery = &created.recovery_key;
    let status = serde_json::to_string(&session.view().unwrap()).unwrap();
    assert!(
        !status.contains(recovery.as_str()),
        "the session view carries the recovery key"
    );
    assert!(!serde_json::to_string(&created.case)
        .unwrap()
        .contains(recovery.as_str()));

    cleanup(&path);
}

/// The error is two parts, and the interface branches on the one that is not
/// prose. A serialised code is a stable snake_case string.
#[test]
fn an_error_serialises_as_a_code_the_interface_can_match_on() {
    let session = Session::default();
    let err = session.with_case(|_| Ok(())).unwrap_err();
    let json = serde_json::to_value(&err).unwrap();

    assert_eq!(json["code"], "no_case_open");
    assert!(json["message"].as_str().unwrap().len() > 10);
}
