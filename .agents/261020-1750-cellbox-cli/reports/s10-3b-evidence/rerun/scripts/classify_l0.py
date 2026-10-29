#!/usr/bin/env python3
"""Classify every L0 run of the S10-3b rerun by code (no hand-written verdicts).

Signatures:
  F1 fixture-sshd-not-started  the runner stopped before booting the candidate (Docker sshd fixture)
  F2 nvme-io-timeout           `[nvme] io timeout` on the main boot (the edit save then fails closed)
  F3 nvme-admin-timeout        `[nvme] admin timeout` at boot (/mnt/sd not mounted)
  F4 prompt-not-idle           a marker failed on `no prompt` while the UART shows the prompt followed by
                               an asynchronous service line (driver idle detection)
usage: classify_l0.py <lanes-dir> <out.json>
"""
import json, re, sys
from pathlib import Path

lanes, out = Path(sys.argv[1]), Path(sys.argv[2])
runs = []
for line in (lanes / "summary.jsonl").read_text().splitlines():
    record = json.loads(line)
    if not record["lane"].startswith("L0-"):
        continue
    evidence = Path(record["work"]) / "evidence"
    verdict_path = evidence / "verdict.json"
    run = {"lane": record["lane"], "exit": record["exit"], "seconds": record["seconds"], "work": record["work"],
           "booted": verdict_path.exists(), "failed_markers": {}, "classes": []}
    serial = {p.name: p.read_text(errors="replace") for p in evidence.glob("serial-*.log")
              if not p.name.endswith(".raw.log")}
    if verdict_path.exists():
        verdict = json.loads(verdict_path.read_text())
        run["qualifying"] = verdict.get("qualifying")
        for name, marker in verdict["markers"].items():
            if not marker["pass"]:
                run["failed_markers"][name] = {
                    "checks": [k for k, c in marker["checks"].items() if not c["pass"]],
                    "error": (marker.get("error") or "")[:240]}
    if record["exit"] != 0:
        if not run["booted"] and any("fixture sshd did not start" in l for l in record["final_lines"]):
            run["classes"].append("F1 fixture-sshd-not-started")
        main = serial.get("serial-main.log", "")
        if re.search(r"\[nvme\] io timeout", main):
            run["classes"].append("F2 nvme-io-timeout")
        if re.search(r"\[nvme\] admin timeout", main):
            run["classes"].append("F3 nvme-admin-timeout")
        # F4 only when no NVMe timeout explains the stall; the boot whose marker failed is checked
        # (the ssh boot for P10C-ssh-serial, else the main boot).
        boot = "serial-ssh.log" if "P10C-ssh-serial" in run["failed_markers"] else "serial-main.log"
        if not run["classes"] and re.search(r"Cellos > \nUSER: \[(net-bridge|serial)\][^\n]*\n",
                                            serial.get(boot, "")) and any(
                "no prompt after" in m["error"] for m in run["failed_markers"].values()):
            run["classes"].append(f"F4 prompt-not-idle ({boot})")
        if not run["classes"]:
            run["classes"].append("UNCLASSIFIED")
    runs.append(run)


def streak(profile):
    best = cur = 0
    for run in runs:
        if run["lane"].startswith(f"L0-{profile}-"):
            cur = cur + 1 if run["exit"] == 0 else 0
            best = max(best, cur)
    return best, cur


summary = {}
for profile in ("headless", "desktop"):
    mine = [r for r in runs if r["lane"].startswith(f"L0-{profile}-")]
    best, final = streak(profile)
    classes = {}
    for r in mine:
        for c in r["classes"]:
            classes[c.split(" (")[0]] = classes.get(c.split(" (")[0], 0) + 1
    summary[profile] = {"runs": len(mine), "pass": sum(r["exit"] == 0 for r in mine),
                        "booted": sum(r["booted"] for r in mine),
                        "booted_pass": sum(r["booted"] and r["exit"] == 0 for r in mine),
                        "longest_consecutive_pass": best, "final_streak": final, "failure_classes": classes}
result = {"summary": summary, "runs": runs,
          "unclassified": [r["lane"] for r in runs if "UNCLASSIFIED" in r["classes"]]}
out.write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result["summary"], indent=1))
print("unclassified:", result["unclassified"])
