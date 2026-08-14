# The reference workflow

Eleven steps. This file is the authoritative numbering; everything else in the
repository defers to it.

It exists because the numbering was, until increment 19, carried only in prose
scattered across eight increment reports — and it had already drifted. Gate 5
signs off against this list, and a list that lives in nobody's file cannot be
signed off against.

## The steps

| # | Step | Delivered by | Exercised by |
|---|---|---|---|
| 1 | Create an encrypted local case | `kokin_store::create_case` | `step_01_a_case_is_created_encrypted` |
| 2 | Ingest one URL, preserving the source it came from | `kokin_ingest::ingest_url` | `step_02_a_url_becomes_a_capture` |
| 3 | Preserve integrity metadata for what was collected | `kokin_blob`, `blob`/`artifact` rows | `step_03_the_bytes_are_addressed_and_verifiable` |
| 4 | Extract normalised observations | `kokin_extract::extract_artifact` | `step_04_a_document_becomes_observations` |
| 5 | Create candidate entities and a relationship | `kokin_graph` | `step_05_entities_and_a_relationship` |
| 6 | Show the relationship with its supporting evidence | `evidence_link`, `read::entity` | `step_06_the_relationship_opens_its_evidence` |
| 7 | Accept it | `kokin_graph::assess`, review state | `step_07_a_person_accepts_it` |
| 8 | Search the case, and review the activity history | `kokin_search::search`, `audit_event` | `step_08_the_case_is_searchable_and_its_history_readable` |
| 9 | Close and reopen with no data loss | `Session::close` / `open` | `step_09_a_closed_case_reopens_unchanged` |
| 10 | Export and restore | `kokin_export::{package, verify, import}` | `step_10_a_case_survives_export_and_restore` |
| 11 | Repeat the whole sequence offline from recorded fixtures | `ReplayHttp` | `step_11_the_whole_sequence_replays_offline` |

## The drift, and how it was resolved

The master plan's gate-5 sign-off sentence is the source. Read as a list it
gives all eleven steps a distinct meaning:

> create encrypted case → ingest one URL → preserve source + integrity metadata
> → extract normalized observations → create candidate entities + relationship →
> show relationship with its evidence → accept → search + review activity
> history → close and reopen with no data loss → export and restore → repeat the
> entire sequence offline from recorded fixtures

Most increment reports agree with it: increment 3 has case creation at 1 and
close/reopen at 9, increments 6 and 7 have ingest at 2–3 and extraction at 4,
increment 8 has the analytical layer at 5, and increment 5b has replay at 11.

**Two reports disagreed.** Increments 8 and 9 both called search *step 6*.
Step 6 is showing a relationship with its evidence, and search is step 8. Both
have been corrected, with a note pointing here.

The correction is small and the mechanism that produced it is not: a number
repeated from memory across eight documents, with nothing anywhere to compare it
against. The same shape as the dangling `D-032` references in increment 17 and
the ordering lint in increment 16 — **a claim with no single definition drifts,
and every copy of it keeps looking right.**

## What each step must actually demonstrate

Passing a step means more than the call returning `Ok`.

- **Step 2** must record *where the bytes came from* — a `source` row with its
  raw and canonical locator, and a `capture` carrying the request, the redirect
  chain, and `capture_completeness`. A blob with no provenance is not step 2.
- **Step 3** must be verifiable *after* the fact: the content hash recomputes
  from the stored bytes, and a modified blob is detected.
- **Step 4**'s observations must each carry a `locator_json` pointing at where in
  the document the value came from.
- **Step 6** is the acceptance criterion *"graph edges open their supporting
  evidence"*. It fails if a relationship exists whose evidence cannot be reached
  from it.
- **Step 7** must record *who* accepted, and the audit chain must show it.
- **Step 8**'s search must return the coverage caveat with its results (D-029),
  and the activity history must be readable as a chain.
- **Step 9** compares the case before and after, not merely that it reopens.
- **Step 10** must round-trip through a *file*, verify without a passphrase, and
  restore to an openable case.
- **Step 11** is not a repetition for its own sake: under `ReplayHttp` a cache
  miss is a **test failure, never a live call**, so passing step 11 is what
  proves steps 2–10 never touched the network.

## A caveat about "recorded" fixtures

Step 11 says *recorded*. As of increment 19 no fixture in this repository was
recorded from anything, because `HttpCapability` has exactly one implementation
and it is `ReplayHttp` — there is no transport in the workspace that could have
made the request.

The acceptance fixture is therefore **synthetic**: a committed HTML file wrapped
into a `Fixture` at test setup. It proves the replay path and the offline
guarantee, and it does not prove that the recording path produces something
replayable, because there is no recording path yet.

`RecordingHttp` arrives with `LiveHttp` in increment 26. That is when this
caveat can be removed, and it should not be removed before then.
