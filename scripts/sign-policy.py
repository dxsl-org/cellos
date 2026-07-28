#!/usr/bin/env python3
"""Build + Ed25519-sign a Cellos operator policy blob (roadmap §G.2 P5b).

Blob format (little-endian) — MUST match kernel/src/policy.rs::parse:
    magic u32 "VPOL" | version u8=2 | flags u8 | entry_count u16
    per entry: path_len u8, path bytes,
               block_io u8, network u8, spawn u8, hyp u8, mmio_devices u8, block_regions u8,
               pcie_driver u8, platform u8, supervisor u8      <-- v2 only
    + Ed25519 signature [u8;64] over all preceding bytes

v1 (6 cap bytes) is still accepted by the kernel parser. Do not emit it: under v1
the three privileged bytes read as 0, and because a `Permit` INTERSECTS, a v1
entry strips pcie_driver/platform/supervisor from every cell it names — so a v1
blob listing /bin/block would kill the block driver.

flags bit 0 = MAINTENANCE_PERMITTED. The kernel's `maintenance-mode` build feature
no longer bypasses policy on its own; it also needs this signed bit. Emit it only
for a deliberately-built recovery image (--maintenance).

The DEV key is derived from a fixed seed so it is reproducible (it is a *dev*
key, gated behind the kernel `dev-policy-key` feature, never shipped in release).
Production signing supplies a real private key via --seed-hex / a KMS, never this
hardcoded dev seed.

Usage:
    python scripts/sign-policy.py --emit-rust   # print dev pubkey + signed blob as Rust literals
    python scripts/sign-policy.py --out POLICY.BIN   # write the signed blob for baking into VIFS1
"""
import argparse
import struct
import sys

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives import serialization
except ImportError:
    sys.exit("error: pip install cryptography")

MAGIC = b"VPOL"
VERSION = 2
DEV_SEED = bytes([0x42] * 32)  # fixed dev seed → deterministic dev keypair

FLAG_MAINTENANCE_PERMITTED = 1 << 0

# Domain limits enforced by kernel/src/policy.rs. A value outside these makes the
# whole blob Invalid → DenyAll for every path → every cell outside the trusted
# core boots with no caps. Checked locally in build_body so that never reaches an
# image: one bad byte here bricks a fleet, and "boot to prompt" still passes
# because the shell comes up from the ramdisk.
MMIO_MASK = 0b111      # DEV_UART=1 | DEV_GPIO=2 | DEV_PCIE=4
REGION_MASK = 0b111    # P1=1 | P4=2 | SRV=4

