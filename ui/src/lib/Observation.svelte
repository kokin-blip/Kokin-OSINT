<script lang="ts">
  import type { CollectionView, ObservationView, QuoteView } from "./bindings";

  interface Props {
    observation: ObservationView;
  }

  let { observation }: Props = $props();

  /**
   * How a quote verdict reads.
   *
   * The verdict arrives from Rust, resolved against bytes the AEAD and the
   * content hash both accepted (D-038). Nothing here recomputes it — a panel
   * that compared `value` against `excerpt.quoted` itself would be comparing
   * two strings it was handed in the same payload, which always agrees and
   * proves nothing.
   */
  type Verdict = {
    tone: "supported" | "qualified" | "unsupported" | "unknown";
    label: string;
    explanation: string;
  };

  function verdict(quote: QuoteView): Verdict {
    switch (quote.agreement) {
      case "exact":
        return {
          tone: "supported",
          label: "Quoted exactly",
          explanation: "The document says precisely this, at the position recorded.",
        };
      case "contains":
        return {
          tone: "supported",
          label: "Found in the document",
          explanation:
            "The recorded value appears inside the bytes cited. Normal: a locator commonly spans an element while the value is its text or one attribute.",
        };
      case "differs":
        return {
          tone: "unsupported",
          label: "The document does not say this",
          explanation:
            "The bytes at the recorded position do not contain this value. The citation does not support the claim, and the claim should not be relied on until that is explained.",
        };
      case "out_of_range":
        return {
          tone: "unsupported",
          label: "Cites bytes the document does not have",
          explanation: `The locator names ${quote.start}–${quote.end} in a document of ${quote.byte_length} bytes.`,
        };
      case "too_large":
        return {
          tone: "unknown",
          label: "Too large to quote",
          explanation: `The locator names ${quote.bytes} bytes, past the ${quote.limit}-byte limit this panel will read. Declined rather than truncated: a shortened quote could be confirmed but never contradicted.`,
        };
      case "unknown_scheme":
        return {
          tone: "unknown",
          label: "A locator this build cannot follow",
          explanation: `The observation records its position as "${quote.scheme}", which this build does not know how to resolve. The raw locator is below, unaltered.`,
        };
      case "unavailable":
        return {
          tone: "unknown",
          label: "The bytes are not available to check",
          explanation:
            quote.document_state === "shredded"
              ? "The document was deliberately destroyed. What it said cannot be confirmed again by anyone."
              : `The document could not be read (${quote.document_state}), so this citation cannot be checked.`,
        };
    }
  }

  let shown = $derived(verdict(observation.quote));
  let excerpt = $derived(
    observation.quote.agreement === "exact" ||
      observation.quote.agreement === "contains" ||
      observation.quote.agreement === "differs"
      ? observation.quote.excerpt
      : null,
  );

  function where(collection: CollectionView): string {
    return collection.source.raw_locator;
  }
</script>

