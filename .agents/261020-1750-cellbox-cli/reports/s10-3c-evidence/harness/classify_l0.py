#!/usr/bin/env python3
"""Classify S10-3c L0 evidence without editing the recorder's summary or historical scripts.

From the repository root:
  python3 -B /tmp/cellbox-s10-3c-harness/classify_l0.py LANES_DIR OUT.json
"""

import argparse
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path.cwd() / "scripts"))
from phase10_l0_stream import LANE, evaluate, read_summary


def nvme_events(serial):
    events = []
    for boot, text in serial.items():
        for number, line in enumerate(text.splitlines(), 1):
            match = re.search(r"\[nvme\] (slow completion (admin|io)|(?:admin|io) timeout|(?:reset|RDY) wait failed|monotonic clock unavailable)", line)
            if not match:
                continue
            event = {"boot": boot, "line_number": number, "line": line, "event": match[1],
                     "elapsed_ms": None, "polls": None}
            fields = {key: int(value) for key, value in re.findall(r"\b(opc|lba|cid|ms)=(\d+)\b", line)}
            event.update({k: v for k, v in fields.items() if k != "ms"})
            elapsed = re.search(r"(?:after|in) (\d+) ms\b", line)
            event["elapsed_ms"] = fields.get("ms", int(elapsed[1]) if elapsed else None)
            polls = re.search(r"(?:after|in) (\d+) polls\b", line)
            event["polls"] = int(polls[1]) if polls else None
            events.append(event)
    return events


def classify(lanes):
    records = read_summary(lanes / "summary.jsonl")
    runs = []
    for record in records:
        if not LANE.fullmatch(record["lane"]):
            continue
        evidence = Path(record["work"]) / "evidence"
        verdict_path = evidence / "verdict.json"
        run = {"lane": record["lane"], "exit": record["exit"], "seconds": record["seconds"],
               "work": record["work"], "counted": record["exit"] != 3,
               "booted": (evidence / "qemu-started").exists() or verdict_path.exists(),
               "failed_markers": {}, "classes": []}
        serial = {p.name: p.read_text(errors="replace") for p in evidence.glob("serial-*.log")
                  if not p.name.endswith(".raw.log")}
        run["nvme_events"] = nvme_events(serial)
        run["slow_completions"] = [e for e in run["nvme_events"] if e["event"].startswith("slow completion")]
        run["max_slow_completion_ms"] = max((e["elapsed_ms"] for e in run["slow_completions"]
                                              if e["elapsed_ms"] is not None), default=None)
        provenance = evidence / "provenance.json"
        run["runner_seconds"] = json.loads(provenance.read_text()).get("run_seconds") if provenance.exists() else None
        if verdict_path.exists():
            verdict = json.loads(verdict_path.read_text())
            run["qualifying"] = verdict.get("qualifying")
            for name, marker in verdict["markers"].items():
                if not marker["pass"]:
                    run["failed_markers"][name] = {
                        "checks": [key for key, check in marker["checks"].items() if not check["pass"]],
                        "error": (marker.get("error") or "")[:240]}
        if record["exit"] != 0:
            if record["exit"] == 3:
                run["classes"].append("H2 status-3-with-boot" if run["booted"] else "F1 pre-QEMU-infrastructure")
            elif not run["booted"] and any("fixture sshd did not start" in line for line in record.get("final_lines", [])):
                run["classes"].append("F1 fixture-sshd-not-started")
            combined = "\n".join(serial.values())
            if "[nvme] io timeout" in combined:
                run["classes"].append("F2 nvme-io-timeout")
            if "[nvme] admin timeout" in combined:
                run["classes"].append("F3 nvme-admin-timeout")
            boot = "serial-ssh.log" if "P10C-ssh-serial" in run["failed_markers"] else "serial-main.log"
            if not run["classes"] and re.search(
                    r"(?:^|\n)USER: Cellos > \r?\n(?:(?:\[\s?(?:INFO|WARN|ERROR|DEBUG|TRACE)\][^\n]*\n))*USER: \[[a-z0-9-]+\] ",
                    serial.get(boot, "")) and any("no prompt after" in m["error"] for m in run["failed_markers"].values()):
                run["classes"].append(f"F4 prompt-not-idle ({boot})")
            if not run["classes"]:
                run["classes"].append("UNCLASSIFIED")
        runs.append(run)
    stream = evaluate(records)
    summary = {}
    for profile, state in stream["profiles"].items():
        mine = [r for r in runs if r["lane"].startswith(f"L0-{profile}-")]
        classes = {}
        for run in mine:
            for kind in run["classes"]:
                key = kind.split(" (")[0]
                classes[key] = classes.get(key, 0) + 1
        summary[profile] = {"attempts": state["attempts"], "runs": state["counted_runs"],
                            "uncounted": state["uncounted"], "pass": state["passes"],
                            "booted": sum(r["booted"] for r in mine),
                            "booted_pass": sum(r["booted"] and r["exit"] == 0 for r in mine),
                            "longest_consecutive_pass": state["longest_streak"], "final_streak": state["streak"],
                            "slow_completions": sum(len(r["slow_completions"]) for r in mine),
                            "failure_classes": classes}
    return {"summary": summary, "stream": stream, "runs": runs,
            "unclassified": [r["lane"] for r in runs if "UNCLASSIFIED" in r["classes"]]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lanes", type=Path)
    parser.add_argument("out", type=Path)
    args = parser.parse_args()
    result = classify(args.lanes)
    args.out.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))
    print("unclassified:", result["unclassified"])


if __name__ == "__main__":
    main()
