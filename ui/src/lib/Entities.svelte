<script lang="ts">
  import Entity from "./Entity.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Observation from "./Observation.svelte";
  import {
    asCommandError,
    entities,
    entity as loadEntity,
    observation as loadObservation,
    type CommandError,
    type EntityRowView,
    type EntityView,
    type ObservationView,
  } from "./bindings";

  let rows = $state<EntityRowView[] | null>(null);
  let selected = $state<EntityView | null>(null);
  let cited = $state<ObservationView | null>(null);
  let error = $state<CommandError | null>(null);
  let busy = $state(false);

  $effect(() => {
    void refresh();
  });

  async function refresh() {
    busy = true;
    try {
      rows = await entities();
      error = null;
    } catch (err: unknown) {
      error = asCommandError(err);
    } finally {
      busy = false;
    }
  }

  async function open(entityId: string) {
    busy = true;
    cited = null;
    try {
      selected = await loadEntity(entityId);
      error = null;
    } catch (err: unknown) {
      error = asCommandError(err);
      selected = null;
    } finally {
      busy = false;
    }
  }

  /**
   * Open the observation an entity's evidence cites.
   *
   * This is the link the whole three-layer model exists for: a claim about a
   * person, opened down to the bytes of the document that says so, with the
   * citation resolved against those bytes rather than asserted (increment 23).
   */
  async function openEvidence(kind: string, id: string) {
    if (kind !== "observation") return;
    busy = true;
    try {
      cited = await loadObservation(id);
      error = null;
    } catch (err: unknown) {
      error = asCommandError(err);
      cited = null;
    } finally {
      busy = false;
    }
  }
</script>

<section aria-labelledby="entities-title">
  <h2 id="entities-title">Entities</h2>

  {#if error}
    <ErrorNotice {error} />
  {/if}

  {#if rows === null}
    <p role="status" aria-live="polite">Reading the case…</p>
  {:else if rows.length === 0}
    <p class="empty">
      No entities yet. Creating one is a write, and the write flows are increment
      26.
    </p>
  {:else}
    <table>
      <caption class="visually-hidden">Entities in this case</caption>
      <thead>
        <tr>
          <th scope="col">Name</th>
          <th scope="col">Type</th>
          <th scope="col">Identifiers</th>
          <th scope="col">Evidence</th>
          <th scope="col">Assessed</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as row (row.entity_id)}
          <tr class:selected={row.entity_id === selected?.entity_id}>
            <td>
              <button
                type="button"
                class="link"
                disabled={busy}
                onclick={() => open(row.entity_id)}
              >
                {row.display_name}
              </button>
              {#if row.merged_count > 0}
                <span class="tag">
                  {row.merged_count === 1
                    ? "1 record merged in"
                    : `${row.merged_count} records merged in`}
                </span>
              {/if}
            </td>
            <td class="quiet">{row.type_key}</td>
            <td class="quiet">{row.identifier_count}</td>
            <td class="quiet">{row.evidence_count}</td>
            <!-- How many dimensions somebody has judged. Deliberately worded as
                 "dimensions judged" rather than as a fraction or a bar: it says
                 whether anyone looked, never what they concluded, and a
                 progress-shaped rendering would read as a confidence (A-033). -->
            <td class="quiet">
              {row.assessed_dimensions === 0
                ? "none judged"
                : `${row.assessed_dimensions} dimensions judged`}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  {#if selected}
    <Entity entity={selected} onevidence={openEvidence} />
  {/if}

  {#if cited}
    <div class="cited">
      <h3>The evidence behind that</h3>
      <Observation observation={cited} />
    </div>
  {/if}
</section>

<style>
  h2 {
    font-size: 1.05rem;
    margin: 0 0 0.75rem;
  }

  h3 {
    font-size: 0.95rem;
    margin: 0;
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

  .quiet {
    color: #8b95a1;
  }

  .tag {
    display: inline-block;
    font-size: 0.65rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding: 0.05rem 0.35rem;
    border-radius: 3px;
    background: #2b323c;
    color: #cbd3dd;
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

  .cited {
    margin-top: 1.5rem;
    padding-top: 1rem;
    border-top: 1px solid #262b32;
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
