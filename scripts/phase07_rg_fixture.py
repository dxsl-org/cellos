#!/usr/bin/env python3
"""Phase 07 S7-4 `rg` fixture disk (phase-07-s4-proposal.md §2.1, D75-1, D77-1).

Writes a raw private disk image whose FAT32 P1 volume (`/mnt/sd` in the guest,
`PART_FAT32_BASE_LBA` = 2048, 524,288 sectors) holds the search tree under `/rg`,
plus `expected.json` with the values the terminal driver asserts. The image is a
throw-away fixture for one private QEMU run: never a user disk.

Usage: phase07_rg_fixture.py DISK_IMG EXPECTED_JSON CONFIG_DIR

CONFIG_DIR is the lane's generated boot configuration (`--config-output-dir` of
mkfat32.py). P1 is the config override volume: once it mounts, init reads
/etc/cellos/*.toml from it, so the fixture carries the image's own configuration.

Tree (every value below is fixed; the driver's `rg_fixture` constants mirror it):
  big.log            7 MiB of 64-byte lines; NEEDLE lines at 1, 57,345 and the last
                     line (no final LF). Larger than cellbox's 6 MiB heap arena (D77-7).
  wide/              120 files `wideNNN` + 32 x + `.txt` ("wide-hit"): 45 B per listing
                     entry, so the directory spans two 4,000-byte pages. Fixture names
                     avoid `-`/`:`, which the text renderer escapes (`\x2D`/`\x3A`).
  ign/               .ignore over .gitignore, deeper .ignore over its parent (IGNTOK).
  enc/bom.txt        offsets count the BOM bytes (no transcoding; upstream rg strips it).
  hd/                hidden file and hidden directory (HIDTOK lines).
  bin/latenul.bin    BINTOK first line, ~40 KiB of text, then a NUL.
  enc/               CRLF, BOM, no final LF and invalid UTF-8 lines (ENCTOK).

P5 /srv (CellosFS, seeded by tools/cellos-fs-provision --seed): cellos/k.txt (read-denied
by the VFS access rules), pub/a.txt and zz.txt (SRVTOK lines): a denied child inside a
backed directory, with readable entries before and after it in byte order.
"""
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASE_LBA = 2048
SECTORS = 524_288
# Sparse, and as large as the whole `api::abi::disk` layout (the cell-store ends at LBA
# 1,127,680), so mounts of the other, empty partitions never read past the device.
DISK_BYTES = 1_127_680 * 512
SRV_BASE_LBA = 931_072
SRV_TREE = {
    "cellos/k.txt": b"SRVTOK denied\n",
    "pub/a.txt": b"SRVTOK a\n",
    "zz.txt": b"SRVTOK z\n",
}

