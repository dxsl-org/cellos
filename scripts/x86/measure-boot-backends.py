#!/usr/bin/env python3
"""Measure prompt arrival while retaining the existing production boot gate."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import statistics
import signal
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts/qemu-x86_64-test.sh"


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("iso", nargs="?", type=Path, default=ROOT / "build/vicell-x86.iso")
    parser.add_argument("--runs", type=positive, default=3)
    parser.add_argument("--window", type=positive, default=90,
                        help="full regression observation window per boot, in seconds")
    parser.add_argument("--backend", choices=("both", "tcg", "kvm"), default="both")
    parser.add_argument("--tcg-cpu", default="qemu64,+pdpe1gb")
    parser.add_argument("--kvm-cpu", default="host")
    parser.add_argument("--evidence-root", type=Path, default=ROOT / "build/x86-backend-evidence")
    args = parser.parse_args()
    iso = args.iso.resolve()
    if not iso.is_file():
        parser.error(f"ISO not found: {iso}")
    if not shutil.which("qemu-system-x86_64") or not shutil.which("bash"):
        parser.error("qemu-system-x86_64 and bash are required")
    backends = ("tcg", "kvm") if args.backend == "both" else (args.backend,)
    if "kvm" in backends and not os.access("/dev/kvm", os.R_OK | os.W_OK):
        parser.error("KVM requires readable/writable /dev/kvm; no TCG fallback")
    args.evidence_root.mkdir(parents=True, exist_ok=True)
    evidence = Path(tempfile.mkdtemp(prefix="boot-", dir=args.evidence_root.resolve()))
    report = {
        "scope": "production x86 boot compatibility; not workload or physical qualification",
        "metric": "host monotonic seconds from runner launch to first serial shell prompt; includes firmware and startup",
        "poll_interval_seconds": 0.01,
        "iso": str(iso), "iso_sha256": digest(iso),
        "runner_sha256": digest(RUNNER),
        "qemu_version": subprocess.check_output(["qemu-system-x86_64", "--version"], text=True).strip(),
        "host": list(os.uname()), "window_seconds": args.window,
        "runs": [],
    }
    print(f"Evidence: {evidence}", flush=True)
    # Alternate backends to reduce ordering bias. Each run retains the entire
    # observation window: a later panic must invalidate an earlier prompt.
    for iteration in range(1, args.runs + 1):
        for backend in backends:
            folder = evidence / f"{iteration}-{backend}"
            folder.mkdir()
            cpu = args.kvm_cpu if backend == "kvm" else args.tcg_cpu
            environment = os.environ.copy()
            for key in ("X86_NIC_MODEL", "X86_SATA_IMAGE", "X86_EXPECT_PCID"):
                environment.pop(key, None)
            environment.update(X86_ACCEL=backend, X86_CPU_MODEL=cpu, BOOT_WINDOW=str(args.window))
            prompt_seconds = None
            started = time.monotonic()
            with (folder / "gate.log").open("wb") as output:
                process = subprocess.Popen(["bash", str(RUNNER), str(iso)], cwd=folder,
                                           env=environment, stdout=output, stderr=subprocess.STDOUT,
                                           start_new_session=True)
                try:
                    while True:
                        serial = folder / "qemu-x86_64.raw.log"
                        if prompt_seconds is None and serial.exists():
                            if b"Cellos >" in serial.read_bytes():
                                prompt_seconds = time.monotonic() - started
                        status = process.poll()
                        if status is not None:
                            break
                        if time.monotonic() - started > args.window + 10:
                            raise TimeoutError("boot runner exceeded its observation window")
                        time.sleep(0.01)
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()
            row = {"backend": backend, "cpu": cpu, "iteration": iteration,
                   "gate_exit_code": status, "prompt_seconds": prompt_seconds,
                   "evidence_directory": str(folder)}
            report["runs"].append(row)
            (evidence / "results.json").write_text(json.dumps(report, indent=2) + "\n")
            if status != 0 or prompt_seconds is None:
                print(f"FAIL: {backend} iteration {iteration}; inspect {folder / 'gate.log'}")
                return 1
            print(f"PASS: {backend} iteration {iteration}: prompt={prompt_seconds:.3f}s", flush=True)
    report["median_prompt_seconds"] = {
        backend: statistics.median(row["prompt_seconds"] for row in report["runs"]
                                   if row["backend"] == backend)
        for backend in backends
    }
    (evidence / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    for backend, seconds in report["median_prompt_seconds"].items():
        print(f"Median {backend}: {seconds:.3f}s")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
