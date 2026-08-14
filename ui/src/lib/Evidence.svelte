<script lang="ts">
  import DocumentPanel from "./DocumentPanel.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Observation from "./Observation.svelte";
  import {
    artifactDocument,
    artifactObservations,
    artifacts,
    asCommandError,
    observation as loadObservation,
    type ArtifactRowView,
    type CommandError,
    type DocumentView,
    type EvidenceListView,
    type ExtractionState,
    type ObservationView,
  } from "./bindings";

  let rows = $state<ArtifactRowView[] | null>(null);
  let selected = $state<string | null>(null);
  let table = $state<EvidenceListView | null>(null);
  let showWithdrawn = $state(false);
  // `doc`, not `document`: the global of that name is what a component reaches
  // for by reflex, and shadowing it here would make that reflex silently wrong.
  let doc = $state<DocumentView | null>(null);
  let detail = $state<ObservationView | null>(null);
  let error = $state<CommandError | null>(null);
  let busy = $state(false);

  /**
   * How each extraction state reads.
   *
   * `never_attempted` is the dangerous one and it is worded as a gap rather
   * than as a status, because a document nothing has read is invisible to
   * search and looks identical to one that holds nothing (D-029).
   */
  const EXTRACTION: Record<ExtractionState, string> = {
    never_attempted: "never read",
    in_progress: "being read",
    failed: "could not be read",
    partial: "partly read",
    complete: "read",
  };

  $effect(() => {
    void refresh();
  });

  async function refresh() {
    busy = true;
    try {
      rows = await artifacts();
      error = null;
    } catch (err: unknown) {
      error = asCommandError(err);
    } finally {
      busy = false;
    }
  }

  async function select(artifactId: string) {
    busy = true;
    detail = null;
    doc = null;
    selected = artifactId;
    try {
      table = await artifactObservations(artifactId, showWithdrawn);
      error = null;
    } catch (err: unknown) {
      error = asCommandError(err);
      table = null;
    } finally {
      busy = false;
    }
  }

  async function toggleWithdrawn() {
    showWithdrawn = !showWithdrawn;
    if (selected) await select(selected);
  }

  async function openDocument() {
    if (!selected) return;
    busy = true;
    try {
      doc = await artifactDocument(selected);
      error = null;
    } catch (err: unknown) {
      error = asCommandError(err);
    } finally {
      busy = false;
    }
  }

  async function openObservation(observationId: string) {
    busy = true;
    try {
      detail = await loadObservation(observationId);
      error = null;
    } catch (err: unknown) {
      error = asCommandError(err);
      detail = null;
    } finally {
      busy = false;
    }
  }
</script>

