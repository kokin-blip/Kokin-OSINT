<script lang="ts">
  import type { DocumentView } from "./bindings";

  interface Props {
    /**
     * Named `doc` rather than `document`, because a prop called `document`
     * shadows the global one inside this component and the next person to reach
     * for `document.querySelector` here would silently get a view type.
     */
    doc: DocumentView;
  }

  let { doc }: Props = $props();

  /**
   * A document is rendered as **text**, never as markup (A-031, D-037).
   *
   * This is not only a safety rule. A browser's rendering of a page can differ
   * arbitrarily from the bytes the case hashed — `display: none`, generated
   * content, script that rewrites the DOM — so the picture is not the evidence.
   * The bytes are the evidence, and they are what the content hash covers.
   *
   * Svelte interpolates text nodes, so `{content.text}` cannot produce an
   * element. `no_part_of_the_interface_can_render_a_document_as_markup` is what
   * keeps that true after somebody decides a preview would be nicer.
   */
  let content = $derived(doc.content);

  function humanBytes(n: number): string {
    if (n < 1024) return `${n} bytes`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MiB`;
  }
</script>

<section class="document" aria-labelledby="document-title">
  <h3 id="document-title">The document</h3>

  <dl class="meta">
    <dt>Type</dt>
    <dd>{doc.media_type}</dd>
    <dt>Size</dt>
    <dd>{humanBytes(doc.byte_length)}</dd>
    <dt>Content hash</dt>
    <dd class="hash">{doc.content_hash}</dd>
  </dl>

  {#if content.state === "readable"}
    {#if content.truncated_bytes > 0}
      <p class="caveat" role="status">
        Showing the first part of this document. {humanBytes(content.truncated_bytes)}
        are not displayed — they were read and verified, and they are not on screen.
      </p>
    {/if}
    {#if content.lossy}
      <p class="caveat" role="status">
        These bytes are not valid text and have been substituted where they could
        not be decoded. What you are reading is a rendering, not the evidence.
      </p>
    {/if}
    <!-- Text, deliberately. See the note above. -->
    <pre class="bytes">{content.text}</pre>
  {:else if content.state === "shredded"}
    <!-- Not an error. Somebody destroyed this on purpose and the case is
         working exactly as designed, so it is presented as a record rather
         than as a failure (A-032). -->
    <div class="state destroyed">
      <p class="title"><span class="label">Destroyed</span> The bytes were shredded</p>
      <p>
        The key that decrypted this document was deliberately destroyed{content.shredded_utc
          ? ` on ${content.shredded_utc}`
          : ""}. It cannot be recovered by anyone, including whoever destroyed it.
      </p>
      <p class="note">
        Everything above survived the shredding — the size, the type and the hash
        are what the case still knows about evidence it no longer holds. Its
        integrity can never be verified again.
      </p>
    </div>
  {:else if content.state === "lost"}
    <div class="state fault">
      <p class="title"><span class="label">Problem</span> The bytes are missing</p>
      <p>
        The case holds the key to this document and not the document. Nobody
        destroyed it: the file is gone from the case directory, which is data
        loss rather than retention.
      </p>
    </div>
  {:else if content.state === "damaged"}
    <div class="state fault">
      <p class="title"><span class="label">Problem</span> The bytes failed verification</p>
      <p>
        This document is present and is not what the case recorded. Something
        modified the case directory. Nothing here should be treated as evidence
        until that is explained.
      </p>
      <p class="detail">{content.detail}</p>
    </div>
  {:else}
    <div class="state fault">
      <p class="title"><span class="label">Problem</span> No provenance for these bytes</p>
      <p>
        The case has an artifact referring to content it has no record of. That
        is a broken case rather than a missing file.
      </p>
    </div>
  {/if}
</section>

<style>
  .document {
    margin-top: 1.5rem;
  }

  h3 {
    font-size: 0.95rem;
    margin: 0 0 0.6rem;
  }

  .meta {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.25rem 1rem;
    margin: 0 0 0.75rem;
    font-size: 0.875rem;
  }

  dt {
    color: #8b95a1;
  }

  dd {
    margin: 0;
  }

  .hash {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8rem;
    overflow-wrap: anywhere;
  }

  .bytes {
    margin: 0;
    padding: 0.75rem;
    background: #0f1216;
    border: 1px solid #262b32;
    border-radius: 4px;
    max-height: 24rem;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.8rem;
  }

  .caveat {
    margin: 0 0 0.5rem;
    color: #d5c48f;
    font-size: 0.875rem;
  }

  .state {
    border: 1px solid;
    border-left-width: 3px;
    border-radius: 4px;
    padding: 0.75rem 1rem;
  }

  .state p {
    margin: 0 0 0.5rem;
  }

  .state p:last-child {
    margin-bottom: 0;
  }

  /* Destroyed on purpose is not a fault, and must not be coloured like one.
     The word carries it; the colour only agrees. */
  .destroyed {
    border-color: #4a5568;
    background: #1b1f26;
  }

  .fault {
    border-color: #8a4a4a;
    background: #241b1b;
  }

  .title {
    font-weight: 600;
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
    margin-right: 0.5rem;
    vertical-align: 0.1em;
  }

  .fault .label {
    background: #4a2b2b;
    color: #ffc9c9;
  }

  .note,
  .detail {
    color: #8b95a1;
    font-size: 0.875rem;
  }

  .detail {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    overflow-wrap: anywhere;
  }
</style>
