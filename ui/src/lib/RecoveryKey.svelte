<script lang="ts">
  /**
   * The one moment this key exists.
   *
   * `create_case` returns it once. It is not stored in usable form anywhere —
   * only its wrap of the case master key is — and there is deliberately no
   * command that returns it again (D-026). So if this screen lets someone walk
   * past it, the case's recovery path is destroyed and nothing says so until a
   * passphrase is forgotten, possibly months later.
   *
   * The controls here follow from that, and each is doing a job:
   *
   * - **No dismiss.** No Escape handler, no click-outside, no close button. The
   *   only exit is the acknowledgement.
   * - **The acknowledgement is a checkbox, not just a button.** A button alone
   *   is one reflexive click from the same outcome as no screen at all.
   * - **Confirm by retyping a segment.** Copying is not recording — a clipboard
   *   is lost on reboot and does not survive to the day this is needed. Asking
   *   for one segment back is the cheapest evidence that the key reached
   *   somewhere durable, and it is deliberately one segment rather than all
   *   eight, because a wall of retyping gets defeated by pasting.
   *
   * None of this can *verify* the key was saved, and it is not pretending to.
   * It raises the cost of skipping to above the cost of complying, which is the
   * most a screen can do.
   */
  interface Props {
    caseId: string;
    recoveryKey: string;
    onacknowledge: () => void;
  }

  let { caseId, recoveryKey, onacknowledge }: Props = $props();

  // Crockford base32 in eight groups of seven (kokin_keys::phrase).
  let segments = $derived(recoveryKey.split("-"));
  // Second-to-last rather than the first: the first is the one visible in a
  // screenshot taken before scrolling, and the last is the one a truncated copy
  // is most likely to have kept.
  let challengeIndex = $derived(Math.max(0, segments.length - 2));
  let expected = $derived(segments[challengeIndex] ?? "");

  let saved = $state(false);
  let typed = $state("");
  let copied = $state(false);

  // Crockford: case-insensitive, and I/L/O/U are excluded from the alphabet
  // precisely because they are read-alikes. Accepting the read-alikes back is
  // the same courtesy the key's own decoder extends.
  function normalise(value: string): string {
    return value
      .trim()
      .toUpperCase()
      .replace(/O/g, "0")
      .replace(/[IL]/g, "1")
      .replace(/U/g, "V")
      .replace(/[^0-9A-Z]/g, "");
  }

  let matches = $derived(
    expected.length > 0 && normalise(typed) === normalise(expected),
  );
  let ready = $derived(saved && matches);

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(recoveryKey);
      copied = true;
    } catch {
      // Clipboard access can be refused, and the key is on screen regardless.
      // Failing loudly here would be alarming about something that does not
      // matter: the durable copy is the written one.
      copied = false;
    }
  }
</script>

<div class="backdrop">
  <div class="panel" role="dialog" aria-modal="true" aria-labelledby="rk-title">
    <h2 id="rk-title">Write this down before continuing</h2>

    <p class="lede">
      This is the recovery key for <strong>{caseId}</strong>. It is the only way
      into this case if the passphrase is forgotten.
    </p>

    <p class="once">
      <strong>It is shown once.</strong> It is not stored anywhere in a form that
      can be read back, and there is no command that will show it again.
    </p>

    <output class="key" aria-label="Recovery key">
      {#each segments as segment, i (i)}<span class="segment">{segment}</span>{/each}
    </output>

    <button type="button" class="copy" onclick={copy}>
      {copied ? "Copied" : "Copy to clipboard"}
    </button>
    {#if copied}
      <p class="hint" role="status" aria-live="polite">
        A clipboard does not survive a reboot. Write it down or put it in a
        password manager.
      </p>
    {/if}

    <hr />

    <label class="check">
      <input type="checkbox" bind:checked={saved} />
      I have recorded this key somewhere I will still have it later.
    </label>

    <label class="challenge">
      <span>
        Type group {challengeIndex + 1} of {segments.length} back to confirm:
      </span>
      <input
        type="text"
        bind:value={typed}
        autocomplete="off"
        spellcheck="false"
        aria-describedby="challenge-state"
      />
    </label>
    <p id="challenge-state" class="hint" role="status" aria-live="polite">
      {#if typed.length === 0}
        Group {challengeIndex + 1} is the second from the end.
      {:else if matches}
        Matches.
      {:else}
        Does not match yet.
      {/if}
    </p>

    <button type="button" class="continue" disabled={!ready} onclick={onacknowledge}>
      Continue to the case
    </button>
    {#if !ready}
      <p class="hint">
        Both steps are required. There is no way past this screen, because there
        is no second chance at this key.
      </p>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(10, 12, 15, 0.92);
    display: grid;
    place-items: center;
    padding: 1.5rem;
    overflow: auto;
  }

  .panel {
    background: #171a20;
    border: 1px solid #2f3742;
    border-radius: 8px;
    padding: 1.5rem;
    max-width: 34rem;
    width: 100%;
  }

  h2 {
    margin: 0 0 0.75rem;
    font-size: 1.15rem;
    text-transform: none;
    letter-spacing: normal;
    color: #e6e8eb;
  }

  .lede,
  .once {
    margin: 0 0 0.75rem;
  }

  .once {
    color: #ffd9a0;
  }

  .key {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    background: #0f1216;
    border: 1px solid #2f3742;
    border-radius: 4px;
    padding: 0.75rem;
    margin-bottom: 0.75rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 1.05rem;
    letter-spacing: 0.06em;
    user-select: all;
  }

  .segment {
    padding: 0.1rem 0.3rem;
  }

  hr {
    border: 0;
    border-top: 1px solid #262b32;
    margin: 1.25rem 0;
  }

  .check,
  .challenge {
    display: block;
    margin-bottom: 0.5rem;
  }

  .challenge span {
    display: block;
    margin-bottom: 0.25rem;
  }

  .challenge input {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    text-transform: uppercase;
    width: 12rem;
  }

  .hint {
    margin: 0.25rem 0 0.75rem;
    color: #8b95a1;
    font-size: 0.875rem;
  }

  .continue {
    margin-top: 0.5rem;
  }
</style>
