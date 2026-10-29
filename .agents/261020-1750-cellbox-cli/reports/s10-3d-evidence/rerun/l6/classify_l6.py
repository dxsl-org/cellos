#!/usr/bin/env python3
"""S10-3 L6 classification (produced by code): before/after per test.
pass->pass ok; pass->fail REGRESSION (stops the slice); fail->fail pre-existing; fail->pass fixed;
skip on either side recorded as skip."""
import json, sys
from pathlib import Path

base = Path(sys.argv[1])
before = json.loads((base / "before/l6.json").read_text())
after = json.loads((base / "after/l6.json").read_text())
rows, regressions = [], []
for consumer, a in after["consumers"].items():
    b = before["consumers"].get(consumer, {"tests": {}})
    for test in sorted(set(a["tests"]) | set(b["tests"])):
        sb, sa = b["tests"].get(test, "absent"), a["tests"].get(test, "absent")
        if sb == "pass" and sa == "pass":
            verdict = "pass"
        elif sb == "pass" and sa != "pass":
            verdict = "REGRESSION"
            regressions.append(f"{consumer}::{test}")
        elif sb == "fail" and sa == "fail":
            verdict = "pre-existing-failure"
        elif sb == "fail" and sa == "pass":
            verdict = "fixed"
        elif "skip" in (sb, sa):
            verdict = "skip"
        else:
            verdict = f"{sb}->{sa}"
        rows.append({"consumer": consumer, "test": test, "before": sb, "after": sa, "verdict": verdict})
summary = {"before_isos": before["isos"], "after_isos": after["isos"], "rows": rows,
           "counts": {v: sum(r["verdict"] == v for r in rows) for v in sorted({r["verdict"] for r in rows})},
           "regressions": regressions, "verdict": "FAIL" if regressions else "PASS"}
(base / "l6-classification.json").write_text(json.dumps(summary, indent=2) + "\n")
for r in rows:
    print(f"{r['verdict']:22} {r['consumer']}::{r['test']} ({r['before']} -> {r['after']})")
print("counts", summary["counts"], "regressions", regressions, "VERDICT", summary["verdict"])
