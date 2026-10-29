#!/usr/bin/env python3
"""S10-5 P3: compare-and-swap copy of the qualified DESKTOP candidate bytes.

Order: init, kernel_fs.img, boot-config/, manifest.json (last), then the kernel ELF.
Every source is the durable candidate copy and is verified against candidate.json before
and after; every destination must hold a recorded pre-promotion hash (or already the
promoted one, so a rerun is idempotent). Nothing is deleted: the old boot-config goes to
the scratch directory. Writes <evidence>/promotion.json. Any mismatch: stop, no retry.
"""
import hashlib, json, os, shutil, sys, datetime
from pathlib import Path

REPO = Path("/home/dmin/cellos")
EVID = REPO / ".agents/261020-1750-cellbox-cli/reports/s10-5-evidence"
DUR = Path.home() / ".cache/cellos-candidates/4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821"
SCRATCH = Path("/tmp/cellbox-s10-5-scratch")
EMB = REPO / "kernel/src/embedded-x86_64"
KERNEL = REPO / "target/x86_64-unknown-none/release/cellos-kernel"
OLD = {
    "init": "0c2f607bb29e090b31c7820290aafc1974c818c0610d216f00b5f88a10104104",
    "kernel_fs.img": "447d0348735a25e8b396c11c0091ff3d214b0c0b392043bb0515d4104a298e2c",
    "kernel": "12cb0bac7849d425b2bacbb5ed503229328c525d71744fcf68d20d5c8a61ac67",
    "boot-config/autoload.toml": "07849a42c41e05b98c6058e96b2715a8e53044ee6a6bb451a059e035a39e150b",
    "boot-config/services.toml": "5c97bad0e8fd22d17312243481a8c16774cb14bf2322086fc6d75f3bbe429cb9",
    "boot-config/system.toml": "74ecea5cd57faacfa1e32d1b006870eb3a85dea5e0b4f9e95753a57ceb147293",
}
CAND_SHA = "4fae8b20ba283b38688028b14008eb0abcf4104ca929a5a80be354675ad32821"
WANT = {"init": "c0bd14580fda0d697a2edd66b19168107fa6877a190bc06c1a8a482b467b23f3",
        "kernel_fs.img": "3848cefb25f0e7c4abc67feccf3d8886292e25da60a88b4633552651292479ec",
        "kernel": "dfbe8c555f54cf15293a8a59eead841d5c417b2faadb2e422ca97aa19744dc1a",
        "manifest.json": "af55e60245832b348e1e20b7689d35d84fe6747b7b359e886ebe3a4a81ae4e8d"}


def sha(p: Path) -> str:
    with p.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def die(msg):
    print("STOP:", msg)
    sys.exit(1)


def put(src: Path, dst: Path, want: str, mode: int):
    """Copy to a temp name in dst's directory, verify, then os.replace."""
    if sha(src) != want:
        die(f"source hash {src}")
    tmp = dst.with_name(".promote-" + dst.name + ".tmp")
    shutil.copyfile(src, tmp)
    os.chmod(tmp, mode)
    with open(tmp, "rb") as f:
        os.fsync(f.fileno())
    if sha(tmp) != want:
        tmp.unlink()
        die(f"temp hash {dst}")
    os.replace(tmp, dst)
    if sha(dst) != want:
        die(f"destination hash after replace {dst}")


def main():
    cj = json.loads((DUR / "candidate.json").read_text())
    if sha(DUR / "candidate.json") != CAND_SHA:
        die("candidate.json")
    d = cj["profiles"]["desktop"]
    cand = {"init": d["init"]["sha256"], "kernel_fs.img": d["kernel_fs_img"]["sha256"],
            "kernel": d["kernel"]["sha256"], "manifest.json": d["manifest_sha256"]}
    if cand != WANT:
        die(f"candidate.json disagrees with the approved hashes: {cand}")
    src = {"init": DUR / "desktop/init", "kernel_fs.img": DUR / "desktop/kernel_fs.img",
           "kernel": DUR / "desktop.kernel.elf", "manifest.json": DUR / "desktop/manifest.json"}
    cfg_src = {p.name: p for p in sorted((DUR / "desktop/boot-config").iterdir())}
    if set(cfg_src) != {"autoload.toml", "services.toml", "system.toml"}:
        die(f"unexpected boot-config files {sorted(cfg_src)}")
    for k, p in src.items():
        if sha(p) != WANT[k]:
            die(f"source {k}")

    # compare-and-swap preconditions
    cur = {"init": EMB / "init", "kernel_fs.img": EMB / "kernel_fs.img", "kernel": KERNEL}
    for k, p in cur.items():
        h = sha(p)
        if h not in (OLD[k], WANT[k]):
            die(f"{p} holds an unexpected hash {h}")
    for name in cfg_src:
        p = EMB / "boot-config" / name
        if not p.is_file() and not (EMB / "boot-config").is_dir():
            die(f"{p} missing")
    for name, p in cfg_src.items():
        cur_p = EMB / "boot-config" / name
        if cur_p.is_file() and sha(cur_p) not in (OLD["boot-config/" + name], sha(p)):
            die(f"{cur_p} holds an unexpected hash")
    mf = EMB / "manifest.json"
    if mf.exists() and sha(mf) != WANT["manifest.json"]:
        die("manifest.json exists with unexpected content")

    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    put(src["init"], EMB / "init", WANT["init"], 0o755)                 # (1)
    put(src["kernel_fs.img"], EMB / "kernel_fs.img", WANT["kernel_fs.img"], 0o644)  # (2)
    # (3) boot-config/: build the new directory, move the old one aside (nothing deleted)
    newcfg = EMB / ".promote-boot-config"
    if newcfg.exists():
        die("stale .promote-boot-config")
    newcfg.mkdir()
    for name, p in cfg_src.items():
        shutil.copyfile(p, newcfg / name)
        os.chmod(newcfg / name, 0o644)
        if sha(newcfg / name) != sha(p):
            die(f"boot-config temp {name}")
    old = EMB / "boot-config"
    aside = SCRATCH / "old-boot-config"
    if old.exists():
        if aside.exists():
            die("scratch old-boot-config already exists")
        shutil.move(str(old), str(aside))
    os.replace(newcfg, old)
    put(src["manifest.json"], mf, WANT["manifest.json"], 0o644)         # (4) manifest last
    put(src["kernel"], KERNEL, WANT["kernel"], 0o755)                   # (5)

    out = {"source_candidate_json_sha256": CAND_SHA, "source": str(DUR), "started_utc": started,
           "finished_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
           "promoted": {}, "old_boot_config_moved_to": str(aside)}
    for k, p in (("kernel/src/embedded-x86_64/init", EMB / "init"),
                 ("kernel/src/embedded-x86_64/kernel_fs.img", EMB / "kernel_fs.img"),
                 ("kernel/src/embedded-x86_64/manifest.json", mf),
                 ("target/x86_64-unknown-none/release/cellos-kernel", KERNEL),
                 *[(f"kernel/src/embedded-x86_64/boot-config/{n}", EMB / "boot-config" / n) for n in cfg_src]):
        out["promoted"][k] = {"sha256": sha(p), "bytes": p.stat().st_size}
    (EVID / "promotion.json").write_text(json.dumps(out, indent=2, sort_keys=True) + "\n")
    print("P3 OK")


if __name__ == "__main__":
    main()
