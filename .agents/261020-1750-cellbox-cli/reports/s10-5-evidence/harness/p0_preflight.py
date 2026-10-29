#!/usr/bin/env python3
"""S10-5 P0: read-only pre-flight, then the durable candidate copy.

Writes <evidence>/p0-preflight.json. Exits non-zero on any mismatch (a stop).
Nothing under the repository is modified; the durable copy goes to $HOME/.cache.
"""
import hashlib, json, os, shutil, subprocess, sys
from pathlib import Path

REPO = Path("/home/dmin/cellos")
EVID = REPO / ".agents/261020-1750-cellbox-cli/reports/s10-5-evidence"
SRC = Path("/tmp/cellbox-s10-3d-resume")
CAND = SRC / "cand"
DURABLE = Path.home() / ".cache/cellos-candidates/4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821"
CAND_SHA = "4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821"

EXPECT = {
    "kernel/src/embedded-x86_64/kernel_fs.img": "447d0348735a25e8b396c11c0091ff3d214b0c0b392043bb0515d4104a298e2c",
    "kernel/src/embedded-x86_64/init": "0c2f607bb29e090b31c7820290aafc1974c818c0610d216f00b5f88a10104104",
    "target/x86_64-unknown-none/release/cellos-kernel": "12cb0bac7849d425b2bacbb5ed503229328c525d71744fcf68d20d5c8a61ac67",
    "kernel/src/embedded-x86_64/boot-config/autoload.toml": "07849a42c41e05b98c6058e96b2715a8e53044ee6a6bb451a059e035a39e150b",
    "kernel/src/embedded-x86_64/boot-config/services.toml": "5c97bad0e8fd22d17312243481a8c16774cb14bf2322086fc6d75f3bbe429cb9",
    "kernel/src/embedded-x86_64/boot-config/system.toml": "74ecea5cd57faacfa1e32d1b006870eb3a85dea5e0b4f9e95753a57ceb147293",
    "kernel/src/main.rs": "a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64",
    "build/vicell-x86.iso": "eb1cb8130476120a2f25ab63f6d1ef45036d497722005ffef93e62433abfef09",
    "scripts/assemble-x86-image.py": "6ea177363aecff6b12c293403df528be28920d05f2281531fb3d2d5b5e1f5cb3",
    "scripts/cellos_sign/signing.py": "a1d25650f79383f7e09af9a95a9b4ad153bec8884f5f8913daf94fb181d1c876",
    "scripts/cellos_sign/cli.py": "ed92c59aff7694458f089e669376acec223099331547c94b2223aa320b126efa",
    "scripts/x86/make-iso-ci.sh": "0e62af27bc279610017b6f891d375e6478e149759ef00e47e86139b762c2ec9c",
    "scripts/package-desktop-x86.py": "286bef7acf3dd90f2b4ad37869168d509e8c8cc636c3eca6fedf48c8439cabcc",
}


