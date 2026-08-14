#!/usr/bin/env python3
"""Check that every internal identifier reference in the docs resolves.

This project's thesis is that a claim should always be traceable to what
supports it. Documentation that cites D-011 when it means D-010, or a control
that names a test_id nothing will ever define, is that same defect in the
project's own output - and it is invisible until someone follows the link and
finds the wrong thing.

Checked reference forms:
  ADR-0007 / docs/adr/0007-*   -> a file in docs/adr/
  D-012                        -> a row in docs/decision-log.csv
  R-013                        -> a row in docs/risk-register.csv
  A-005                        -> a row in docs/threat-model/attack-catalog.csv
  S-009                        -> a row in research/source-registry.csv
  P1..P8                       -> the PoC identifiers named in the plan

Run: python scripts/lint_crossrefs.py
"""

from __future__ import annotations

import csv
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent

# Where each prefix's valid ids come from: (csv path, id column).
REGISTRIES = {
    "D": ("docs/decision-log.csv", "decision_id"),
    "R": ("docs/risk-register.csv", "risk_id"),
    "A": ("docs/threat-model/attack-catalog.csv", "attack_id"),
    "S": ("research/source-registry.csv", "source_id"),
}

# PoC ids are defined in the plan rather than in a CSV, so they are enumerated
# here. Widen this when a PoC is added.
KNOWN_POCS = {f"P{n}" for n in range(1, 9)}

REF_PATTERN = re.compile(r"\b(?:(ADR)-(\d{4})|([DRAS])-(\d{3})|(P[1-8]))\b")

# Files to scan. Deliberately excludes the CSVs' own id columns, which are
# definitions rather than references - a registry defining D-012 is not a
# reference to it.
#
# The Rust sources are here because that is where most of these references now
# live. This lint checked Markdown only for sixteen increments, during which the
# argument for every control moved into the doc comment beside it - so a module
# note citing a decision that was never written scanned clean, and nine of them
# had accumulated by increment 17. The same widening the ordering lint needed at
# increment 16 (D-022), for the same reason: a guard scoped to where a problem
# has occurred stops covering where it will, and reports OK the whole time.
SCAN_GLOBS = [
    "docs/**/*.md",
    "research/**/*.md",
    "README.md",
    "*.md",
    "crates/**/*.rs",
    "src-tauri/**/*.rs",
    "scripts/*.py",
]


def load_ids(problems: list[str]) -> dict[str, set[str]]:
    known: dict[str, set[str]] = {}
    for prefix, (rel, col) in REGISTRIES.items():
        path = REPO / rel
        if not path.exists():
            problems.append(f"{rel} is missing, so {prefix}-NNN references cannot be checked")
            known[prefix] = set()
            continue
        with path.open(newline="", encoding="utf-8") as fh:
            known[prefix] = {
                (row.get(col) or "").strip()
                for row in csv.DictReader(fh)
                if (row.get(col) or "").strip()
            }
    return known


def adr_numbers() -> set[str]:
    return {p.name[:4] for p in (REPO / "docs" / "adr").glob("*.md")}


def main() -> int:
    problems: list[str] = []
    known = load_ids(problems)
    adrs = adr_numbers()

    seen = 0
    files: list[pathlib.Path] = []
    for pattern in SCAN_GLOBS:
        files.extend(REPO.glob(pattern))

    for path in sorted(set(files)):
        rel = path.relative_to(REPO).as_posix()
        for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            for m in REF_PATTERN.finditer(line):
                seen += 1
                if m.group(1):  # ADR-NNNN
                    num = m.group(2)
                    if num not in adrs:
                        problems.append(
                            f"{rel}:{lineno}: ADR-{num} has no file in docs/adr/"
                        )
                elif m.group(3):  # D/R/A/S-NNN
                    prefix, num = m.group(3), m.group(4)
                    ident = f"{prefix}-{num}"
                    if ident not in known.get(prefix, set()):
                        source = REGISTRIES[prefix][0]
                        problems.append(f"{rel}:{lineno}: {ident} is not in {source}")
                else:  # PoC
                    poc = m.group(5)
                    if poc not in KNOWN_POCS:
                        problems.append(f"{rel}:{lineno}: unknown PoC {poc}")

    if problems:
        print(f"::error::{len(problems)} unresolved cross-reference(s):")
        for p in problems:
            print(f"  {p}")
        return 1

    print(f"OK: {seen} internal references across {len(set(files))} files all resolve")
    return 0


if __name__ == "__main__":
    sys.exit(main())
