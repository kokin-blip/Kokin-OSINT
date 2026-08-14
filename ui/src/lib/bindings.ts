/**
 * The command boundary, typed.
 *
 * Every call into Rust goes through this file and nothing else calls `invoke`.
 * That is the same rule `src-tauri/src/views.rs` holds on the other side — one
 * file lists what crosses, so a person can read the whole boundary in one
 * sitting — and `the_typescript_bindings_cover_every_view_field` fails if a
 * field is added to a Rust view and not mirrored here.
 *
 * # Errors are codes, not prose
 *
 * `CommandError.code` is a closed set and it is the only thing any caller may
 * branch on. Matching on `message` is what D-028 exists to prevent: a reworded
 * message silently removes a branch, and a wrong-passphrase dialog that stops
 * appearing because somebody improved the wording is a bug nobody catches in
 * review. Messages are for showing a person, never for deciding.
 */

import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/** Mirrors `ErrorCode` in `src-tauri/src/error.rs`, serialised snake_case. */
export type ErrorCode =
  | "no_case_open"
  | "case_already_open"
  | "wrong_passphrase"
  | "mistyped_recovery_key"
  | "not_a_case"
  | "case_exists"
  | "storage"
  | "not_found"
  | "search_index_corrupt"
  | "evidence_required"
  | "explanation_required"
  | "already_merged"
  | "not_merged"
  | "invalid_value"
  | "machine_may_not_act"
  | "file_unreadable"
  | "extraction_failed"
  | "evidence_unavailable"
  | "internal";

export interface CommandError {
  code: ErrorCode;
  message: string;
}

/**
 * Narrow an unknown rejection to a `CommandError`.
 *
 * Tauri rejects with whatever the command's error type serialised to, so in
 * practice this is always a `CommandError` — but "in practice" is not a type,
 * and a panic in the command layer arrives here as a string. Anything that is
 * not recognisably one becomes `internal`, which is documented on the Rust side
 * as a bug rather than a bucket, so it reads correctly either way.
 */
export function asCommandError(error: unknown): CommandError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof (error as CommandError).code === "string"
  ) {
    return error as CommandError;
  }
  return {
    code: "internal",
    message: typeof error === "string" ? error : String(error),
  };
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

export interface CaseView {
  case_id: string;
  root: string;
  schema_version: number;
  /**
   * False means a forgotten passphrase is permanent. The interface must say so
   * out loud rather than leave it to be discovered.
   */
  has_recovery_key: boolean;
}

/**
 * The one payload in this entire boundary that carries key material, and it
 * does so exactly once (D-026).
 *
 * There is no command that returns `recovery_key` again — not by design
 * oversight but because keeping it for the session or re-deriving it are both
 * things this design refuses. If the interface does not put it in front of the
 * user now, it is gone and a forgotten passphrase becomes permanent.
 */
export interface NewCaseView {
  case: CaseView;
  recovery_key: string;
}

export interface SessionView {
  case: CaseView | null;
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

export function storageEncryptionStatus(): Promise<string> {
  return invoke("storage_encryption_status");
}

export function createCase(
  path: string,
  caseId: string,
  passphrase: string,
): Promise<NewCaseView> {
  return invoke("create_case", { path, caseId, passphrase });
}

export function openCase(path: string, passphrase: string): Promise<CaseView> {
  return invoke("open_case", { path, passphrase });
}

export function openCaseWithRecoveryKey(
  path: string,
  recoveryKey: string,
): Promise<CaseView> {
  return invoke("open_case_with_recovery_key", { path, recoveryKey });
}

export function closeCase(): Promise<void> {
  return invoke("close_case");
}

export function sessionStatus(): Promise<SessionView> {
  return invoke("session_status");
}
