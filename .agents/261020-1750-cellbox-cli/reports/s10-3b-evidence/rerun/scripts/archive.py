#!/usr/bin/env python3
"""Copy the S10-3b-rerun evidence into the plan reports (small text/JSON files only; screenshots stay
in /tmp). Mirrors the layout of reports/s10-3-evidence/rerun-fleet."""
import json, shutil, sys
from pathlib import Path

W = Path("/tmp/cellbox-s10-3b-tsbn")
DEST = Path(sys.argv[1])
TEXT = {".json", ".log", ".txt", ".sha256", ".jsonl", ".out", ".ls"}


def small_files(src: Path, dst: Path, limit=4 << 20):
    dst.mkdir(parents=True, exist_ok=True)
    for path in sorted(src.iterdir()):
        if path.is_file() and path.suffix in TEXT and path.stat().st_size <= limit:
            shutil.copy2(path, dst / path.name)


lanes = DEST / "lanes"
lanes.mkdir(parents=True, exist_ok=True)
for line in (W / "lanes/summary.jsonl").read_text().splitlines():
    record = json.loads(line)
    evidence = Path(record["work"]) / "evidence"
    if evidence.is_dir():
        small_files(evidence, lanes / record["lane"])
    shutil.copy2(W / "lanes" / f"{record['lane']}.log", lanes / f"{record['lane']}.log")
for name in ("summary.jsonl", "l4-streak.txt", "l0-runs.txt", "l0-streak.txt",
             "stream_a.out", "stream_b.out", "stream_c.out", "stream_d.out"):
    if (W / "lanes" / name).exists():
        shutil.copy2(W / "lanes" / name, lanes / name)
for side in ("before", "after"):
    small_files(W / "l6" / side, DEST / "l6" / side)
    boot = W / "l6" / side / "qemu-x86_64-test"
    if (boot / "run.log").exists():
        shutil.copy2(boot / "run.log", DEST / "l6" / side / "qemu-x86_64-test.run.log")
for name in ("l6-classification.json", "classify.log", "before.log", "after.log"):
    if (W / "l6" / name).exists():
        shutil.copy2(W / "l6" / name, DEST / "l6" / name)
small_files(W / "audit", DEST / "audit")
cand = DEST / "candidate"
small_files(W / "logs", cand)
shutil.copy2(W / "cand/candidate.json", cand / "candidate.json")
for profile in ("headless", "desktop"):
    shutil.copy2(W / "cand" / profile / "manifest.json", cand / f"manifest-{profile}.json")
small_files(Path("/tmp/cellbox-s10-3-1EGP/canonical"), DEST / "canonical")
scripts = DEST / "scripts"
scripts.mkdir(exist_ok=True)
for path in ("build-candidate.sh", "candidate_json.py", "archive.py", "lanes/run_lane.py", "lanes/stream_a.sh",
             "lanes/stream_b.sh", "lanes/stream_c.sh", "lanes/stream_d.sh", "l6/run_l6.py", "l6/classify_l6.py",
             "audit/run_audit.py"):
    shutil.copy2(W / path, scripts / Path(path).name)
print(sum(1 for _ in DEST.rglob("*") if _.is_file()), "files archived to", DEST)
