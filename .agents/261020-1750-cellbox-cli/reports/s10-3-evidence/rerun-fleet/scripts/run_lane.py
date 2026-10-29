#!/usr/bin/env python3
"""S10-3 lane harness: run one existing lane runner (import mode or default) in a fresh mktemp work
dir, keep its full log, and append one JSON record (produced by code) to lanes/summary.jsonl.

usage: run_lane.py <name> <work-prefix> <work-env-var> [ENV=VAL ...] -- <runner> [args...]
"""
import json, os, re, subprocess, sys, tempfile, time
from pathlib import Path

LANES = Path(__file__).resolve().parent
REPO = Path("/home/dmin/cellos")
name, prefix, work_var, *rest = sys.argv[1:]
split = rest.index("--")
env_pairs, command = rest[:split], rest[split + 1:]
work = tempfile.mkdtemp(prefix=prefix)
env = {k: v for k, v in os.environ.items()
       if k not in ("CELLBOX_PRIVATE_FIXTURE_SIGNING", "CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI", "RUSTFLAGS")}
env[work_var] = work
for pair in env_pairs:
    key, _, value = pair.partition("=")
    env[key] = value
log = LANES / f"{name}.log"
start = time.time()
with open(log, "w") as handle:
    rc = subprocess.run(["bash", *command], cwd=REPO, env=env, stdout=handle, stderr=subprocess.STDOUT).returncode
text = log.read_text(errors="replace")
lines = text.splitlines()
summary = [l for l in lines if re.match(r"^(PASS|FAIL)\b.*(checks|markers|PASS|verdict|runtime)", l)][-3:]
record = {"lane": name, "work": work, "exit": rc, "seconds": round(time.time() - start, 1),
          "env": env_pairs, "command": command, "final_lines": lines[-4:], "summary_lines": summary,
          "verdict": "PASS" if rc == 0 else "FAIL"}
for candidate in ("evidence/verdict.json", "evidence/payload-match.json", "evidence/candidate-provenance.json"):
    path = Path(work) / candidate
    record[candidate.split("/")[-1]] = str(path) if path.exists() else None
with open(LANES / "summary.jsonl", "a") as out:
    out.write(json.dumps(record) + "\n")
print(json.dumps({k: record[k] for k in ("lane", "exit", "seconds", "work", "summary_lines")}))
sys.exit(rc)