# Operator policy: the CEILING each path may hold. It intersects the manifest
# request, so an entry can only ever take authority away — but a missing entry is
# not neutral: under the kernel's `policy-required` feature an unlisted path gets
# CapSet::EMPTY. Every cell that needs any capability must therefore appear here,
# including demos the shell launches. See the enumeration table in
# .agents/260727-2101-midori-lessons-cellos/phase-03-policy-cap-coverage.md.
#
# (path, block_io, network, spawn, hyp, mmio, regions, pcie_driver, platform, supervisor)
DEV_POLICY = [
    # ── root authority ────────────────────────────────────────────────────────
    # init is Spawner::Root and exempt in-kernel, so this entry is informational
    # today. It stays maximal on purpose: init's own caps are the CEILING for
    # every cell it spawns, so a narrow entry here would starve its children if
    # that exemption is ever removed.
    ("/bin/init",        1, 1, 1, 0, 3, 0b111, 1, 1, 1),
    # Kernel-spawned (Root, exempt); listed so the intent is reviewable.
    ("/bin/platform",    0, 0, 0, 0, 0, 0,     0, 1, 0),
    # ── core services ─────────────────────────────────────────────────────────
    ("/bin/vfs",         1, 0, 0, 0, 0, 0b111, 0, 0, 0),
    ("/bin/net",         0, 1, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/net-broker",  0, 1, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/supervisor",  0, 0, 1, 0, 0, 0,     0, 0, 1),
    ("/bin/hypervisor",  0, 0, 0, 1, 0, 0,     0, 0, 0),
    # Shell keeps gpio|uart: it is the SPAWNER of the peripheral demos, and a
    # spawner's caps are the ceiling for its children. Dropping this to 0 (as the
    # v1 policy did) silently disables every MMIO demo, not just the shell.
    ("/bin/shell",       0, 0, 1, 0, 3, 0,     0, 0, 0),
    # ── driver cells (pcie_driver comes from CapSet::with_path_caps) ───────────
    ("/bin/block",       0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/nvme",        0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/input",       0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/virtio-net",  0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/e1000",       0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/virtio-gpu",  0, 0, 0, 0, 0, 0,     1, 0, 0),
    # ── shell-launched cells that need MMIO or spawn ──────────────────────────
    ("/bin/periph-demo", 0, 0, 0, 0, 3, 0,     0, 0, 0),
    ("/bin/periph-test", 0, 0, 0, 0, 3, 0,     0, 0, 0),
    ("/bin/robot-demo",  0, 1, 0, 0, 2, 0,     0, 0, 0),
    ("/bin/sensor-demo", 0, 0, 0, 0, 2, 0,     0, 0, 0),
    ("/bin/spi-demo",    0, 0, 0, 0, 2, 0,     0, 0, 0),
    ("/bin/pwm-demo",    0, 0, 0, 0, 2, 0,     0, 0, 0),
    ("/bin/gpio-test-rv",0, 0, 0, 0, 2, 0,     0, 0, 0),
    ("/bin/bench",       0, 0, 1, 0, 0, 0,     0, 0, 0),
    ("/bin/bench-probe", 0, 0, 1, 0, 0, 0,     0, 0, 0),
]


def build_body(entries, flags=0):
    out = bytearray()
    out += MAGIC
    out += struct.pack("<BBH", VERSION, flags, len(entries))
    seen = set()
    for (path, bio, net, spawn, hyp, mmio, regions, pcie, plat, sup) in entries:
        pb = path.encode("ascii")
        if len(pb) > 255:
            sys.exit(f"path too long: {path}")
        # A duplicate path is not rejected by the kernel — lookup() returns the
        # FIRST match, so a later, tighter entry would be silently dead. Catch it
        # here rather than shipping a policy whose text disagrees with its effect.
        if path in seen:
            sys.exit(f"duplicate path: {path}")
        seen.add(path)
        if mmio & ~MMIO_MASK:
            sys.exit(f"{path}: mmio {mmio:#b} outside MMIO_MASK {MMIO_MASK:#b} — blob would be Invalid")
        if regions & ~REGION_MASK:
            sys.exit(f"{path}: block_regions {regions:#b} outside REGION_MASK {REGION_MASK:#b} — blob would be Invalid")
        for name, v in (("pcie_driver", pcie), ("platform", plat), ("supervisor", sup)):
            if v not in (0, 1):
                sys.exit(f"{path}: {name} must be 0 or 1, got {v} — blob would be Invalid")
        out.append(len(pb))
        out += pb
        out += bytes([bio, net, spawn, hyp, mmio, regions, pcie, plat, sup])
    return bytes(out)


def decode_body(body):
    """Decode a blob body using the SAME rules as kernel/src/policy.rs::parse.

    Deliberately an independent decoder rather than a mirror of build_body: it is
    the gate that must fail here, on a workstation, instead of on a fleet. A blob
    the kernel rejects becomes PolicyState::Invalid → DenyAll for every path →
    every cell outside the trusted core boots with no capabilities. That failure
    does not show up in a "boot to prompt" check, because the shell is trusted
    core and comes up regardless.

    Raises ValueError exactly where the kernel would return None.
    """
    if len(body) < 8:
        raise ValueError("body shorter than header")
    if body[:4] != MAGIC:
        raise ValueError("bad magic")
    version = body[4]
    cap_bytes = {1: 6, 2: 9}.get(version)
    if cap_bytes is None:
        raise ValueError(f"unknown version {version}")
    flags = body[5]
    if flags & ~FLAG_MAINTENANCE_PERMITTED:
        raise ValueError(f"unknown flag bits in {flags:#04x}")
    count = struct.unpack_from("<H", body, 6)[0]

    out, off = [], 8
    for i in range(count):
        if off >= len(body):
            raise ValueError(f"entry {i}: truncated at path_len")
        path_len = body[off]
        off += 1
        if off + path_len > len(body):
            raise ValueError(f"entry {i}: truncated path")
        path = body[off:off + path_len].decode("utf-8")
        off += path_len
        if off + cap_bytes > len(body):
            raise ValueError(f"entry {i} ({path}): truncated caps")
        caps = body[off:off + cap_bytes]
        off += cap_bytes
        if caps[4] & ~MMIO_MASK:
            raise ValueError(f"{path}: mmio {caps[4]:#b} out of domain")
        if caps[5] & ~REGION_MASK:
            raise ValueError(f"{path}: block_regions {caps[5]:#b} out of domain")
        if cap_bytes == 9:
            for name, v in (("pcie_driver", caps[6]), ("platform", caps[7]), ("supervisor", caps[8])):
                if v > 1:
                    raise ValueError(f"{path}: {name}={v} out of domain")
            priv = (caps[6], caps[7], caps[8])
        else:
            priv = (0, 0, 0)
        out.append((path, caps[0], caps[1], caps[2], caps[3], caps[4], caps[5], *priv))
    if off != len(body):
        raise ValueError(f"{len(body) - off} trailing bytes after {count} entries")
    return flags, out


def assert_round_trip(body, entries, flags):
    """Decode the blob we just built and require it to say what the table says.

    Runs unconditionally, not behind a flag: a gate you can forget to run is how
    qemu-boot-test.sh asserted "FAT16 mounted" for months while the images it
    checked contained no cells at all.
    """
    got_flags, got = decode_body(body)
    if got_flags != flags:
        sys.exit(f"round-trip: flags {got_flags:#04x} != {flags:#04x}")
    want = [tuple(e) for e in entries]
    if got != want:
        for i, (g, w) in enumerate(zip(got, want)):
            if g != w:
                sys.exit(f"round-trip: entry {i} decoded {g}, expected {w}")
        sys.exit(f"round-trip: {len(got)} entries decoded, {len(want)} expected")


def sign(body, seed=DEV_SEED):
    priv = Ed25519PrivateKey.from_private_bytes(seed)
    pub = priv.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    sig = priv.sign(body)
    return pub, sig


def rust_array(name, data):
    body = ", ".join(f"0x{b:02x}" for b in data)
    return f"pub const {name}: [u8; {len(data)}] = [{body}];"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--emit-rust", action="store_true", help="print dev pubkey + signed blob as Rust literals")
    ap.add_argument("--out", help="write the signed blob to this file (for baking into VIFS1 as /POLICY.BIN)")
    ap.add_argument("--maintenance", action="store_true",
                    help="set MAINTENANCE_PERMITTED — the second factor a maintenance-mode kernel "
                         "needs to bypass policy. Recovery images only.")
    args = ap.parse_args()

    flags = FLAG_MAINTENANCE_PERMITTED if args.maintenance else 0
    body = build_body(DEV_POLICY, flags)
    assert_round_trip(body, DEV_POLICY, flags)
    pub, sig = sign(body)
    blob = body + sig

    if args.out:
        with open(args.out, "wb") as f:
            f.write(blob)
        print(f"wrote {len(blob)} bytes to {args.out}", file=sys.stderr)

    if args.emit_rust or not args.out:
        print(rust_array("DEV_FLEET_PUBKEY", pub))
        print(rust_array("DEV_POLICY_BLOB", blob))
        print(f"// blob = {len(blob)} bytes ({len(body)} body + 64 sig), {len(DEV_POLICY)} entries", file=sys.stderr)


if __name__ == "__main__":
    main()
