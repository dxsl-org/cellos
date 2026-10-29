#!/usr/bin/env python3
"""Tabulate the S10-3b sub-checks (SC1 Ctrl-C, SC2 cat, the P10G-vt hostile screen) over every booted
L0 run of the rerun, by code. usage: s10_3b_checks.py <lanes-dir> <out.json>"""
import json, sys
from pathlib import Path

lanes, out = Path(sys.argv[1]), Path(sys.argv[2])
WANT = {"P10C-ctrl-c": ("sleep-ctrl-c-prompt", "sleep-ctrl-c-130", "top-batch-ctrl-c-prompt", "top-batch-ctrl-c-130",
                        "cat-sink-ctrl-c-prompt", "cat-sink-output-stops", "cat-sink-ctrl-c-130"),
        "P10C-bytes": ("disk-non-utf8-lossy-text", "disk-non-utf8-uart-bytes", "disk-oversized-file-too-large-1",
                       "disk-directory-is-a-directory-1"),
        "P10G-vt": None}
table = {}
for line in (lanes / "summary.jsonl").read_text().splitlines():
    record = json.loads(line)
    verdict = Path(record["work"]) / "evidence/verdict.json"
    if not record["lane"].startswith("L0-") or not verdict.exists():
        continue
    markers = json.loads(verdict.read_text())["markers"]
    row = {}
    for marker, checks in WANT.items():
        if marker not in markers:
            continue
        got = markers[marker]["checks"]
        if checks is None:
            row[marker] = {"marker_pass": markers[marker]["pass"],
                           "hostile": {k: c["pass"] for k, c in got.items() if "hostile" in k.lower()}}
        else:
            row[marker] = {k: got[k]["pass"] if k in got else "not-reached" for k in checks}
    table[record["lane"]] = row
counts = {}
for row in table.values():
    for marker, checks in row.items():
        items = checks["hostile"].items() if marker == "P10G-vt" else checks.items()
        for key, value in items:
            slot = counts.setdefault(f"{marker}:{key}", {"pass": 0, "fail": 0, "not-reached": 0})
            slot["pass" if value is True else "fail" if value is False else "not-reached"] += 1
out.write_text(json.dumps({"counts": counts, "runs": table}, indent=2) + "\n")
print(json.dumps(counts, indent=1))
