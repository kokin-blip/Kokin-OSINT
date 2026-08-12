#!/usr/bin/env python3
"""Enforce that research claims are sourced, or are labelled as inferred.

The spec requires inferred conclusions to be labelled as such. Prose saying so
is aspirational; this makes it a build failure.

Every row in research/*.csv must either cite a source_id that resolves to a row
in research/source-registry.csv, or set inferred=true and give a rationale. A
row that does neither is an unattributed claim, and unattributed claims are the
exact failure mode this project exists to avoid in its own output.

Run: python scripts/lint_research.py
Exit code 1 on any violation, with every violation listed rather than just the
first, so one CI run reports the full set of problems.
"""

from __future__ import annotations

import csv
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
RESEARCH = REPO / "research"
REGISTRY = RESEARCH / "source-registry.csv"

SOURCE_ID = re.compile(r"^S-\d{3}$")
ISO_DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")

# Registry columns that must be present and non-empty on every row. A source
# without a URL and an access date cannot be re-checked by a reviewer, which
# defeats the point of recording it.
REGISTRY_REQUIRED = ["source_id", "title", "url", "accessed", "type"]

# Rows in a claim file may cite several sources; this splits "S-001;S-004".
MULTI_SEP = ";"


def fail(problems: list[str], msg: str) -> None:
    problems.append(msg)


def load_registry(problems: list[str]) -> set[str]:
    """Return the set of known source_ids, reporting registry defects."""
    if not REGISTRY.exists():
        fail(problems, f"{REGISTRY.relative_to(REPO)} is missing")
        return set()

    ids: set[str] = set()
    with REGISTRY.open(newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        missing_cols = [c for c in REGISTRY_REQUIRED if c not in (reader.fieldnames or [])]
        if missing_cols:
            fail(problems, f"source-registry.csv lacks columns: {', '.join(missing_cols)}")
            return set()

        for lineno, row in enumerate(reader, start=2):
            where = f"source-registry.csv:{lineno}"
            sid = (row.get("source_id") or "").strip()

            if not SOURCE_ID.match(sid):
                fail(problems, f"{where}: source_id {sid!r} is not of the form S-001")
                continue
            if sid in ids:
                fail(problems, f"{where}: duplicate source_id {sid}")
            ids.add(sid)

            for col in REGISTRY_REQUIRED:
                if not (row.get(col) or "").strip():
                    fail(problems, f"{where}: {sid} has an empty {col}")

            accessed = (row.get("accessed") or "").strip()
            if accessed and not ISO_DATE.match(accessed):
                fail(problems, f"{where}: {sid} accessed={accessed!r} is not YYYY-MM-DD")

            url = (row.get("url") or "").strip()
            # Not every source is a web page (a licence file in a repo, a book),
            # but anything claiming to be a URL must actually be fetchable.
            if url and not url.startswith(("http://", "https://")):
                fail(problems, f"{where}: {sid} url {url!r} is not http(s)")

    return ids


def check_claim_file(path: pathlib.Path, known: set[str], problems: list[str]) -> int:
    """Validate one claims CSV. Returns the number of rows checked."""
    rows = 0
    with path.open(newline="", encoding="utf-8") as fh:
        reader = csv.DictReader(fh)
        cols = reader.fieldnames or []
        name = path.name

        for required in ("source_id", "inferred"):
            if required not in cols:
                fail(problems, f"{name}: missing the {required!r} column")
        if "source_id" not in cols or "inferred" not in cols:
            return 0

        # An inferred row has to say why it is inferred; otherwise inferred=true
        # becomes a way to launder unsourced claims past this check.
        rationale_col = next(
            (c for c in ("rationale", "notes", "inference_basis") if c in cols), None
        )
        if rationale_col is None:
            fail(problems, f"{name}: needs a rationale, notes, or inference_basis column")

        for lineno, row in enumerate(reader, start=2):
            rows += 1
            where = f"{name}:{lineno}"

            # DictReader silently pads short rows with None and dumps extras
            # under the None key, so a single missing comma shifts every later
            # column and surfaces as a baffling error about some unrelated
            # field. Check the arity first and say so directly.
            if None in row or any(v is None for v in row.values()):
                actual = len([v for v in row.values() if v is not None])
                if None in row:
                    actual += len(row[None])
                fail(
                    problems,
                    f"{where}: has {actual} fields, header has {len(cols)} - "
                    "a column is missing or extra, so every later column is shifted",
                )
                continue

            inferred = (row.get("inferred") or "").strip().lower()
            cited = (row.get("source_id") or "").strip()

            if inferred not in ("true", "false", ""):
                fail(problems, f"{where}: inferred={inferred!r} must be true or false")

            if inferred == "true":
                if rationale_col and not (row.get(rationale_col) or "").strip():
                    fail(
                        problems,
                        f"{where}: inferred=true but {rationale_col} is empty - "
                        "say what the inference rests on",
                    )
                continue

            if not cited:
                fail(
                    problems,
                    f"{where}: no source_id and not marked inferred=true - "
                    "unattributed claim",
                )
                continue

            for sid in (s.strip() for s in cited.split(MULTI_SEP)):
                if not sid:
                    continue
                if not SOURCE_ID.match(sid):
                    fail(problems, f"{where}: source_id {sid!r} is malformed")
                elif sid not in known:
                    fail(problems, f"{where}: source_id {sid} is not in source-registry.csv")

    return rows


def main() -> int:
    problems: list[str] = []

    if not RESEARCH.is_dir():
        print("research/ does not exist yet - nothing to lint")
        return 0

    known = load_registry(problems)
    claim_files = sorted(p for p in RESEARCH.glob("*.csv") if p != REGISTRY)

    total = 0
    for path in claim_files:
        total += check_claim_file(path, known, problems)

    if problems:
        print(f"::error::{len(problems)} research citation problem(s):")
        for p in problems:
            print(f"  {p}")
        return 1

    print(
        f"OK: {len(known)} sources; {total} claim rows across "
        f"{len(claim_files)} file(s), all cited or labelled inferred"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
