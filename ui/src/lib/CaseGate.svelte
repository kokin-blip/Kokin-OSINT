<script lang="ts">
  /**
   * What you see when no case is open: create one, or open one.
   *
   * Creation accepts an optional typed path; an empty path is resolved by Rust
   * to a unique folder in OS application data. Opening still needs a path.
   */
  import type { Session } from "./session.svelte";

  interface Props {
    session: Session;
  }

  let { session }: Props = $props();

  type Mode = "open" | "create";
  let mode = $state<Mode>("open");

  let path = $state("");
  let passphrase = $state("");
  let caseId = $state("");
  let recoveryKey = $state("");
  let useRecoveryKey = $state(false);

  let canSubmit = $derived(
    !session.busy &&
      (mode === "create" || path.trim().length > 0) &&
      (mode === "create"
        ? caseId.trim().length > 0 && passphrase.length > 0
        : useRecoveryKey
          ? recoveryKey.trim().length > 0
          : passphrase.length > 0),
  );

  async function submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (!canSubmit) return;

    if (mode === "create") {
      await session.create(path.trim(), caseId.trim(), passphrase);
    } else if (useRecoveryKey) {
      await session.openWithRecoveryKey(path.trim(), recoveryKey.trim());
    } else {
      await session.open(path.trim(), passphrase);
    }

    // Cleared whatever happened. A passphrase sitting in a component field
    // after the form has been submitted is a passphrase in a heap snapshot, and
    // the failure case is exactly when it lingers longest.
    passphrase = "";
    recoveryKey = "";
  }
</script>

<section aria-labelledby="gate-title">
  <h2 id="gate-title">{mode === "create" ? "New case" : "Open a case"}</h2>

  <div class="modes" role="group" aria-label="Case action">
    <button
      type="button"
      aria-pressed={mode === "open"}
      onclick={() => (mode = "open")}>Open existing</button
    >
    <button
      type="button"
      aria-pressed={mode === "create"}
      onclick={() => (mode = "create")}>Create new</button
    >
  </div>

  <form onsubmit={submit}>
    <label>
      <span>Case folder</span>
      <input
        type="text"
        bind:value={path}
        placeholder={mode === "create"
          ? "Leave blank to create in the app’s local data folder"
          : "path to an existing .kokincase folder"}
        autocomplete="off"
        spellcheck="false"
        required={mode === "open"}
      />
    </label>

    <p class="hint">{mode === "create" ? "Optional. Leave blank to create a unique case folder in the app’s local data folder. The chosen path is shown after creation." : "Enter the folder of an existing case."}</p>

    {#if mode === "create"}
      <label>
        <span>Case name</span>
        <input type="text" bind:value={caseId} autocomplete="off" required />
      </label>
      <label>
        <span>Passphrase</span>
        <input type="password" bind:value={passphrase} autocomplete="new-password" required />
      </label>
      <p class="hint">
        The passphrase encrypts the case. A recovery key is generated with it and
        shown once — you will be asked to record it before the case opens.
      </p>
    {:else if useRecoveryKey}
      <label>
        <span>Recovery key</span>
        <input
          type="text"
          bind:value={recoveryKey}
          autocomplete="off"
          spellcheck="false"
          class="mono"
          required
        />
      </label>
      <button type="button" class="link" onclick={() => (useRecoveryKey = false)}>
        Use the passphrase instead
      </button>
    {:else}
      <label>
        <span>Passphrase</span>
        <input type="password" bind:value={passphrase} autocomplete="current-password" required />
      </label>
      <button type="button" class="link" onclick={() => (useRecoveryKey = true)}>
        Unlock with the recovery key instead
      </button>
    {/if}

    <button type="submit" class="primary" disabled={!canSubmit}>
      {#if session.busy}
        Working…
      {:else if mode === "create"}
        Create case
      {:else}
        Open case
      {/if}
    </button>
    {#if session.busy}
      <!-- Key derivation is Argon2id at 64 MiB and takes visible time. Silence
           here reads as a hang, and a hang reads as a broken passphrase. -->
      <p class="hint" role="status" aria-live="polite">
        Deriving the key. This is deliberately slow.
      </p>
    {/if}
  </form>
</section>

<style>
  section { max-width: 640px; padding: 28px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface-panel); }

  h2 {
    font-size: 24px;
    text-transform: none;
    letter-spacing: normal;
    color: var(--foreground);
    margin: 0 0 0.75rem;
  }

  .modes {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1.25rem;
  }

  .modes button[aria-pressed="true"] {
    background: var(--accent-surface);
    border-color: var(--border-strong);
    color: var(--foreground);
  }

  form {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.75rem;
    max-width: 30rem;
  }

  label {
    display: block;
    width: 100%;
  }

  label span {
    display: block;
    margin-bottom: 0.25rem;
    font-size: 0.875rem;
    color: var(--foreground-secondary);
  }

  input {
    width: 100%;
  }

  .mono {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    text-transform: uppercase;
  }

  .hint {
    margin: 0;
    color: var(--foreground-secondary);
    font-size: 0.875rem;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
    font-size: 0.875rem;
  }
</style>
