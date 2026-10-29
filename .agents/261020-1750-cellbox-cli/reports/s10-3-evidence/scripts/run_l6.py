#!/usr/bin/env python3
"""S10-3 L6: run the 12 ISO consumers (+ the CI boot check) against the candidate ISOs and, read-only,
against the current build/ baselines; classify every test (produced by code, never edited).

usage: run_l6.py <candidate.json> <out-dir> before|after
"""
import hashlib, json, os, re, subprocess, sys, time
from pathlib import Path

REPO = Path("/home/dmin/cellos")
cand_path, out, phase = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
cand = json.loads(cand_path.read_text())
out.mkdir(parents=True, exist_ok=True)
sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()

if phase == "after":
    iso = {p: cand["profiles"][p]["iso"]["path"] for p in ("headless", "desktop", "pc")}
    for p, path in iso.items():
        if sha(path) != cand["profiles"][p]["iso"]["sha256"]:
            sys.exit(f"REFUSED: {p} ISO differs from candidate.json")
else:
    iso = {"headless": str(REPO / "build/vicell-x86.iso"), "desktop": str(REPO / "build/cellos-x86_64.iso"),
           "pc": str(REPO / "build/x86-pc-lane/vicell-x86-pc.iso")}

H, D, PC = iso["headless"], iso["desktop"], iso["pc"]
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
base_env["CARGO_TARGET_DIR"] = str(out.parent / f"target-{phase}")
record = {"phase": phase, "isos": {k: {"path": v, "sha256": sha(v)} for k, v in iso.items()},
          "consumers": {}}
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
# The CI boot check, from a scratch cwd (it writes qemu-x86_64.log in its cwd).
boot_dir = out / "qemu-x86_64-test"
boot_dir.mkdir(exist_ok=True)
proc = subprocess.run(["bash", str(REPO / "scripts/qemu-x86_64-test.sh"), H], cwd=boot_dir,
                      env={**base_env, "BOOT_WINDOW": "90"}, capture_output=True, text=True)
(boot_dir / "run.log").write_text(proc.stdout + proc.stderr)
record["consumers"]["qemu-x86_64-test.sh"] = {"exit": proc.returncode,
                                              "tests": {"boot-to-prompt": "pass" if proc.returncode == 0 else "fail"}}
print(f"qemu-x86_64-test.sh: exit {proc.returncode}")
(out / "l6.json").write_text(json.dumps(record, indent=2) + "\n")
