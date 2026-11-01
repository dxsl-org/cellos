#!/usr/bin/env python3
"""Report-only A/B summary of two benchmark capture sets from one machine.

Usage:
    python3 scripts/bench_ab.py RESULTS_DIR --baseline ID [ID ...] --candidate ID [ID ...]

Both sets must be VALID captures of one compatibility group (same profile,
QEMU shape, toolchain, kernel features and machine fingerprint); only source
and artifact identity may differ. Each observation (``tier/path/metric/stat``)
is reduced to its median per set, so SAS and domain values never merge. The
verdict is informational: an A/B on an unqualified machine (unknown governor
or timer) is evidence of direction and size, never a budget PASS/FAIL.

Exit status: 0 summary printed, 1 invalid or incomparable input, 2 usage.
"""

from __future__ import annotations

import argparse
import statistics
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from bench_results import (  # noqa: E402
    _compatibility_key,
    _observations,
    _unqualified_fields,
    load_json_text,
    validate_capture,
)


def load_set(results_dir: Path, ids: list[str]) -> tuple[list[dict[str, Any]], list[str]]:
    documents: list[dict[str, Any]] = []
    errors: list[str] = []
    for capture_id in ids:
        path = results_dir / f"perf-{capture_id}.json"
        try:
            document = load_json_text(path.read_text(encoding="utf-8"))
        except Exception as exc:  # noqa: BLE001 - report every unreadable capture
            errors.append(f"{capture_id}: unreadable ({exc})")
            continue
        problems = validate_capture(document, results_dir)
        if problems:
            errors.append(f"{capture_id}: INVALID ({problems[0]})")
            continue
        documents.append(document)
    return documents, errors


def medians(documents: list[dict[str, Any]]) -> dict[str, tuple[float, str]]:
    series: dict[str, list[float]] = {}
    directions: dict[str, str] = {}
    for document in documents:
        for key, (value, direction) in _observations(document).items():
            series.setdefault(key, []).append(value)
            directions[key] = direction
    return {key: (float(statistics.median(values)), directions[key]) for key, values in series.items()}


def summarize(baseline: list[dict[str, Any]], candidate: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Per-observation rows; ``change_percent`` is None when the baseline is zero
    or the observation exists in only one set."""
    base, cand = medians(baseline), medians(candidate)
    rows = []
    for key in sorted(set(base) | set(cand)):
        b, c = base.get(key), cand.get(key)
        change = None
        if b is not None and c is not None and b[0] != 0:
            change = (c[0] - b[0]) / b[0] * 100.0
        rows.append({
            "observation": key,
            "baseline": None if b is None else b[0],
            "candidate": None if c is None else c[0],
            "direction": (b or c)[1],
            "change_percent": change,
        })
    return rows


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("results_dir")
    parser.add_argument("--baseline", nargs="+", required=True)
    parser.add_argument("--candidate", nargs="+", required=True)
    args = parser.parse_args(argv)

    results_dir = Path(args.results_dir).resolve()
    baseline, base_errors = load_set(results_dir, args.baseline)
    candidate, cand_errors = load_set(results_dir, args.candidate)
    for error in base_errors + cand_errors:
        print(f"[ab] INVALID {error}")
    if base_errors or cand_errors:
        return 1
    groups = {_compatibility_key(document) for document in baseline + candidate}
    if len(groups) != 1:
        print("[ab] INCOMPARABLE: captures span different profile/QEMU/toolchain/features/fingerprint groups")
        return 1

    unqualified = sorted({field for document in baseline + candidate for field in _unqualified_fields(document)})
    accelerator = baseline[0]["capture"]["qemu"]["accelerator"]
    print(f"[ab] profile={baseline[0]['capture']['profile']} accelerator={accelerator} "
          f"baseline={len(baseline)} candidate={len(candidate)} (medians per set)")
    print(f"[ab] QUALIFICATION: {'UNQUALIFIED (unknown: ' + ', '.join(unqualified) + ')' if unqualified else 'qualified machine'}; report-only")
    for row in summarize(baseline, candidate):
        change = "n/a" if row["change_percent"] is None else f"{row['change_percent']:+.1f}%"
        better = "lower" if row["direction"] == "lower" else "higher"
        print(f"[ab]   {row['observation']}: baseline={row['baseline']} candidate={row['candidate']} "
              f"change={change} ({better} is better)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
