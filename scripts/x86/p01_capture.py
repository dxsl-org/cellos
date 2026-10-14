#!/usr/bin/env python3
"""Offline, finite private P01 image/capture lane; never writes production inputs.

Build: python3 scripts/x86/p01_capture.py build --out build/p01-private
Run:   python3 scripts/x86/p01_capture.py capture --build build/p01-private --out build/p01-captures
Parse: python3 scripts/x86/p01_capture.py parse --capture build/p01-captures/<UUID>
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

from p01_trace import parse_capture, parse_g1_capture, G1_EXPERIMENT, validate_g1_review

REPO = Path(__file__).resolve().parents[2]
COUNTS = ("0", "1", "4", "8", "denied")
G1_COUNTS = ("g1-overlap", "g1-yield", "g1-retention")


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def save(path, value):
    with Path(path).open("x") as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write("\n")


def seal(root):
    for path in root.rglob("*"):
        if path.is_file():
            path.chmod(0o400)
    for path in sorted((p for p in root.rglob("*") if p.is_dir()), reverse=True):
        path.chmod(0o500)
    root.chmod(0o500)


def new_root(path):
    path = Path(path).resolve()
    path.mkdir(mode=0o700, parents=True, exist_ok=False)
    return path


def source_inputs():
    command = ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"]
    names = subprocess.check_output(command, cwd=REPO).split(b"\0")
    # Complete local build-source trees + root toolchain/lock/config descriptors.
    roots = ("kernel/", "libs/", "cells/", "scripts/", "tools/", "targets/", ".cargo/", "boards/", "hal/", "third_party/", "limine/")
    selected = sorted({os.fsdecode(name) for name in names if name and
                       (os.fsdecode(name).startswith(roots) or os.fsdecode(name) in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"))})
    return {name: digest(REPO / name) for name in selected if (REPO / name).is_file()}


def record_environment(environment):
    """Keep build/runtime settings, never an unfiltered credential-bearing env."""
    names = {"PATH", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "TMPDIR", "CC",
             "CFLAGS", "BINDGEN_ARGS", "EMBEDDED_OVERRIDE", "CELLOS_P01_TRACE",
             "CELLOS_P01_FAULT", "CELLOS_G1_CPU", "X86_KERNEL", "X86_ISO_ROOT", "RUSTFLAGS"}
    prefixes = ("CARGO_", "RUST_", "RUSTUP_", "CC_", "CFLAGS_", "BINDGEN_")
    sensitive = ("TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "AUTH", "API_KEY")
    return {key: value for key, value in environment.items()
            if (key in names or key.startswith(prefixes))
            and not any(word in key.upper() for word in sensitive)}


def build(args):
    root = new_root(args.out)
    commands = []
    files = {}
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(root / "target"),
               CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic",
               CC_x86_64_unknown_none="cc",
               CFLAGS_x86_64_unknown_none="-ffreestanding -fno-stack-protector -mno-red-zone -mno-sse -mno-mmx -DLFS_NO_INTRINSICS -I" + str(REPO / "third_party/freestanding-include"),
               BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none="--target=x86_64-linux-gnu")
    # Environment provenance includes overrides which could otherwise contaminate Cargo.
    for key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "EMBEDDED_OVERRIDE", "CELLOS_P01_TRACE", "CELLOS_P01_FAULT", "CELLOS_G1_CPU"):
        env.pop(key, None)
    env["CELLOS_P01_TRACE"] = "0"
    def run(argv, extra=None):
        callenv = dict(env, **(extra or {}))
        number = len(commands)
        row = {"argv": argv, "cwd": str(REPO), "environment": record_environment(callenv),
               "environment_scope": "operational allowlist; not exhaustive external-input provenance"}
        commands.append(row)
        with (root / f"command-{number:03}.raw.log").open("xb") as log:
            result = subprocess.run(argv, cwd=REPO, env=callenv, stdout=log, stderr=subprocess.STDOUT)
        row["returncode"] = result.returncode
        if result.returncode:
            raise RuntimeError(f"command {number} failed ({result.returncode}); raw output retained")
    common = ["cargo", "build", "--locked", "--offline", "--release", "--target", "x86_64-unknown-none", "-Z", "build-std=core,alloc"]
    try:
        inputs_before = source_inputs()
        save(root / "inputs.json", inputs_before)
        run(common + ["-p", "app-init"])
        run(common + ["-p", "service-vfs", "-p", "app-bench", "--bin", "service-vfs", "--bin", "bench", "--bin", "bench-probe"] + (["--features", "app-bench/g1-cpu-workload"] if args.g1 else []))
        release = root / "target/x86_64-unknown-none/release"
        artifacts = [release / name for name in ("app-init", "service-vfs", "bench", "bench-probe")]
        run([sys.executable, "scripts/cellos-sign", "--quiet", "--objcopy", "objcopy", "--sign"] + [str(p) for p in artifacts])
        signed = {p.name: digest(p) for p in artifacts}
        for count in args.counts:
            scenario = root / count
            override = scenario / "override"
            override.mkdir(parents=True)
            shutil.copyfile(release / "app-init", override / "init")
            (override / "services.toml").write_text("version = 1\n")
            role = "cpu-accounting:" + count[3:] if args.g1 else "parked-footprint:" + count
            (override / "autoload.toml").write_text('version = 1\n\n[[cells]]\nname = "bench"\npath = "/bin/bench"\nargs = ["' + role + '"]\nenabled = true\nrequired = true\nrestart = "never"\n')
            (override / "p01_manifest.bin").write_bytes(b"".join(bytes.fromhex(signed[n]) for n in ("app-init", "bench", "bench-probe")))
            run([sys.executable, "scripts/sign-policy.py", "--out", str(override / "POLICY.BIN")])
            pairs = [str(release / "service-vfs"), "/bin/vfs", str(release / "bench"), "/bin/bench", str(release / "bench-probe"), "/bin/bench-probe"]
            for name, target in (("services.toml", "/etc/cellos/services.toml"), ("autoload.toml", "/etc/cellos/autoload.toml"), ("POLICY.BIN", "/POLICY.BIN")):
                pairs.extend([str(override / name), target])
            run([sys.executable, "tools/mkfat32.py", str(override / "kernel_fs.img")] + pairs)
            # Read back actual packaged bytes, not a source-only packaging assertion.
            for source, target in zip(pairs[::2], pairs[1::2]):
                returned = subprocess.run(["mtype", "-i", str(override / "kernel_fs.img"), "::" + target], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
                commands.append({"argv": ["mtype", "-i", str(override / "kernel_fs.img"), "::" + target], "returncode": returned.returncode, "readback_sha256": hashlib.sha256(returned.stdout).hexdigest()})
                if returned.stdout != Path(source).read_bytes():
                    raise RuntimeError("packaged byte mismatch: " + target)
            private_env = {"CELLOS_P01_TRACE": "1", "EMBEDDED_OVERRIDE": str(override), "RUSTFLAGS": "-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static"}
            if args.g1:
                private_env["CELLOS_G1_CPU"] = "1"
            if args.fault:
                private_env["CELLOS_P01_FAULT"] = args.fault
            run(common + ["-p", "cellos-kernel", "--features", "test-hooks"], private_env)
            shutil.copyfile(release / "cellos-kernel", scenario / "kernel.elf")
            run(["bash", "scripts/x86/make-iso-ci.sh", str(scenario / "image.iso")], {"X86_KERNEL": str(scenario / "kernel.elf"), "X86_ISO_ROOT": str(scenario / "iso-root")})
            files[count] = {str(p.relative_to(root)): digest(p) for p in [scenario / "image.iso", scenario / "kernel.elf", *override.iterdir()] if p.is_file()}
        if source_inputs() != inputs_before:
            raise RuntimeError("build source changed during packaging; artifacts are unbound")
        authority = REPO / ".agents/261011-1200-cell-contract-authority"
        pins = {name: digest(authority / name) for name in ("g1-local-accounting-candidate.md", "scoped-g1-g2-authorization.json", "g1-g2-scoped-review.md")} if args.g1 else {}
        save(root / "build.json", {"version": 1, "scope": "private-G1-Intel-QEMU-development" if args.g1 else "private-P01-no-architecture-approval", "signed": signed, "scenarios": files, "commands": commands, "inputs_sha256": digest(root / "inputs.json"), "fault": args.fault, "g1": args.g1, "g1_experiment": G1_EXPERIMENT if args.g1 else None, "authority_pins": pins})
    except BaseException as error:
        save(root / "build-failure.json", {"error": str(error), "commands": commands, "scenarios": files})
        seal(root)
        raise
    seal(root)
    print(root / "build.json")


def capture(args):
    buildroot = Path(args.build).resolve()
    manifest = json.loads((buildroot / "build.json").read_text())
    if bool(manifest.get("g1")) != args.g1:
        raise RuntimeError("private lane/build mismatch")
    decoder = parse_g1_capture if args.g1 else parse_capture
    if args.g1:
        review = json.loads(Path(args.g1_review).read_text()) if args.g1_review else {}
        validate_g1_review(review, digest(buildroot / "build.json"), manifest["inputs_sha256"])
        if digest(buildroot / "inputs.json") != manifest["inputs_sha256"]:
            raise RuntimeError("G1 immutable source receipt changed")
        inputs = json.loads((buildroot / "inputs.json").read_text())
        for name in ("scripts/x86/p01_capture.py", "scripts/x86/p01_trace.py"):
            if inputs.get(name) != digest(REPO / name):
                raise RuntimeError("G1 producer differs from reviewed build: " + name)
    qemu_path = shutil.which(args.qemu)
    if qemu_path is None:
        raise RuntimeError("QEMU binary unavailable: " + args.qemu)
    qemu_path = str(Path(qemu_path).resolve())
    qemu_version = subprocess.check_output([qemu_path, "--version"], stderr=subprocess.STDOUT).decode()
    if args.cpus == 2 and not args.g1:
        qualification = json.loads(Path(args.one_cpu_evidence).read_text()) if args.one_cpu_evidence else {}
        if not qualification.get("one_cpu_accounting_qualified") or qualification.get("build_sha256") != digest(buildroot / "build.json"):
            raise RuntimeError("twoCPU requires qualified same-build oneCPU matrix evidence")
        matrix_root = Path(args.one_cpu_evidence).resolve().parent
        seen = set()
        qualified_counts = {count: set() for count in ("0", "1", "4", "8")}
        for row in qualification.get("captures", []):
            identity = str(uuid.UUID(row["capture_uuid"]))
            if identity in seen:
                raise RuntimeError("reused captureUUID in oneCPU matrix")
            seen.add(identity)
            prior = matrix_root / identity
            meta = json.loads((prior / "capture.json").read_text())
            if meta["cpus"] != 1 or meta["build_sha256"] != digest(buildroot / "build.json"):
                raise RuntimeError("oneCPU image/config mismatch")
            if row["count"] in qualified_counts:
                if meta["count"] != row["count"] or meta["replica"] != row["replica"] or not parse_capture(prior)["fully_settled"]:
                    raise RuntimeError("oneCPU accounting does not qualify")
                qualified_counts[row["count"]].add(row["replica"])
        if any(replicas != {0, 1, 2} for replicas in qualified_counts.values()):
            raise RuntimeError("oneCPU requires3freshboots per0/1/4/8")
    if args.cpus == 2 and args.g1:
        qualification = json.loads(Path(args.one_cpu_evidence).read_text()) if args.one_cpu_evidence else {}
        if qualification.get("one_cpu_g1_qualified") is not True or qualification.get("build_sha256") != digest(buildroot / "build.json"):
            raise RuntimeError("twoCPU G1 requires separate same-build oneCPU G1 evidence")
        matrix_root = Path(args.one_cpu_evidence).resolve().parent
        qualified = set()
        seen = set()
        for row in qualification.get("captures", []):
            identity = str(uuid.UUID(row["capture_uuid"]))
            if identity in seen:
                raise RuntimeError("duplicate G1 oneCPU capture")
            seen.add(identity)
            prior = matrix_root / identity
            meta = json.loads((prior / "capture.json").read_text())
            if meta["cpus"] != 1 or meta["build_sha256"] != digest(buildroot / "build.json") or not parse_g1_capture(prior)["g1_accounting_valid"]:
                raise RuntimeError("G1 oneCPU evidence does not qualify")
            qualified.add((meta["count"], meta["replica"]))
        if qualified != {(count, replica) for count in G1_COUNTS for replica in range(3)}:
            raise RuntimeError("G1 oneCPU requires three fresh boots per finite mode")
    out = new_root(args.out)
    rows = []
    producer_paths = (Path(__file__), REPO / "scripts/x86/p01_trace.py")
    producer_hashes = tuple(digest(path) for path in producer_paths)
    for count in args.counts:
        for replica in range(3):
            if tuple(digest(path) for path in producer_paths) != producer_hashes:
                raise RuntimeError("capture producer source changed; do not mix parser/runner versions")
            identity = str(uuid.uuid4())
            directory = out / identity
            directory.mkdir()
            image = buildroot / count / "image.iso"
            expected = manifest["scenarios"][count]
            for name, sha in expected.items():
                if digest(buildroot / name) != sha:
                    raise RuntimeError("immutable build changed: " + name)
            cpu = "qemu64,vendor=GenuineIntel,+pdpe1gb" if args.g1 else "qemu64,+pdpe1gb"
            command = [qemu_path, "-machine", "q35", "-accel", "tcg", "-cpu", cpu, "-smp", str(args.cpus), "-m", "256M", "-nographic", "-cdrom", str(image), "-boot", "d", "-no-reboot", "-snapshot", "-nic", "none", "-monitor", "none"]
            if args.intel_iommu:
                command.extend(["-device", "intel-iommu,intremap=on"])
            if args.g1:
                command.extend(["-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"])
            metadata = {"version": 1, "capture_uuid": identity, "count": count, "replica": replica, "cpus": args.cpus, "command": command, "cwd": str(REPO), "host_bound_seconds": 150, "build_sha256": digest(buildroot / "build.json"), "artifact_sha256": expected, "manifest_hex": "".join(manifest["signed"][n] for n in ("app-init", "bench", "bench-probe")), "freshness": "host-assigned; not kernel-attested", "environment": record_environment(os.environ), "environment_scope": "operational allowlist; not exhaustive external-input provenance",
                        "runner_sha256": producer_hashes[0], "parser_sha256": producer_hashes[1],
                        "image_path": str(image), "image_sha256": digest(image),
                        "qemu_binary_path": qemu_path}
            metadata["qemu_binary_sha256"] = digest(qemu_path)
            metadata["qemu_version"] = qemu_version
            if args.g1:
                metadata.update(g1=True, g1_experiment=G1_EXPERIMENT, review_sha256=digest(args.g1_review))
            save(directory / "capture.json", metadata)
            shutil.copyfile(buildroot / "build.json", directory / "build.json")
            if args.g1:
                shutil.copyfile(buildroot / "inputs.json", directory / "inputs.json")
            shutil.copyfile(buildroot / count / "override/autoload.toml", directory / "autoload.toml")
            shutil.copyfile(buildroot / count / "override/services.toml", directory / "services.toml")
            if args.g1:
                shutil.copyfile(args.g1_review, directory / "g1-review.json")
            start = time.monotonic()
            with (directory / "serial.raw.log").open("xb") as raw:
                vm = subprocess.Popen(command, cwd=REPO, stdin=subprocess.DEVNULL, stdout=raw, stderr=subprocess.STDOUT)
                termination = "natural"
                try:
                    # Leave shutdown/reaping room inside the150s total host bound.
                    vm.wait(timeout=145)
                except subprocess.TimeoutExpired:
                    termination = "host-bound-terminate"
                    vm.terminate()
                    try:
                        vm.wait(timeout=4)
                    except subprocess.TimeoutExpired:
                        termination = "host-bound-kill"
                        vm.kill()
                        vm.wait(timeout=1)
            save(directory / "termination.json", {"returncode": vm.returncode, "elapsed_seconds": time.monotonic() - start, "termination": termination, "raw_sha256": digest(directory / "serial.raw.log"), "capture_sha256": digest(directory / "capture.json"), "retained_through_vm_termination": True})
            evidence = decoder(directory)
            if tuple(digest(path) for path in producer_paths) != producer_hashes:
                evidence["structurally_complete"] = evidence["fully_settled"] = False
                if args.g1:
                    evidence["g1_accounting_valid"] = False
                evidence["qualification_blockers"].append("capture producer source changed during VM observation")
            save(directory / "evidence.json", evidence)
            seal(directory)
            if args.g1:
                rows.append({"capture_uuid": identity, "count": count, "replica": replica, "g1_accounting_valid": evidence["g1_accounting_valid"]})
                continue
            rows.append({"capture_uuid": identity, "count": count, "replica": replica, "fully_settled": evidence["fully_settled"], "structurally_complete": evidence["structurally_complete"], "denied_control_accepted": evidence.get("denied_control_accepted", False)})
    if args.g1:
        qualified = args.cpus == 1 and set(args.counts) == set(G1_COUNTS) and all(row["g1_accounting_valid"] for row in rows)
        save(out / "matrix.json", {"build_sha256": digest(buildroot / "build.json"), "cpus": args.cpus, "captures": rows, "one_cpu_g1_qualified": qualified, "production_qualified": False, "hardware_D_W_qualified": False})
    else:
        qualified = args.cpus == 1 and set(args.counts) >= {"0", "1", "4", "8"} and all(row["fully_settled"] for row in rows if row["count"] != "denied")
        save(out / "matrix.json", {"build_sha256": digest(buildroot / "build.json"), "cpus": args.cpus, "captures": rows, "one_cpu_accounting_qualified": qualified, "throughput_claim": False, "architecture_approval": False})
    seal(out)
    print(out / "matrix.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    b = sub.add_parser("build")
    b.add_argument("--out", required=True)
    b.add_argument("--counts", nargs="+", choices=COUNTS, default=list(COUNTS))
    b.add_argument("--fault", choices=("contention", "full", "unready", "late"))
    b.add_argument("--g1", action="store_true")
    b.add_argument("--g1-modes", nargs="+", choices=G1_COUNTS, default=list(G1_COUNTS))
    c = sub.add_parser("capture")
    c.add_argument("--build", required=True)
    c.add_argument("--out", required=True)
    c.add_argument("--counts", nargs="+", choices=COUNTS, default=list(COUNTS))
    c.add_argument("--cpus", type=int, choices=(1, 2), default=1)
    c.add_argument("--one-cpu-evidence")
    c.add_argument("--qemu", default="qemu-system-x86_64")
    c.add_argument("--intel-iommu", action="store_true", help="Capture real q35 VT-d root/context software state; does not certify DMA authority")
    c.add_argument("--g1", action="store_true")
    c.add_argument("--g1-modes", nargs="+", choices=G1_COUNTS, default=list(G1_COUNTS))
    c.add_argument("--g1-review")
    p = sub.add_parser("parse")
    p.add_argument("--capture", required=True)
    p.add_argument("--g1", action="store_true")
    args = parser.parse_args()
    if args.action in ("build", "capture") and args.g1:
        args.counts = args.g1_modes
        if args.action == "build" and args.fault:
            parser.error("G1 observer fault contamination is forbidden")
    if hasattr(args, "counts") and len(set(args.counts)) != len(args.counts):
        parser.error("duplicate scenario selection")
    if args.action == "build": build(args)
    elif args.action == "capture": capture(args)
    else: print(json.dumps((parse_g1_capture if args.g1 else parse_capture)(Path(args.capture)), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
