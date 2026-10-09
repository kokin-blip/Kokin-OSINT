# Threat model

Status: implementation summary updated 2026-10-08; release security verification remains open.

SQLCipher and encrypted blobs, parameterized writes, citation verification,
text-only evidence display, machine-actor restrictions, and evidence grounding
have deterministic contract tests. Live HTTP now routes exclusively through
`kokin-net`: vetted/pinned destination addresses, TLS hostname verification,
redirect checks, no proxy inheritance, rate/size/time limits and offline policy.
Transport tests are synthetic; live-service behavior needs separate verification.

Native file selections produce short-lived single-use generation-bound tokens;
background collection takes no case lock during network I/O. Writes are serialized
and stale generations cannot commit. Atomic writes become non-cancellable once
started. Restore/export stage before publishing; failed stages are cleaned up.
Filesystem metadata checks do not eliminate all races with hostile local writers.

AI, browser execution and third-party plugins remain planned. The historical
attack catalog records granular controls, not a current security score: its
original aggregate counts are no longer authoritative. Audit chains are tamper-
evident within the application trust boundary, not signatures or chain of custody.

## Assets, in priority order

1. **Case contents** — collected evidence about real people, often before any
   wrongdoing is established. Disclosure is the worst outcome this system can
   produce, and the people harmed are usually not the user.
2. **The integrity of the evidence-to-conclusion link.** A tool that silently
   attaches the wrong evidence to a conclusion is worse than no tool, because it
   produces confident errors.
3. **The investigator's identity and intent.** Which targets are being examined,
   and when, is itself sensitive and leaks through network egress.
4. **Availability** — last. A crashed app is recoverable; a leaked case is not.

## Trust boundaries

### B1 — disk ↔ application

Anything with filesystem access: a stolen laptop, another local account, a
backup tool, a sync client.

Full-disk encryption does not help here, because the disk is decrypted exactly
when the user is logged in. Controls are SQLCipher page-level encryption over
the whole case file including the FTS index (ADR-0004), and per-blob envelope
encryption. **A-016** covers the failure mode where a build silently links plain
SQLite — the one defect that every functional test would pass.

### B2 — application ↔ network

The only sanctioned crossing is `kokin-net` (ADR-0010). Attacks: **A-001**
SSRF, **A-002** DNS rebinding, **A-003** scheme abuse, **A-004** cross-host
redirect, **A-011** oversized response, **A-013** tracking-parameter leakage,
**A-015** network access added outside the broker.

A-015 is the meta-attack: it is not an attack on a control but on the
architecture that makes the controls reachable, which is why it is enforced by a
CI lint rather than by review.

### B3 — collected content ↔ processing

**All collected content is hostile input.** This boundary exists because it is
the one people forget: a captured page is data, and its resemblance to
instructions is a property of the attacker's choosing.

Attacks: **A-005** prompt injection, **A-009** HTML injection, **A-010** archive
bombs, **A-012** malformed media. The controls are ordinary parser hygiene plus
one structural rule — the model has no tools bound, so there is no instruction a
page can contain that reaches an action.

### B4 — AI ↔ analytical layer

Model output is a suggestion about evidence, never a fact. **A-006** (uncited
assertion) is blocked by a schema `CHECK`, not by UI discipline, because UI
discipline is what erodes first.

### B5 — automation ↔ analytical layer

**A-007**: a similarity score must never merge two entities. Enforced by a
`CHECK` rejecting `rule:` and `ai:` actors on merges (ADR-0008). A wrong merge
conflates two real people, and under a destructive merge design it is
unrecoverable — hence merge-as-projection.

### B6 — application ↔ export consumers

Exports are opened by spreadsheets, browsers, and other tools that treat data as
executable. **A-008** CSV formula injection is neutralised at the writer so no
call site can forget it.

### B7 — case owner ↔ audit log

**A-017 is not mitigated, and cannot be.** Anyone holding the case passphrase
can recompute the hash chain from genesis and produce a valid falsified history.
The chain is tamper-evident against outsiders and offers nothing against the
case owner, who is inside the trust boundary.

This is recorded as an accepted limitation rather than a gap to close, because
no local-only design closes it. It is why ADR-0014 forbids describing this
system as chain of custody, and why the disclaimer must appear in the UI rather
than only in this file.

### B8 — repository ↔ contributors

**A-014**: recorded fixtures are captured from live services and can retain
cookies, tokens, or client IPs, which a commit makes permanent. A versioned
sanitiser runs on record and a CI job greps for secret patterns.

**A-018**: a replay cache miss must fail the test, never fall through to a live
call — otherwise the offline guarantee is decorative. The whole suite runs under
`KOKIN_NETWORK=deny`.

## Out of scope for Phase 1

A malicious build toolchain, a compromised OS, hardware attacks, and a coerced
user. Also out of scope: protecting the user from themselves — someone who
exports a case and emails it has left the system's control, and no control here
pretends otherwise.
