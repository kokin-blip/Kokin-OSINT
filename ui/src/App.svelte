<script lang="ts">
  import CaseGate from "./lib/CaseGate.svelte";
  import ErrorNotice from "./lib/ErrorNotice.svelte";
  import Evidence from "./lib/Evidence.svelte";
  import RecoveryKey from "./lib/RecoveryKey.svelte";
  import { storageEncryptionStatus } from "./lib/bindings";
  import { Session } from "./lib/session.svelte";

  const session = new Session();

  // Increment 1's check, kept. A binary that silently linked plain SQLite
  // instead of SQLCipher would look identical in every other respect, and the
  // one place that is worth saying is the window where cases get created.
  let encryption = $state<"checking" | "ok" | "failed">("checking");
  let backend = $state("");

  $effect(() => {
    storageEncryptionStatus()
      .then((name) => {
        encryption = "ok";
        backend = name;
      })
      .catch((err: unknown) => {
        encryption = "failed";
        backend = String(err);
      });
  });

  $effect(() => {
    void session.refresh();
  });
</script>

<main>
  <header>
    <h1>Kokin-OSINT</h1>
    {#if session.openCase}
      <div class="case">
        <span class="name">{session.openCase.case_id}</span>
        <span class="path" title={session.openCase.root}>{session.openCase.root}</span>
        <button type="button" onclick={() => session.close()} disabled={session.busy}>
          Close case
        </button>
      </div>
    {/if}
  </header>

  {#if encryption === "failed"}
    <!-- Not routed through ErrorNotice: that renders a CommandError, and this is
         a startup check rather than a failed action. It is also the one message
         here that should stop someone creating a case at all. -->
    <div class="notice fault" role="alert">
      <p class="title"><span class="label">Problem</span> Storage is not encrypted</p>
      <p class="detail">{backend}</p>
    </div>
  {/if}

  {#if session.error}
    <ErrorNotice error={session.error} onclose={() => session.close()} />
  {/if}

  {#if session.phase.kind === "starting"}
    <p role="status" aria-live="polite">Starting…</p>
  {:else if session.phase.kind === "closed"}
    <CaseGate {session} />
  {:else if session.phase.kind === "recovery-key"}
    <RecoveryKey
      caseId={session.phase.case.case_id}
      recoveryKey={session.phase.recoveryKey}
      onacknowledge={() => session.acknowledgeRecoveryKey()}
    />
  {:else}
    <section aria-labelledby="case-title">
      <h2 id="case-title">Case open</h2>
      <dl>
        <dt>Schema version</dt>
        <dd>{session.phase.case.schema_version}</dd>
        <dt>Recovery key</dt>
        <dd>
          {#if session.phase.case.has_recovery_key}
            Available — a forgotten passphrase can still be recovered from.
          {:else}
            <strong>None.</strong> If the passphrase is forgotten, this case cannot
            be opened again by anyone.
          {/if}
        </dd>
      </dl>

      <p class="pending">
        Entities, search and the write flows are not wired to this shell yet.
        The commands exist; the panels are increments 24 to 26.
      </p>
    </section>

    <Evidence />
  {/if}

  <footer>
    <p>
      Version 0.0.1 —
      {#if encryption === "checking"}
        checking storage encryption…
      {:else if encryption === "ok"}
        cases are written with {backend}.
      {:else}
        storage encryption could not be confirmed.
      {/if}
    </p>
  </footer>
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    line-height: 1.5;
    background: #14161a;
    color: #e6e8eb;
  }

  :global(button) {
    font: inherit;
    background: #232a33;
    color: #e6e8eb;
    border: 1px solid #39424f;
    border-radius: 4px;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
  }

  :global(button:hover:not(:disabled)) {
    background: #2b333f;
  }

  :global(button:disabled) {
    opacity: 0.5;
    cursor: not-allowed;
  }

  :global(button.primary) {
    background: #2f4a6d;
    border-color: #3f6394;
  }

  :global(button.primary:hover:not(:disabled)) {
    background: #375880;
  }

  :global(input) {
    font: inherit;
    background: #0f1216;
    color: #e6e8eb;
    border: 1px solid #39424f;
    border-radius: 4px;
    padding: 0.4rem 0.5rem;
  }

  /* Focus is never removed and never relies on colour alone — an outline with
     an offset is visible against every background this app uses. */
  :global(:focus-visible) {
    outline: 2px solid #8fb8e8;
    outline-offset: 2px;
  }

  main {
    max-width: 52rem;
    margin: 0 auto;
    padding: 2.5rem 1.5rem;
  }

  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 2rem;
  }

  h1 {
    margin: 0;
    font-size: 1.5rem;
    letter-spacing: -0.01em;
  }

  .case {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .name {
    font-weight: 600;
  }

  .path {
    color: #8b95a1;
    font-size: 0.8rem;
    max-width: 22rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  h2 {
    font-size: 1.05rem;
    margin: 0 0 0.75rem;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.4rem 1rem;
    margin: 0 0 1.5rem;
  }

  dt {
    color: #8b95a1;
  }

  dd {
    margin: 0;
  }

  .pending {
    color: #8b95a1;
    font-size: 0.875rem;
  }

  .notice {
    border: 1px solid #8a4a4a;
    border-left-width: 3px;
    border-radius: 4px;
    background: #241b1b;
    padding: 0.75rem 1rem;
    margin: 1rem 0;
  }

  .notice .title {
    margin: 0;
    font-weight: 600;
  }

  .notice .label {
    display: inline-block;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    background: #4a2b2b;
    color: #ffc9c9;
    margin-right: 0.5rem;
    vertical-align: 0.1em;
  }

  .notice .detail {
    margin: 0.4rem 0 0;
    color: #8b95a1;
    font-size: 0.875rem;
  }

  footer {
    margin-top: 3rem;
    padding-top: 1.5rem;
    border-top: 1px solid #262b32;
    color: #6b7480;
    font-size: 0.875rem;
  }

  @media (prefers-reduced-motion: reduce) {
    :global(*) {
      animation-duration: 0.01ms !important;
      transition-duration: 0.01ms !important;
    }
  }
</style>
