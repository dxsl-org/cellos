#!/usr/bin/env python3
"""Resume L0 from run_lane.py's append-only summary; never write that summary ourselves.

Run: python3 -B scripts/phase10_l0_stream.py --candidate CANDIDATE.json \
         --run-lane /tmp/PRIVATE/lanes/run_lane.py
Replay only: add --dry-run (or use --summary FILE --dry-run without --run-lane).
Exit: 0 complete (or a dry-run inspection), 1 counted-run bound, 3 infrastructure blocker.
The supplied run_lane.py must live in the private evidence directory, not the historical archive.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

LANE = re.compile(r"^L0-(headless|desktop)-run([0-9]+)$")


def read_summary(path):
    path = Path(path)
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()] if path.exists() else []


def evaluate(records, profiles=("headless", "desktop"), required=6, max_runs=12):
    states = {profile: {"attempts": 0, "counted_runs": 0, "uncounted": 0, "passes": 0,
                        "streak": 0, "longest_streak": 0, "next_attempt": 1} for profile in profiles}
    infra_streak = 0
    blocked = False
    for record in records:
        match = LANE.fullmatch(record["lane"])
        if not match or match[1] not in states:
            continue
        state = states[match[1]]
        state["attempts"] += 1
        state["next_attempt"] = max(state["next_attempt"], int(match[2]) + 1)
        if record["exit"] == 3:
            state["uncounted"] += 1
            infra_streak += 1
            blocked |= infra_streak >= 3
            continue
        infra_streak = 0
        state["counted_runs"] += 1
        state["passes"] += record["exit"] == 0
        state["streak"] = state["streak"] + 1 if record["exit"] == 0 else 0
        state["longest_streak"] = max(state["longest_streak"], state["streak"])
    return {"profiles": states, "infrastructure_streak": infra_streak, "infrastructure_blocked": blocked,
            "complete": all(s["streak"] >= required for s in states.values()),
            "exhausted": [p for p, s in states.items() if s["counted_runs"] >= max_runs and s["streak"] < required]}


def orchestrate(candidate, run_lane, profiles=("headless", "desktop"), required=6, max_runs=12,
                invoke=None, report=None):
    """Only run_lane appends records. Re-read its output after every attempt, including status 3."""
    invoke = invoke or subprocess.call
    report = report or (lambda state: print(json.dumps(state), flush=True))
    run_lane = Path(run_lane).resolve()
    summary = run_lane.parent / "summary.jsonl"
    repo = Path(__file__).resolve().parent.parent
    while True:
        records = read_summary(summary)
        state = evaluate(records, profiles, required, max_runs)
        report(state)
        if state["infrastructure_blocked"]:
            return 3
        if state["complete"]:
            return 0
        pending = [p for p in profiles if state["profiles"][p]["streak"] < required
                   and state["profiles"][p]["counted_runs"] < max_runs]
        if not pending:
            return 1
        profile = pending[0]
        lane = f"L0-{profile}-run{state['profiles'][profile]['next_attempt']}"
        status = invoke([sys.executable, "-B", str(run_lane), lane, "/tmp/cellbox-phase10-", "P10_WORK", "--",
                         "scripts/qemu-phase10-canonical-test.sh", "--candidate", str(candidate),
                         "--profile", profile, "--work", "{work}"], cwd=repo)
        updated = read_summary(summary)
        # A crashed recorder is infrastructure, not a fabricated run. Keep all its output intact.
        if (len(updated) != len(records) + 1 or updated[:-1] != records
                or updated[-1].get("lane") != lane or updated[-1].get("exit") != status):
            raise RuntimeError(f"run_lane did not append exactly one matching record for {lane} (exit {status})")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--run-lane", type=Path)
    parser.add_argument("--summary", type=Path, help="replay input; only accepted with --dry-run")
    parser.add_argument("--profiles", nargs="+", choices=("headless", "desktop"), default=["headless", "desktop"])
    parser.add_argument("--required", type=int, default=6)
    parser.add_argument("--max-runs", type=int, default=12)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args(argv)
    if args.required < 1 or args.max_runs < args.required or len(set(args.profiles)) != len(args.profiles):
        parser.error("require unique profiles and 1 <= required <= max-runs")
    if args.dry_run:
        if not args.summary and not args.run_lane:
            parser.error("--dry-run requires --summary or --run-lane")
        state = evaluate(read_summary(args.summary or args.run_lane.parent / "summary.jsonl"),
                         args.profiles, args.required, args.max_runs)
        print(json.dumps(state, indent=2))
        return 3 if state["infrastructure_blocked"] else 1 if state["exhausted"] else 0
    if args.summary or not args.candidate or not args.run_lane:
        parser.error("a stream requires --candidate and --run-lane; --summary is replay-only")
    if not args.run_lane.is_file():
        parser.error("--run-lane must name the private run_lane.py recorder")
    try:
        return orchestrate(args.candidate.resolve(), args.run_lane, args.profiles, args.required, args.max_runs)
    except (OSError, ValueError, RuntimeError) as exc:
        print(f"FAIL: L0 recorder infrastructure: {exc}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    raise SystemExit(main())
