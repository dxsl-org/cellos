#!/usr/bin/env python3
"""S10-5 V7: L6 once. The 12 ISO consumers and the CI boot check on the ISO that the
reopened G3 built from the promoted default kernel (scratch, never build/); the PC ISO is
the hash-checked candidate pc.iso. Classified (by code) against the recorded S10-3.14
before-baseline (consumers on build/vicell-x86.iso `eb1cb813...`).

usage: p7_l6.py <promoted-iso> <out-dir>   (the harness is the S10-3.14 run_l6.py/classify_l6.py logic)
"""
import hashlib, json, os, re, subprocess, sys, time
from pathlib import Path

REPO = Path("/home/dmin/cellos")
DUR = Path.home() / ".cache/cellos-candidates/4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821"
BASELINE = Path("/tmp/cellbox-s10-3d-resume/l6/before/l6.json")
iso_path, out = Path(sys.argv[1]), Path(sys.argv[2])
out.mkdir(parents=True, exist_ok=True)
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
cand = json.loads((DUR / "candidate.json").read_text())
pc = DUR / "pc.iso"
if sha(pc) != cand["profiles"]["pc"]["iso"]["sha256"]:
    sys.exit("REFUSED: pc.iso differs from candidate.json")
D = str(iso_path)
H = D                       # the promoted default-kernel ISO stands in for build/vicell-x86.iso
PC = str(pc)
CONSUMERS = [
    ("x86_64-boot", {"VICELL_X86_ISO": H}, []),
    ("nvme-x86", {"VICELL_X86_ISO": H}, []),
    ("virtio-x86", {"VICELL_X86_ISO": H}, []),
    ("pcie-multibus-x86", {"VICELL_X86_ISO": H}, []),
    ("handoff", {"VICELL_X86_ISO": H}, ["handoff_x86"]),
    ("ahci-x86", {"VICELL_AHCI_ISO": H}, []),
    ("xhci-x86", {"VICELL_XHCI_ISO": H}, []),
    ("serial-x86", {"VICELL_SERIAL_ISO": H}, []),
    ("nic-x86", {"VICELL_NIC_ISO": H}, []),
    ("igb-x86", {"VICELL_IGB_ISO": H, "VICELL_IGB_VTD_ISO": H}, []),
    ("iommu-dmar-x86", {"VICELL_IOMMU_ISO": H, "VICELL_PC_ISO": PC}, []),
    ("desktop-x86", {"CELLOS_DESKTOP_ISO": D, "CELLOS_DESKTOP_ACCEL": "tcg"}, []),
]
base_env = {k: v for k, v in os.environ.items()
            if not (k.startswith("VICELL_") or k.startswith("CELLOS_") or k == "CI")}
base_env["CARGO_TARGET_DIR"] = str(out.parent / "target-l6")
record = {"phase": "promoted", "isos": {"promoted_default_kernel_iso": {"path": D, "sha256": sha(D)},
                                        "pc": {"path": PC, "sha256": sha(PC)}}, "consumers": {}}
for name, env, filt in CONSUMERS:
    start = time.time()
    command = ["cargo", "test", "--manifest-path", "tests/integration/Cargo.toml", "--test", name,
               "--target", "x86_64-unknown-linux-gnu", "--", *filt, "-Z", "unstable-options",
               "--format", "json", "--show-output", "--test-threads=1"]
    proc = subprocess.run(command, cwd=REPO, env={**base_env, **env}, capture_output=True, text=True)
    (out / f"{name}.stdout.jsonl").write_text(proc.stdout)
    (out / f"{name}.stderr.log").write_text(proc.stderr)
    skipped = set(re.findall(r"SKIPPED \([^)]*\): (\S+)", proc.stderr))
    tests = {}
    for line in proc.stdout.splitlines():
        if not line.startswith("{"):
            continue
        event = json.loads(line)
        if event.get("type") != "test" or event.get("event") == "started":
            continue
        text = event.get("stdout", "") or ""
        status = {"ok": "pass", "failed": "fail", "ignored": "ignored"}.get(event["event"], event["event"])
        if status == "pass" and (event["name"] in skipped or re.search(r"(?m)^SKIP", text)):
            status = "skip"
        tests[event["name"]] = status
    record["consumers"][name] = {"env": env, "filter": filt, "exit": proc.returncode, "tests": tests,
                                 "seconds": round(time.time() - start, 1)}
    print(f"{name}: exit {proc.returncode}, {sum(s == 'pass' for s in tests.values())} pass, "
          f"{sum(s == 'fail' for s in tests.values())} fail, {sum(s == 'skip' for s in tests.values())} skip",
          flush=True)
boot_dir = out / "qemu-x86_64-test"
boot_dir.mkdir(exist_ok=True)
proc = subprocess.run(["bash", str(REPO / "scripts/qemu-x86_64-test.sh"), H], cwd=boot_dir,
                      env={**base_env, "BOOT_WINDOW": "90"}, capture_output=True, text=True)
(boot_dir / "run.log").write_text(proc.stdout + proc.stderr)
record["consumers"]["qemu-x86_64-test.sh"] = {"exit": proc.returncode,
                                              "tests": {"boot-to-prompt": "pass" if proc.returncode == 0 else "fail"}}
print(f"qemu-x86_64-test.sh: exit {proc.returncode}")
(out / "l6.json").write_text(json.dumps(record, indent=2) + "\n")

# classification (same rules as the S10-3.14 classify_l6.py)
before = json.loads(BASELINE.read_text())
rows, regressions = [], []
for consumer, a in record["consumers"].items():
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
summary = {"baseline": str(BASELINE), "baseline_isos": before["isos"], "after_isos": record["isos"],
           "rows": rows, "counts": {v: sum(r["verdict"] == v for r in rows) for v in sorted({r["verdict"] for r in rows})},
           "regressions": regressions, "verdict": "FAIL" if regressions else "PASS"}
(out / "l6-classification.json").write_text(json.dumps(summary, indent=2) + "\n")
for r in rows:
    print(f"{r['verdict']:22} {r['consumer']}::{r['test']} ({r['before']} -> {r['after']})")
print("counts", summary["counts"], "regressions", regressions, "VERDICT", summary["verdict"])
sys.exit(1 if regressions else 0)