<article class="observation" aria-labelledby="observation-title">
  <h3 id="observation-title">
    <span class="kind">{observation.kind}</span>
    {observation.value}
  </h3>

  {#if observation.superseded}
    <!-- The case has changed its mind about this. Shown before anything else,
         because everything below describes a claim the case has withdrawn. -->
    <div class="withdrawn" role="status">
      <p class="title"><span class="label">Withdrawn</span> A later reading replaced this</p>
      <p>
        {#if observation.superseded.superseded_by}
          A rerun of the extractor on {observation.superseded.recorded_utc} recorded
          a different value in this position.
        {:else}
          A rerun of the extractor on {observation.superseded.recorded_utc} read the
          same document and did not find this at all — a stronger statement than
          a corrected value.
        {/if}
      </p>
    </div>
  {/if}

  <section class="quote {shown.tone}" aria-labelledby="quote-title">
    <h4 id="quote-title">
      <span class="label">{shown.label}</span>
    </h4>
    <p class="explanation">{shown.explanation}</p>

    {#if excerpt}
      {#if excerpt.lossy}
        <p class="explanation">
          Some of these bytes are not valid text and were substituted. What is
          shown is a rendering of the evidence, not the evidence.
        </p>
      {/if}
      <!-- Three text nodes, never markup. The split comes from Rust so no
           offset arithmetic happens here: highlighting the wrong span would
           look entirely convincing. -->
      <p class="excerpt">
        <span class="context">{excerpt.before}</span><mark>{excerpt.quoted}</mark><span
          class="context">{excerpt.after}</span>
      </p>
    {/if}
  </section>

  <section aria-labelledby="lineage-title">
    <h4 id="lineage-title">Where this came from</h4>

    <ol class="lineage">
      {#if observation.lineage.collection}
        {@const c = observation.lineage.collection}
        <li>
          <span class="step">Collected</span>
          <div>
            <p class="what">{where(c)}</p>
            <p class="detail">
              {c.capture.requested_utc} · {c.capture.connector} · job {c.capture.job_id}
              {#if c.capture.http_status}· HTTP {c.capture.http_status}{/if}
              · {c.capture.capture_completeness}
            </p>
            {#if c.capture.from_fixture}
              <p class="detail flag">
                Replayed from a recorded fixture, not fetched. The date above is
                when it was replayed.
              </p>
            {/if}
            {#if c.source.canonical_locator !== c.source.raw_locator}
              <p class="detail">Normalised to {c.source.canonical_locator}</p>
            {/if}
            {#if c.ingest}
              <p class="detail">
                {c.ingest.transform_name}
                {c.ingest.transform_version} · build {c.ingest.code_version}
              </p>
            {/if}
          </div>
        </li>
      {:else}
        <li>
          <span class="step">Collected</span>
          <div>
            <p class="what missing">The case has no record of how this arrived.</p>
            <p class="detail">
              Every artifact should have a capture and a source. This one does
              not, which is a broken case rather than an empty panel.
            </p>
          </div>
        </li>
      {/if}

      <li>
        <span class="step">Stored</span>
        <div>
          <p class="what">{observation.lineage.artifact.media_type}</p>
          <p class="detail hash">{observation.lineage.artifact.content_hash}</p>
        </div>
      </li>

      <li>
        <span class="step">Read</span>
        <div>
          <p class="what">
            {observation.lineage.extraction.transform_name}
            {observation.lineage.extraction.transform_version}
          </p>
          <p class="detail">
            build {observation.lineage.extraction.code_version} ·
            {observation.lineage.extraction.started_utc} ·
            {observation.lineage.extraction.status}
          </p>
          {#if observation.lineage.extraction.error_detail}
            <p class="detail">{observation.lineage.extraction.error_detail}</p>
          {/if}
        </div>
      </li>

      <li>
        <span class="step">Recorded</span>
        <div>
          <p class="what">{observation.observed_utc}</p>
          <p class="detail">
            {observation.locator.scheme}{#if observation.locator.selector}
              · {observation.locator.selector}{/if}{#if observation.locator.start !== null}
              · bytes {observation.locator.start}–{observation.locator.end}{/if}
          </p>
          <p class="detail raw">{observation.locator.raw_json}</p>
        </div>
      </li>
    </ol>

    {#if observation.lineage.also_collected.length > 0}
      <!-- An investigative fact, not a storage detail. Naming only one origin
           would tell the analyst this evidence has a single provenance when
           the case knows it does not. -->
      <div class="also">
        <p class="title">
          These exact bytes also reached this case from
          {observation.lineage.also_collected.length === 1
            ? "one other place"
            : `${observation.lineage.also_collected.length} other places`}:
        </p>
        <ul>
          {#each observation.lineage.also_collected as other (other.artifact_id)}
            <li>
              <span class="what">{where(other)}</span>
              <span class="detail">{other.capture.requested_utc} · {other.capture.connector}</span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </section>
</article>

<style>
  .observation {
    border: 1px solid #262b32;
    border-radius: 6px;
    padding: 1rem 1.25rem;
    margin-top: 1rem;
  }

  h3 {
    margin: 0 0 0.75rem;
    font-size: 1rem;
    overflow-wrap: anywhere;
  }

  h4 {
    margin: 1.25rem 0 0.5rem;
    font-size: 0.85rem;
    color: #8b95a1;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .kind {
    display: inline-block;
    font-size: 0.7rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    background: #232a33;
    color: #9fb3cc;
    margin-right: 0.5rem;
    vertical-align: 0.15em;
    text-transform: none;
    letter-spacing: 0;
  }

  .quote {
    border: 1px solid;
    border-left-width: 3px;
    border-radius: 4px;
    padding: 0.6rem 0.9rem;
  }

  .quote h4 {
    margin: 0;
  }

  /* Tone is carried by the label word. A citation that does not hold is the one
     thing in this panel that must survive being read in monochrome. */
  .supported {
    border-color: #3f6b4a;
    background: #171f1a;
  }

  .qualified,
  .unknown {
    border-color: #4a5568;
    background: #1b1f26;
  }

  .unsupported {
    border-color: #8a4a4a;
    background: #241b1b;
  }

  .label {
    display: inline-block;
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    background: #2b323c;
    color: #cbd3dd;
    vertical-align: 0.1em;
  }

  .unsupported .label {
    background: #4a2b2b;
    color: #ffc9c9;
  }

  .supported .label {
    background: #24402c;
    color: #b8dcc0;
  }

  .explanation {
    margin: 0.5rem 0 0;
    font-size: 0.875rem;
    color: #c3ccd6;
  }

  .excerpt {
    margin: 0.6rem 0 0;
    padding: 0.6rem;
    background: #0f1216;
    border-radius: 4px;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 14rem;
    overflow: auto;
  }

  .context {
    color: #6b7480;
  }

  mark {
    background: #3d4f2b;
    color: #e8f0da;
    padding: 0.05rem 0;
  }

  .withdrawn {
    border: 1px solid #6b5a3a;
    border-left-width: 3px;
    border-radius: 4px;
    background: #221d14;
    padding: 0.6rem 0.9rem;
    margin-bottom: 0.75rem;
  }

  .withdrawn p {
    margin: 0.4rem 0 0;
    font-size: 0.875rem;
  }

  .withdrawn .title {
    margin: 0;
    font-weight: 600;
    font-size: 1rem;
  }

  .withdrawn .label {
    background: #4a3f24;
    color: #f0dcb0;
    margin-right: 0.5rem;
  }

  .lineage {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.6rem;
  }

  .lineage > li {
    display: grid;
    grid-template-columns: 7rem 1fr;
    gap: 0.75rem;
    align-items: start;
  }

  .step {
    color: #8b95a1;
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    padding-top: 0.1rem;
  }

  .what {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .missing {
    color: #e0a0a0;
  }

  .detail {
    margin: 0.15rem 0 0;
    color: #8b95a1;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }

  .flag {
    color: #d5c48f;
  }

  .hash,
  .raw {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
  }

  .also {
    margin-top: 1rem;
    border-top: 1px solid #262b32;
    padding-top: 0.75rem;
  }

  .also .title {
    margin: 0 0 0.4rem;
    font-size: 0.875rem;
  }

  .also ul {
    margin: 0;
    padding-left: 1.1rem;
  }

  .also li {
    margin-bottom: 0.3rem;
  }

  .also .detail {
    display: block;
  }
</style>
