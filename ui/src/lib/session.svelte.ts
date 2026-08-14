/**
 * What case is open, and what the interface is allowed to render because of it.
 *
 * The Rust session holds one `Option<OpenCase>` and refuses to swap it (D-027).
 * This mirrors that: one case, or none. It deliberately does **not** cache
 * anything from inside a case — the moment this held its own copy of case data,
 * a stale panel could describe a case the commands no longer act on, which is
 * A-030 in a different costume.
 */

import {
  asCommandError,
  closeCase,
  createCase,
  openCase,
  openCaseWithRecoveryKey,
  sessionStatus,
  type CaseView,
  type CommandError,
  type NewCaseView,
} from "./bindings";

/** What the shell is showing. */
export type Phase =
  | { kind: "starting" }
  | { kind: "closed" }
  | { kind: "open"; case: CaseView }
  /**
   * A case was created and its recovery key has not yet been acknowledged.
   *
   * A distinct phase rather than a flag on `open`, because the difference is
   * what the interface may render: in this phase it may render the key and
   * nothing else. Making it a boolean would make "the case is open" and "the
   * key is still unsaved" independently true, and the whole point is that they
   * are not (D-026).
   */
  | { kind: "recovery-key"; case: CaseView; recoveryKey: string };

export class Session {
  phase = $state<Phase>({ kind: "starting" });
  /** The last failure, for the surface that reports it. Cleared on any attempt. */
  error = $state<CommandError | null>(null);
  busy = $state(false);

  /** The open case, or null. Convenience for the many places that need it. */
  get openCase(): CaseView | null {
    return this.phase.kind === "open" || this.phase.kind === "recovery-key"
      ? this.phase.case
      : null;
  }

  async refresh(): Promise<void> {
    try {
      const view = await sessionStatus();
      // Never downgrades out of `recovery-key`: a refresh must not be a way to
      // get past an unacknowledged key.
      if (this.phase.kind === "recovery-key") return;
      this.phase = view.case ? { kind: "open", case: view.case } : { kind: "closed" };
    } catch (raw) {
      this.error = asCommandError(raw);
      this.phase = { kind: "closed" };
    }
  }

  async create(path: string, caseId: string, passphrase: string): Promise<void> {
    await this.attempt(async () => {
      const created: NewCaseView = await createCase(path, caseId, passphrase);
      this.phase = {
        kind: "recovery-key",
        case: created.case,
        recoveryKey: created.recovery_key,
      };
    });
  }

  async open(path: string, passphrase: string): Promise<void> {
    await this.attempt(async () => {
      this.phase = { kind: "open", case: await openCase(path, passphrase) };
    });
  }

  async openWithRecoveryKey(path: string, recoveryKey: string): Promise<void> {
    await this.attempt(async () => {
      this.phase = {
        kind: "open",
        case: await openCaseWithRecoveryKey(path, recoveryKey),
      };
    });
  }

  async close(): Promise<void> {
    await this.attempt(async () => {
      await closeCase();
      this.phase = { kind: "closed" };
    });
  }

  /**
   * Acknowledge the recovery key, which is the only way out of that phase.
   *
   * Takes no argument and checks nothing: the control that the user actually
   * recorded the key lives in the component, because it is a question about
   * what a person did, and nothing here can verify it. What this guarantees is
   * narrower and is the part that can be guaranteed — **the key is dropped from
   * memory at this point and no code path can produce it again.**
   */
  acknowledgeRecoveryKey(): void {
    if (this.phase.kind !== "recovery-key") return;
    this.phase = { kind: "open", case: this.phase.case };
  }

  private async attempt(action: () => Promise<void>): Promise<void> {
    this.busy = true;
    this.error = null;
    try {
      await action();
    } catch (raw) {
      this.error = asCommandError(raw);
    } finally {
      this.busy = false;
    }
  }
}
