<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // Increment 1 shows exactly one thing, and it is deliberately the scariest
  // one: whether this build genuinely encrypts case files. A binary that
  // silently linked plain SQLite would look identical in every other respect.
  let status = $state<"checking" | "ok" | "failed">("checking");
  let detail = $state("");

  $effect(() => {
    invoke<string>("storage_encryption_status")
      .then((backend) => {
        status = "ok";
        detail = backend;
      })
      .catch((err: unknown) => {
        status = "failed";
        detail = String(err);
      });
  });
</script>

<main>
  <h1>Kokin-OSINT</h1>
  <p class="tagline">Local-first investigation workspace</p>

  <section aria-labelledby="storage-heading">
    <h2 id="storage-heading">Storage</h2>
    <p role="status" aria-live="polite">
      {#if status === "checking"}
        Checking storage encryption…
      {:else if status === "ok"}
        <span class="ok">Encrypted</span> — case files are written with {detail}.
      {:else}
        <span class="fail">Not encrypted</span> — {detail}
      {/if}
    </p>
  </section>

  <footer>
    <p>
      Version 0.0.1 — skeleton. No collection, storage, or analysis features are
      implemented yet.
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

  main {
    max-width: 46rem;
    margin: 0 auto;
    padding: 3rem 1.5rem;
  }

  h1 {
    margin: 0;
    font-size: 1.75rem;
    letter-spacing: -0.01em;
  }

  .tagline {
    margin: 0.25rem 0 2.5rem;
    color: #9aa3ad;
  }

  h2 {
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: #9aa3ad;
    margin-bottom: 0.5rem;
  }

  /* Status is never conveyed by colour alone — the word carries the meaning.
     Colour is reinforcement only. (Accessibility requirement, see the spec.) */
  .ok {
    color: #6ee7a8;
    font-weight: 600;
  }

  .fail {
    color: #ff8f8f;
    font-weight: 600;
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