<section aria-labelledby="evidence-title">
  <h2 id="evidence-title">Evidence</h2>

  {#if error}
    <ErrorNotice {error} />
  {/if}

  {#if rows === null}
    <p role="status" aria-live="polite">Reading the case…</p>
  {:else if rows.length === 0}
    <p class="empty">
      This case holds no documents yet. Bringing one in is a write, and the write
      flows are increment 26 — nothing here can collect anything.
    </p>
  {:else}
    <table class="artifacts">
      <caption class="visually-hidden">Documents in this case</caption>
      <thead>
        <tr>
          <th scope="col">Collected from</th>
          <th scope="col">Type</th>
          <th scope="col">Observations</th>
          <th scope="col">Reading</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as row (row.artifact_id)}
          <tr class:selected={row.artifact_id === selected}>
            <td>
              <button type="button" class="link" onclick={() => select(row.artifact_id)}>
                {row.canonical_locator ?? "no recorded source"}
              </button>
            </td>
            <td class="quiet">{row.media_type}</td>
            <td class="quiet">{row.observations}</td>
            <!-- The word, not a colour or a bar. "never read" has to survive
                 being skimmed, because it is the state that makes a search of
                 this case incomplete. -->
            <td class:gap={row.extraction !== "complete"}>{EXTRACTION[row.extraction]}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  {#if table}
    <section class="detail" aria-labelledby="table-title">
      <h3 id="table-title">What this document says</h3>

      {#if table.gaps.length > 0}
        <!-- Travels with the table for the reason coverage travels with a
             result list: a full table from a partly-read document is more
             misleading than an empty one, because nobody interrogates it. -->
        <div class="caveat" role="status">
          <p class="title">This reading was incomplete</p>
          <ul>
            {#each table.gaps as gap (gap.gap_kind)}
              <li>
                {gap.detail}
                {#if gap.magnitude !== null && gap.unit}
                  — {gap.magnitude}
                  {gap.unit}
                {:else}
                  — the run did not record how much it missed, which is not the
                  same as none
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      {#if table.superseded > 0}
        <p class="withdrawn-count">
          {table.superseded}
          {table.superseded === 1 ? "observation has" : "observations have"} been withdrawn
          by a later reading of this document.
          <button type="button" class="link" onclick={toggleWithdrawn}>
            {showWithdrawn ? "Hide them" : "Show them"}
          </button>
        </p>
      {/if}

      {#if table.observations.length === 0}
        <p class="empty">
          {table.artifact.extraction === "never_attempted"
            ? "Nothing has read this document, so the case knows nothing about what is in it."
            : "The run that read this document recorded nothing."}
        </p>
      {:else}
        <table class="observations">
          <caption class="visually-hidden">Observations from this document</caption>
          <thead>
            <tr>
              <th scope="col">Kind</th>
              <th scope="col">Value</th>
              <th scope="col">Recorded</th>
            </tr>
          </thead>
          <tbody>
            {#each table.observations as row (row.observation_id)}
              <tr class:withdrawn={row.superseded}>
                <td class="kind">{row.kind}</td>
                <td>
                  <button
                    type="button"
                    class="link"
                    onclick={() => openObservation(row.observation_id)}
                  >
                    {row.value}
                  </button>
                  {#if row.superseded}<span class="tag">withdrawn</span>{/if}
                </td>
                <td class="quiet">{row.observed_utc}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      {#if detail}
        <Observation observation={detail} />
      {/if}

      {#if doc}
        <DocumentPanel {doc} />
      {:else}
        <p>
          <button type="button" onclick={openDocument} disabled={busy}>
            Show the document itself
          </button>
        </p>
      {/if}
    </section>
  {/if}
</section>

<style>
  h2 {
    font-size: 1.05rem;
    margin: 0 0 0.75rem;
  }

  h3 {
    font-size: 0.95rem;
    margin: 0 0 0.6rem;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.875rem;
  }

  th {
    text-align: left;
    font-weight: 600;
    color: #8b95a1;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 0.3rem 0.5rem;
    border-bottom: 1px solid #262b32;
  }

  td {
    padding: 0.4rem 0.5rem;
    border-bottom: 1px solid #1c2027;
    vertical-align: top;
    overflow-wrap: anywhere;
  }

  tr.selected td {
    background: #1b212a;
  }

  tr.withdrawn td {
    color: #7c848f;
  }

  .quiet {
    color: #8b95a1;
  }

  .gap {
    color: #d5c48f;
  }

  .kind {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    color: #9fb3cc;
  }

  .tag {
    display: inline-block;
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 0.05rem 0.35rem;
    border-radius: 3px;
    background: #4a3f24;
    color: #f0dcb0;
    margin-left: 0.4rem;
    vertical-align: 0.1em;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    color: #8fb8e8;
    text-align: left;
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }

  .link:hover {
    background: none;
    color: #b7d3f5;
  }

  .detail {
    margin-top: 1.75rem;
    padding-top: 1.25rem;
    border-top: 1px solid #262b32;
  }

  .caveat {
    border: 1px solid #6b5a3a;
    border-left-width: 3px;
    border-radius: 4px;
    background: #221d14;
    padding: 0.6rem 0.9rem;
    margin-bottom: 0.75rem;
    font-size: 0.875rem;
  }

  .caveat .title {
    margin: 0 0 0.3rem;
    font-weight: 600;
  }

  .caveat ul {
    margin: 0;
    padding-left: 1.1rem;
  }

  .withdrawn-count {
    font-size: 0.875rem;
    color: #c3ccd6;
  }

  .empty {
    color: #8b95a1;
    font-size: 0.875rem;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
