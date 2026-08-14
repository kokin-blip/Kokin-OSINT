<script lang="ts">
  import type { CommandError, ErrorCode } from "./bindings";

  interface Props {
    error: CommandError;
    /** Offered when the failure is one a close would resolve. */
    onclose?: () => void;
  }

  let { error, onclose }: Props = $props();

  /**
   * How a failure reads, decided by its code and never by its message (D-028).
   *
   * `tone` is the load-bearing field. Several of these codes are the product
   * working exactly as designed — a machine refused a decision only a person may
   * make, a case declining to be swapped out from under a rendered panel — and
   * rendering those in red teaches an analyst that the safety rails are faults.
   * The words carry the meaning; colour only reinforces it.
   */
  type Tone = "refusal" | "wrong-input" | "fault";

  interface Presentation {
    tone: Tone;
    title: string;
    /** What the person can do. Empty when there is genuinely nothing. */
    remedy: string;
  }

  const PRESENTATION: Record<ErrorCode, Presentation> = {
    case_already_open: {
      tone: "refusal",
      title: "A case is already open",
      remedy:
        "Close it first. Opening a second case would leave every panel on screen describing the first one.",
    },
    no_case_open: {
      tone: "refusal",
      title: "No case is open",
      remedy: "Create or open a case to continue.",
    },
    machine_may_not_act: {
      tone: "refusal",
      title: "That decision needs a person",
      remedy: "Merges and assessments are recorded against whoever made them.",
    },
    evidence_required: {
      tone: "refusal",
      title: "This needs evidence",
      remedy: "Cite at least one observation before recording it.",
    },
    explanation_required: {
      tone: "refusal",
      title: "This needs a reason",
      remedy: "Say what the judgement is based on. It cannot be reconsidered later without it.",
    },
    already_merged: {
      tone: "refusal",
      title: "Already the same entity",
      remedy: "Open the entity they resolve to.",
    },
    not_merged: {
      tone: "refusal",
      title: "Not merged into anything",
      remedy: "",
    },

    wrong_passphrase: {
      tone: "wrong-input",
      title: "That passphrase did not open the case",
      remedy: "Check it and try again, or unlock with the recovery key.",
    },
    mistyped_recovery_key: {
      tone: "wrong-input",
      title: "That recovery key is not valid",
      remedy:
        "The checksum did not match, so it is mistyped rather than wrong. Check for confused characters.",
    },
    not_a_case: {
      tone: "wrong-input",
      title: "Not a case",
      remedy: "That folder has no case header. Choose a .kokincase folder.",
    },
    case_exists: {
      tone: "wrong-input",
      title: "Something is already there",
      remedy: "Choose a different location, or open what is already there.",
    },
    file_unreadable: {
      tone: "wrong-input",
      title: "That file could not be read",
      remedy: "Check the file still exists and that you have permission to read it.",
    },
    invalid_value: {
      tone: "wrong-input",
      title: "Not a value this case accepts",
      remedy: "The options may have changed since this screen loaded. Reload and try again.",
    },
    not_found: {
      tone: "wrong-input",
      title: "No longer in this case",
      remedy: "It may have been removed since this screen loaded.",
    },

    extraction_failed: {
      tone: "fault",
      title: "This document could not be read",
      remedy:
        "The document and its provenance are untouched — only the extraction did not happen.",
    },
    evidence_unavailable: {
      tone: "fault",
      title: "The bytes behind this are not available",
      remedy: "They were destroyed deliberately, or the case has lost them.",
    },
    search_index_corrupt: {
      tone: "fault",
      title: "The search index is damaged",
      remedy: "It can be rebuilt from the case. No evidence is affected.",
    },
    storage: {
      tone: "fault",
      title: "The case could not be read or written",
      remedy: "",
    },
    internal: {
      tone: "fault",
      title: "Something went wrong in the application",
      remedy: "This is a defect rather than anything you did.",
    },
  };

  let shown = $derived(PRESENTATION[error.code]);
</script>

<!-- assertive rather than polite: this always follows an action the user just
     took and is waiting on, so it should interrupt rather than queue. -->
<div class="notice {shown.tone}" role="alert" aria-live="assertive">
  <p class="title">
    <span class="label">{shown.tone === "fault" ? "Problem" : "Refused"}</span>
    {shown.title}
  </p>
  {#if shown.remedy}
    <p class="remedy">{shown.remedy}</p>
  {/if}
  <!-- The message is shown and never matched on. It carries the specifics the
       code cannot: which file, which scale, which value. -->
  <p class="detail">{error.message}</p>
  {#if error.code === "case_already_open" && onclose}
    <button type="button" onclick={onclose}>Close the open case</button>
  {/if}
</div>

<style>
  .notice {
    border: 1px solid;
    border-left-width: 3px;
    border-radius: 4px;
    padding: 0.75rem 1rem;
    margin: 1rem 0;
  }

  /* Tone is carried by the label word first. Colour reinforces and never
     substitutes — a refusal and a fault must be distinguishable in monochrome
     and to a screen reader. */
  .refusal,
  .wrong-input {
    border-color: #4a5568;
    background: #1b1f26;
  }

  .fault {
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
    margin-right: 0.5rem;
    vertical-align: 0.1em;
  }

  .fault .label {
    background: #4a2b2b;
    color: #ffc9c9;
  }

  .title {
    margin: 0;
    font-weight: 600;
  }

  .remedy {
    margin: 0.4rem 0 0;
    color: #c3ccd6;
  }

  .detail {
    margin: 0.4rem 0 0;
    color: #8b95a1;
    font-size: 0.875rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    overflow-wrap: anywhere;
  }

  button {
    margin-top: 0.75rem;
  }
</style>
