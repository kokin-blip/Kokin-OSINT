#!/usr/bin/env python3
"""Refuse an SQL ordering that breaks a timestamp tie on a random id.

Three times now, in three crates, this codebase has written

    ORDER BY created_utc, id

and meant "in the order these were written". It is not that. Every `*_utc`
column here is a second-resolution ISO string, and the rows being ordered are
routinely written inside one second - by one extractor pass, or by one loop
inside one transaction - so the tie is the normal case rather than an edge one.
Ids are BLAKE3 over random bytes and carry no time at all, so breaking the tie
on `id` is a coin flip.

The occurrences, all found by a test failing intermittently rather than by
review:

  A-023  kokin-search coverage: picked the wrong run, so an artifact that had
         been re-extracted reported the older run's status. A wrong answer.
  D-021  kokin-graph resolution: picked the wrong survivor of a merge. Either
         is correct, but the same case rebuilt the same way displayed a
         different name.
  D-022  kokin-graph evidence_for: returned evidence in an unstable order while
         its own doc comment promised insertion order.

The fix is always `rowid`, SQLite's insertion order. This lint exists because
the wrong version reads as obviously correct, which is why it kept being
written.

False positives: an ordering on a genuinely unique, genuinely monotonic id
column. There is no such column in this codebase. If one is added, widen
ALLOWED_TIE_BREAKS rather than deleting the check - and say why in a comment,
because that is the claim a reader will want to check.

Run: python scripts/lint_sql_ordering.py
"""

from __future__ import annotations

import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent

# `audit_event.id` is an INTEGER PRIMARY KEY - a rowid alias, monotonic by
# definition - and the audit chain is defined in terms of it. That is the one
# id column in this schema that means what an ordering wants it to mean.
ALLOWED_TIE_BREAKS = {"rowid", "audit_event.id"}

# An ORDER BY whose first term is a *_utc column, capturing everything after it
# up to the end of the clause. SQL is spread over several lines in this
# codebase, so this runs against the whole file with DOTALL off and newlines
# folded first.
ORDER_BY = re.compile(
    r"ORDER\s+BY\s+([A-Za-z_][\w.]*_utc)\s*(?:ASC|DESC)?\s*,\s*([^\"']*?)"
    r"(?=\s*(?:LIMIT|OFFSET|\)|\"|$))",
    re.IGNORECASE,
)

ID_TERM = re.compile(r"\b([\w.]*\bid)\b", re.IGNORECASE)


def offending_terms(tail: str) -> list[str]:
    """The id-shaped tie-break terms in `tail` that are not allowed."""
    bad = []
    for m in ID_TERM.finditer(tail):
        term = m.group(1)
        if term.lower() in ALLOWED_TIE_BREAKS:
            continue
        if term.lower().endswith("rowid"):
            continue
        bad.append(term)
    return bad


def main() -> int:
    problems: list[str] = []
    checked = 0

    # src-tauri writes SQL too, as of increment 16. It was outside this glob for
    # as long as it had no queries, which is exactly how a guard comes to be
    # looking at the wrong directory by the time it matters.
    sources = sorted(REPO.glob("crates/**/*.rs")) + sorted(REPO.glob("src-tauri/**/*.rs"))
    for path in sources:
        rel = path.relative_to(REPO).as_posix()
        if "/target/" in rel:
            continue
        text = path.read_text(encoding="utf-8")
        checked += 1
        # Fold newlines so a clause split across lines is still one string,
        # keeping enough of the original to report a line number.
        flat = text.replace("\r\n", "\n")
        for m in ORDER_BY.finditer(flat):
            bad = offending_terms(m.group(2))
            if not bad:
                continue
            lineno = flat.count("\n", 0, m.start()) + 1
            problems.append(
                f"{rel}:{lineno}: ORDER BY {m.group(1)} ... {', '.join(bad)} - "
                f"a second-resolution timestamp broken on a random id. "
                f"Use rowid (see D-022)."
            )

    if problems:
        print(f"::error::{len(problems)} unstable SQL ordering(s):")
        for p in problems:
            print(f"  {p}")
        return 1

    print(f"OK: no timestamp ordering broken on a random id, across {checked} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