LINE = 64
BIG_LINES = 7 * 1024 * 1024 // LINE
NEEDLE_LINES = (1, BIG_LINES // 2 + 1, BIG_LINES)
WIDE = 120


def wide_name(i: int) -> str:
    return f"wide{i:03d}{'x' * 32}.txt"

def big_log() -> bytes:
    """Every line is 64 bytes: 63 content bytes plus LF, except the last, which is
    64 content bytes without LF, so the offset of line n is (n - 1) * 64."""
    out = bytearray()
    for n in range(1, BIG_LINES + 1):
        head = (b"NEEDLE-%d " % n) if n in NEEDLE_LINES else (b"line %08d " % n)
        out += head.ljust(LINE, b"x") if n == BIG_LINES else head.ljust(LINE - 1, b"x") + b"\n"
    assert len(out) == BIG_LINES * LINE
    return bytes(out)


def tree() -> dict[str, bytes]:
    files = {"big.log": big_log()}
    for i in range(WIDE):
        files[f"wide/{wide_name(i)}"] = b"wide-hit\n"
    files.update({
        "ign/.gitignore": b"keep.txt\ndrop.txt\n",
        "ign/.ignore": b"!keep.txt\nback.txt\n",
        "ign/keep.txt": b"IGNTOK keep\n",
        "ign/drop.txt": b"IGNTOK drop\n",
        "ign/sub/.ignore": b"!back.txt\n",
        "ign/sub/back.txt": b"IGNTOK back\n",
        "hd/vis.txt": b"HIDTOK vis\n",
        "hd/.dot.txt": b"HIDTOK dot\n",
        "hd/.hid/h.txt": b"HIDTOK h\n",
        "bin/latenul.bin": b"BINTOK text\n" + b"filler line of text\n" * 2100 + b"\0tail\n",
        "enc/bad.txt": b"ENCTOK \xff bad\n",
        "enc/bom.txt": b"\xef\xbb\xbffirst\nENCTOK bom\n",
        "enc/crlf.txt": b"ENCTOK crlf\r\nplain\r\n",
        "enc/nolf.txt": b"x\nENCTOK nolf",
    })
    return files


def expected() -> dict:
    p = "/mnt/sd/rg"
    return {
        "big": {
            "path": f"{p}/big.log",
            "size": BIG_LINES * LINE,
            "line_numbers": list(NEEDLE_LINES),
            "absolute_offsets": [(n - 1) * LINE for n in NEEDLE_LINES],
        },
        "wide": {"rows": [f"{p}/wide/{wide_name(i)}:1:wide-hit" for i in range(WIDE)], "pages_min": 2},
        "ign_default": [f"{p}/ign/keep.txt:1:IGNTOK keep", f"{p}/ign/sub/back.txt:1:IGNTOK back"],
        "ign_no_ignore": [
            f"{p}/ign/drop.txt:1:IGNTOK drop",
            f"{p}/ign/keep.txt:1:IGNTOK keep",
            f"{p}/ign/sub/back.txt:1:IGNTOK back",
        ],
        "hd_default": [f"{p}/hd/vis.txt:1:HIDTOK vis"],
        "hd_hidden": [
            f"{p}/hd/.dot.txt:1:HIDTOK dot",
            f"{p}/hd/.hid/h.txt:1:HIDTOK h",
            f"{p}/hd/vis.txt:1:HIDTOK vis",
        ],
        "bin_a": [f"{p}/bin/latenul.bin:1:BINTOK text"],
        "enc_text": [
            f"{p}/enc/bad.txt:1:ENCTOK \\xFF bad",
            f"{p}/enc/bom.txt:2:ENCTOK bom",
            f"{p}/enc/crlf.txt:1:ENCTOK crlf\\r",
            f"{p}/enc/nolf.txt:2:ENCTOK nolf",
        ],
        "srv_rows": ["/srv/pub/a.txt:1:SRVTOK a", "/srv/zz.txt:1:SRVTOK z"],
        "bom_offset": 9,
    }


def run(*argv: str, **kw) -> None:
    subprocess.run(argv, check=True, **kw)


def main() -> int:
    if len(sys.argv) != 4:
        print(__doc__, file=sys.stderr)
        return 2
    disk, exp, config = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
    if not (config / "services.toml").is_file():
        print(f"FAIL: {config} holds no services.toml", file=sys.stderr)
        return 1
    with open(disk, "wb") as f:
        f.truncate(DISK_BYTES)
    run(sys.executable, str(ROOT / "tools/mkfat32_inplace.py"), "--config-dir", str(config),
        str(disk), str(SECTORS), str(BASE_LBA))
    env = dict(os.environ, MTOOLS_SKIP_CHECK="1")
    target = f"{disk}@@{BASE_LBA * 512}"
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "rg"
        for rel, data in tree().items():
            path = src / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        run("mcopy", "-s", "-i", target, str(src), "::/", env=env)
        # P5 /srv (CellosFS, offline provisioner): a backed directory whose `cellos` child
        # is read-denied to every ordinary Cell, between two readable siblings.
        srv = Path(tmp) / "srv"
        for rel, data in SRV_TREE.items():
            path = srv / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        tool = ROOT / "tools/cellos-fs-provision"
        host = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
        target_triple = next(l.split(": ", 1)[1] for l in host.splitlines() if l.startswith("host: "))
        build_env = {k: v for k, v in os.environ.items() if k not in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")}
        run("cargo", "run", "--quiet", "--locked", "--offline", "--manifest-path", str(tool / "Cargo.toml"),
            "--target", target_triple, "--", "--image", str(disk), "--base", str(SRV_BASE_LBA),
            "--sectors", "131072", "--intent", "fresh-blank", "--receipt", str(exp.with_name("srv-receipt.json")),
            "--seed", str(srv), env=build_env)
    # Read the tree back through mtools so a silently dropped or renamed entry fails here.
    listing = subprocess.run(["mdir", "-/", "-b", "-i", target, "::/rg"], check=True,
                             capture_output=True, text=True, env=env).stdout.split()
    want = {f"::/rg/{rel}" for rel in tree()}
    missing = sorted(want - set(listing))
    if missing:
        print(f"FAIL: fixture misses {missing[:5]} ({len(missing)})", file=sys.stderr)
        return 1
    exp.write_text(json.dumps(expected(), indent=1) + "\n")
    print(f"rg fixture: {disk} ({len(want)} files), expectations {exp}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
