<script lang="ts">
  /**
   * What you see when no case is open: create one, or open one.
   *
   * Paths are typed rather than picked, because there is no native file dialog
   * yet (increment 25, which is also what closes A-034). That is poor and it is
   * honest about being poor — a placeholder showing the shape of a case path is
   * more use than a browse button that does not exist.
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
      path.trim().length > 0 &&
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
          ? "C:\\Cases\\Ferry Road.kokincase"
          : "path to an existing .kokincase folder"}
        autocomplete="off"
        spellcheck="false"
        required
      />
    </label>

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
  h2 {
    font-size: 1.05rem;
    text-transform: none;
    letter-spacing: normal;
    color: #e6e8eb;
    margin: 0 0 0.75rem;
  }

  .modes {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 1.25rem;
  }

  .modes button[aria-pressed="true"] {
    background: #2b333f;
    border-color: #4a5768;
    color: #e6e8eb;
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
    color: #c3ccd6;
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
    color: #8b95a1;
    font-size: 0.875rem;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: #8fb8e8;
    text-decoration: underline;
    cursor: pointer;
    font-size: 0.875rem;
  }
</style>