def sha(p: Path) -> str:
    with p.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def main() -> int:
    res: dict = {"stops": []}

    def stop(msg):
        res["stops"].append(msg)
        print("STOP:", msg)

    # candidate integrity (source)
    if sha(CAND / "candidate.json") != CAND_SHA:
        stop("candidate.json hash")
    cj = json.loads((CAND / "candidate.json").read_text())
    cand_files = {}
    for prof in ("headless", "desktop"):
        p = cj["profiles"][prof]
        for key in ("kernel_fs_img", "init", "kernel", "iso"):
            cand_files[p[key]["path"]] = p[key]["sha256"]
        for path, ent in p["files"].items():
            cand_files[ent["file"]] = ent["sha256"]
    pc = cj["profiles"]["pc"]
    for key in ("kernel", "iso"):
        cand_files[pc[key]["path"]] = pc[key]["sha256"]
    bad = [p for p, h in cand_files.items() if sha(Path(p)) != h]
    res["candidate_files_checked"] = len(cand_files)
    if bad:
        stop(f"candidate file hash mismatch: {bad[:5]}")

    # 469 canonical paths
    before = json.loads((SRC / "canonical/before.json").read_text())
    changed = []
    for rel, ent in before.items():
        p = REPO / rel
        try:
            if sha(p) != ent["sha256"]:
                changed.append(rel)
        except OSError:
            changed.append(rel + " (missing)")
    res["canonical_paths"] = len(before)
    res["canonical_changed"] = changed
    if changed or len(before) != 469:
        stop(f"469-path comparison: {changed[:5]}")

    # explicit expected hashes
    res["expected"] = {}
    for rel, want in EXPECT.items():
        got = sha(REPO / rel)
        res["expected"][rel] = {"sha256": got, "ok": got == want}
        if got != want:
            stop(f"{rel}: {got} != {want}")

    # git / dirtiness
    git = lambda *a: subprocess.run(["git", *a], cwd=REPO, capture_output=True, text=True, check=True).stdout
    res["git_head"] = git("rev-parse", "HEAD").strip()
    status = git("status", "--short").splitlines()
    res["git_status_count"] = len(status)
    res["git_modified_under"] = {
        d: [l for l in status if l[3:].startswith(d)]
        for d in ("kernel/", "cells/", "scripts/", ".github/")
    }
    res["main_rs_status"] = [l for l in status if l.endswith("kernel/src/main.rs")]
    if res["git_head"] != "bfd05ec02977c722ac595f7752f601bd7ca4c92d":
        stop("git HEAD differs")

    # running processes
    ps = subprocess.run(["ps", "-eo", "pid,args"], capture_output=True, text=True).stdout.splitlines()
    busy = [l for l in ps if any(w in l for w in ("qemu-system", "cargo ", "rustc ", "pytest")) and "p0_preflight" not in l]
    res["busy_processes"] = busy
    if busy:
        stop(f"busy processes: {busy[:3]}")

    # tools
    for tool in ("xorriso", "qemu-system-x86_64"):
        res[tool] = shutil.which(tool)
        if not res[tool]:
            stop(f"{tool} missing")
    if not (REPO / "limine/limine-8.7.0/bin/BOOTX64.EFI").is_file():
        stop("limine 8.7.0 missing")

    # durable copy
    if not res["stops"]:
        if DURABLE.exists():
            stop(f"durable copy already exists: {DURABLE}")
        else:
            DURABLE.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(CAND, DURABLE, symlinks=True)
            for root, dirs, files in os.walk(DURABLE, topdown=False):
                for f in files:
                    fp = Path(root, f)
                    fp.chmod(0o555 if os.access(fp, os.X_OK) else 0o444)
                for d in dirs:
                    Path(root, d).chmod(0o555)
            DURABLE.chmod(0o500)
            # verify: byte equality and candidate.json hashes through the copy
            mism = []
            for p in CAND.rglob("*"):
                if p.is_file():
                    q = DURABLE / p.relative_to(CAND)
                    if not q.is_file() or sha(q) != sha(p):
                        mism.append(str(p))
            res["durable_copy"] = str(DURABLE)
            res["durable_mismatches"] = mism
            if mism:
                stop(f"durable copy mismatch: {mism[:3]}")
            if sha(DURABLE / "candidate.json") != CAND_SHA:
                stop("durable candidate.json hash")
            res["durable_files"] = sum(1 for p in DURABLE.rglob("*") if p.is_file())
            res["durable_bytes"] = sum(p.stat().st_size for p in DURABLE.rglob("*") if p.is_file())

    EVID.mkdir(parents=True, exist_ok=True)
    (EVID / "p0-preflight.json").write_text(json.dumps(res, indent=2, sort_keys=True) + "\n")
    print("P0", "FAIL" if res["stops"] else "OK")
    return 1 if res["stops"] else 0


if __name__ == "__main__":
    sys.exit(main())
