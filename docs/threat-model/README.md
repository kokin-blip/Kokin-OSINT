# Threat model

Status: **draft**, to be revised at gate 5.

STRIDE per trust boundary. Every attack class in this document has a row in
`attack-catalog.csv` carrying a control, the crate that implements it, and a
`test_id`. A control without a test is a wish, so the catalog's `status` column
distinguishes `active` (test exists and runs in CI) from `planned`.

Current state: **3 active, 7 partial, 7 planned, 1 accepted limitation.**

`partial` is doing real work in that count. The egress-broker rows (A-001 to
A-004, A-011) have their policy implemented and tested exhaustively offline —
but **no live transport routes through them yet**, so nothing is actually being
protected in production. They are not `active` and must not be described as
though they were. That ratio is the honest measure of how far the security
architecture is from implemented, and it should be quoted whenever this
project's security posture is described.

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
