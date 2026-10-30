#!/usr/bin/env python3
"""Build + Ed25519-sign a Cellos operator policy blob (roadmap §G.2 P5b).

Blob format (little-endian) — MUST match kernel/src/policy.rs::parse:
    magic u32 "VPOL" | version u8=8 | flags u8 | entry_count u16
    per entry: path_len u8, path bytes,
               block_io u8, network u8, spawn u8, hyp u8, mmio_devices u8, block_regions u8,
               pcie_driver u8, platform u8, supervisor u8,          <-- v2+
               usb_driver u8                                        <-- v3+
               serial_port u8                                       <-- v4+
               net_resolve u8, net_connect u8, net_listen u8, net_l2 u8   <-- v5+
               observe_tasks u8, observe_memory u8, vfs_mutate u8          <-- v6+
               config_write u8                                             <-- v7+
               kms_ssh_sign u8, kms_ssh_admin u8                           <-- v8
    + Ed25519 signature [u8;64] over all preceding bytes

Older versions (6/9/10/11/15/18/19 cap bytes) are still accepted by the kernel parser.
Do not emit them. Under v1–v4 the privileged bytes they lack read as 0, and because
a `Permit` INTERSECTS, such an entry strips those caps from every cell it names.
The network rights (annex S2-N, compatibility rule E1), the S2-T/S2-W rights
(annex §4) and the Q right (Phase 01 CP1, D-Q6) are the opposite: an entry that
predates them decodes them as 1, so it leaves them to the reviewed launch ceiling.
The two KSSH K3 rights (`kms_ssh_sign`, `kms_ssh_admin`; O-11) follow the privileged
rule again: signing authority was never ambient, so an entry that predates v8
decodes both as 0 and its Permit strips them until the policy is re-signed at v8.
A v8 row states every right explicitly, so a path that needs one must say so here.

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
import re
import struct
import sys
from pathlib import Path

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives import serialization
except ImportError:
    sys.exit("error: pip install cryptography")

MAGIC = b"VPOL"
VERSION = 8
# Cap-byte stride per blob version; mirrors `cap_bytes_for` in kernel/src/policy.rs.
CAP_BYTES = {1: 6, 2: 9, 3: 10, 4: 11, 5: 15, 6: 18, 7: 19, 8: 21}
DEV_SEED = bytes([0x42] * 32)  # fixed dev seed → deterministic dev keypair

FLAG_MAINTENANCE_PERMITTED = 1 << 0

# Domain limits enforced by kernel/src/policy.rs. A value outside these makes the
# whole blob Invalid → DenyAll for every path → every cell outside the trusted
# core boots with no caps. Checked locally in build_body so that never reaches an
# image: one bad byte here bricks a fleet, and "boot to prompt" still passes
# because the shell comes up from the ramdisk.
# These MUST mirror the kernel's masks in kernel/src/policy.rs. If this file is
# the stricter of the two it rejects blobs the kernel would have accepted, which
# blocks a legitimate re-bake; if it is the looser one it emits a blob the kernel
# scores Invalid, which is the fleet-wide DenyAll described above. Bit values come
# from the DEV_* constants in kernel/src/resource_registry.rs.
DEV_UART = 1 << 0
DEV_GPIO = 1 << 1
DEV_PCIE = 1 << 2
DEV_CAN = 1 << 3
DEV_ADC = 1 << 4
DEV_I2C = 1 << 5
DEV_SPI = 1 << 6
DEV_DISPLAY = 1 << 7
MMIO_MASK = DEV_UART | DEV_GPIO | DEV_PCIE | DEV_CAN | DEV_ADC | DEV_I2C | DEV_SPI | DEV_DISPLAY
REGION_MASK = 0b1111   # P1=1 | P4=2 | SRV=4 | /bin/vfs cell-store=8

# Operator policy: the CEILING each path may hold. It intersects the manifest
# request, so an entry can only ever take authority away — but a missing entry is
# not neutral: under the kernel's `policy-required` feature an unlisted path gets
# CapSet::EMPTY. Every cell that needs any capability must therefore appear here,
# including demos the shell launches.
#
# For the paths that mint privileged (P-TRUST) authority from the install path, a
# missing entry is not merely permissive either: a kernel with a loaded policy
# strips those caps, so the driver cell comes up inert. `assert_ptrust_covered`
# makes that a bake failure instead, checked against the kernel's own table.
#
# (path, block_io, network, spawn, hyp, mmio, regions, pcie_driver, platform, supervisor
#  [, usb_driver [, serial_port [, net_resolve, net_connect, net_listen, net_l2
#  [, observe_tasks, observe_memory, vfs_mutate [, config_write
#  [, kms_ssh_sign [, kms_ssh_admin]]]]]]])
#
# Omitted trailing cap columns are 0, EXCEPT the three S2-T/S2-W rights (task
# observation, system memory, VFS mutation), the Q right (Config write) and the two
# KSSH K3 rights: a row that omits them gets exactly what the kernel's
# `CapSet::with_path_caps` requests for the path. For S2-T/S2-W that is all three
# unless the path is under a user-app sandbox prefix (annex S2-T/S2-W §3, D-TW1);
# for Q it is 1 for `/bin/cellbox` only (Phase 01 CP1, D-Q5); for `kms_ssh_sign` it
# is 1 for `/bin/cellbox-ssh` only and for `kms_ssh_admin` 1 for
# `/bin/kms-ssh-admin` only (KSSH K3; KSSH K7 adds the DEV_POLICY row for the admin
# path — without a row a loaded policy strips the right, H-1). The S2-T/S2-W and Q rights were ambient before
# their slices, so a 0 default would strip `ps`/`free`, file writes and `export`
# from every listed cell. A restricted row states them.
#
# The four network rights (annex S2-N §5) must match the kernel's
# `CapSet::with_path_net_rights` grant table: a Permit intersects, so a 0 here
# strips a right the reviewed launch edge would allow.
NET_CLIENT = (1, 1, 0, 0)    # resolve + connect
NET_PEER = (1, 1, 1, 0)      # resolve + connect + listen
NET_LISTENER = (0, 0, 1, 0)  # listen only
NET_L2 = (0, 0, 0, 1)        # raw frames only
OBSERVE_MUTATE = (1, 1, 1)   # CapSet::LEGACY_OBSERVE_MUTATE
# Mirrors `CapSet::with_path_observe_mutate_rights` in kernel/src/task/cap.rs.
SANDBOX_PREFIXES = ("/apps/", "/data/bin/", "/home/")
# Mirrors `CapSet::with_path_config_rights` in kernel/src/task/cap.rs.
CONFIG_WRITE_PATHS = ("/bin/cellbox",)
# Mirrors `CapSet::with_path_kms_ssh_rights` in kernel/src/task/cap.rs.
KMS_SSH_SIGN_PATHS = ("/bin/cellbox-ssh",)
KMS_SSH_ADMIN_PATHS = ("/bin/kms-ssh-admin",)
DEV_POLICY = [
    # ── root authority ────────────────────────────────────────────────────────
    # init is Spawner::Root and exempt in-kernel, so this entry is informational
    # today. It stays maximal on purpose: init's own caps are the CEILING for
    # every cell it spawns, so a narrow entry here would starve its children if
    # that exemption is ever removed.
    ("/bin/init",        1, 1, 1, 0, DEV_UART | DEV_GPIO | DEV_I2C | DEV_SPI | DEV_DISPLAY, 0b111, 1, 1, 1),
    # Kernel-spawned (Root, exempt); listed so the intent is reviewable.
    ("/bin/platform",    0, 0, 0, 0, 0, 0,     0, 1, 0),
    # ── core services ─────────────────────────────────────────────────────────
    ("/bin/vfs",         1, 0, 0, 0, 0, 0b1111, 0, 0, 0),
    ("/bin/net",         0, 1, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/net-broker",  0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_PEER),
    ("/bin/supervisor",  0, 0, 1, 0, 0, 0,     0, 0, 1),
    ("/bin/hypervisor",  0, 0, 0, 1, 0, 0,     0, 0, 0, 0, 0, *NET_L2),
    # Shell holds no ambient lifecycle authority; exact child launch ceilings
    # are reviewed in kernel/src/loader/launch_profile.rs.
    ("/bin/cellbox",     0, 0, 0, 0, 0, 0,     0, 0, 0),
    # Phase 09 D79-9: the same cellbox ELF at its ssh install path; outbound
    # client rights only, no direct NIC access.
    ("/bin/cellbox-ssh", 0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    # KSSH K7: the KMS SSH admin cell, launched by the shell's reviewed path edge only.
    # No network, no spawn; the omitted columns default to the kernel's own request for
    # the path (X/M/W and the admin right; never the sign right).
    ("/bin/kms-ssh-admin", 0, 0, 0, 0, 0, 0,   0, 0, 0),
    # ── driver cells (pcie_driver comes from CapSet::with_path_caps) ───────────
    ("/bin/block",       0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/nvme",        0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/ahci",        0, 0, 0, 0, 0, 0,     1, 0, 0),
    # xHCI USB host: pcie_driver for the 0C:03:30 BAR claim, usb_driver for the
    # kernel-verified input-producer identity (sys_register_nic_driver), same
    # split as /bin/dwc2-usb + the PCIe cells.
    ("/bin/xhci",        0, 0, 0, 0, 0, 0,     1, 0, 0, 1),
    ("/bin/input",       0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/virtio-net",  0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/e1000",       0, 0, 0, 0, 0, 0,     1, 0, 0),
    # igb NIC family (i210/i211): same pcie_driver-only shape as /bin/e1000.
    ("/bin/igb",         0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/virtio-gpu",  0, 0, 0, 0, 0, 0,     1, 0, 0),
    ("/bin/bcm-display", 0, 0, 0, 0, DEV_DISPLAY, 0,     0, 0, 0),
    ("/bin/dwc2-usb",    0, 0, 1, 0, 0, 0,     0, 0, 0, 1),
    # x86 serial Driver Cell: port-I/O authority over the 16550 ports the kernel
    # probed. No PCIe, no MMIO, no DMA — legacy I/O space only.
    ("/bin/serial",      0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 1),
    ("/bin/lan9514",     0, 0, 0, 0, 0, 0,     0, 0, 0, 0),
    ("/bin/periph-demo", 0, 0, 0, 0, 3, 0,     0, 0, 0),
    ("/bin/periph-test", 0, 0, 0, 0, 3, 0,     0, 0, 0),
    ("/bin/robot-demo",  0, 1, 0, 0, 2, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    ("/bin/sensor-demo", 0, 0, 0, 0, DEV_GPIO | DEV_I2C, 0,    0, 0, 0),
    ("/bin/spi-demo",    0, 0, 0, 0, DEV_GPIO | DEV_SPI, 0,    0, 0, 0),
    ("/bin/pwm-demo",    0, 0, 0, 0, 2, 0,     0, 0, 0),
    ("/bin/gpio-test-rv",0, 0, 0, 0, 2, 0,     0, 0, 0),
    ("/bin/bench",       0, 0, 1, 0, 0, 0,     0, 0, 0),
    ("/bin/bench-probe", 0, 0, 1, 0, 0, 0,     0, 0, 0),
    ("/bin/c2c-render",  0, 0, 1, 0, 0, 0,     0, 0, 0),
    ("/bin/c2c-render-worker", 0, 0, 0, 0, 0, 0, 0, 0, 0),
    ("/bin/c2c-render-domain-worker", 0, 0, 0, 0, 0, 0, 0, 0, 0),
    ("/bin/hotswap-demo-v1", 0, 0, 1, 0, 0, 0, 0, 0, 0),
    ("/bin/hotswap-demo-v2", 0, 0, 1, 0, 0, 0, 0, 0, 0),
    ("/bin/hypha",       0, 0, 1, 0, 0, 0,     0, 0, 0),
    ("/bin/llm-gateway", 0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    ("/bin/tool-spawn",  0, 0, 1, 0, 0, 0,     0, 0, 0),
    ("/bin/nc",          0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_PEER),
    ("/bin/curl",        0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    ("/bin/wget",        0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    ("/bin/httpd",       0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_LISTENER),
    ("/bin/mqtt",        0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    ("/bin/posix-shim-test", 0, 1, 0, 0, 0, 0, 0, 0, 0),
    ("/bin/https-demo",  0, 1, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    ("/bin/http-smoke",  0, 1, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/std-smoke",   0, 0, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/desktop",     0, 0, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/compositor",  0, 0, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/fb-console",  0, 0, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/sshd",        0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_LISTENER),
    ("/bin/ocel",        0, 0, 0, 0, 0, 0,     0, 0, 0, 0, 0, *NET_CLIENT),
    ("/bin/ocel-js",     0, 0, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/ocel-quickjs", 0, 0, 0, 0, 0, 0,    0, 0, 0),
    ("/bin/ocel-pdf",     0, 0, 0, 0, 0, 0,    0, 0, 0),
    ("/bin/tier2-smoke", 0, 0, 0, 0, 0, 0,     0, 0, 0),
    ("/bin/tier2-exploit", 0, 0, 0, 0, 0, 0,   0, 0, 0),
]


ROW_FIELDS = 22  # path + 21 cap columns (the v8 entry)
SIGN_ROW_FIELDS = 21  # path + 20 cap columns (through kms_ssh_sign)
CFG_ROW_FIELDS = 20  # path + 19 cap columns (through config_write, the v7 entry)
OM_ROW_FIELDS = 19  # path + 18 cap columns (through the S2-T/S2-W rights)
NET_ROW_FIELDS = 16  # path + 15 cap columns (through the network rights)


def observe_mutate_request(path):
    """The S2-T/S2-W rights the kernel's `with_path_caps` requests for `path`."""
    return (0, 0, 0) if path.startswith(SANDBOX_PREFIXES) else OBSERVE_MUTATE


def config_write_request(path):
    """The Q right the kernel's `with_path_caps` requests for `path`."""
    return (1,) if path in CONFIG_WRITE_PATHS else (0,)


def kms_ssh_sign_request(path):
    """The KSSH K3 sign right the kernel's `with_path_caps` requests for `path`."""
    return (1,) if path in KMS_SSH_SIGN_PATHS else (0,)


def kms_ssh_admin_request(path):
    """The KSSH K3 admin right the kernel's `with_path_caps` requests for `path`."""
    return (1,) if path in KMS_SSH_ADMIN_PATHS else (0,)


def full_row(row):
    """Pad a DEV_POLICY row to every v8 column.

    Omitted cap columns are 0 except the trailing S2-T/S2-W three, the Q column
    and the two KSSH K3 columns, which default to the kernel's own request for
    the path (see the DEV_POLICY comment).
    """
    n = len(row)
    path = row[0]
    if n == ROW_FIELDS:
        return tuple(row)
    if n == SIGN_ROW_FIELDS:
        return tuple(row) + kms_ssh_admin_request(path)
    if n == CFG_ROW_FIELDS:
        return tuple(row) + kms_ssh_sign_request(path) + kms_ssh_admin_request(path)
    if n == OM_ROW_FIELDS:
        return (tuple(row) + config_write_request(path)
                + kms_ssh_sign_request(path) + kms_ssh_admin_request(path))
    if not 10 <= n <= NET_ROW_FIELDS or n in (13, 14, 15):
        sys.exit(
            f"{path}: row has {n} fields — network rights take all four columns "
            "and S2-T/S2-W rights all three"
        )
    return (tuple(row) + (0,) * (NET_ROW_FIELDS - n) + observe_mutate_request(path)
            + config_write_request(path) + kms_ssh_sign_request(path)
            + kms_ssh_admin_request(path))


def build_body(entries, flags=0):
    out = bytearray()
    out += MAGIC
    out += struct.pack("<BBH", VERSION, flags, len(entries))
    seen = set()
    for row in entries:
        (path, bio, net, spawn, hyp, mmio, regions, pcie, plat, sup, usb, serial,
         n_resolve, n_connect, n_listen, n_l2, o_tasks, o_memory, vfs_mut, cfg_w,
         k_sign, k_admin) = full_row(row)
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
        for name, v in (
            ("pcie_driver", pcie),
            ("platform", plat),
            ("supervisor", sup),
            ("usb_driver", usb),
            ("serial_port", serial),
            ("net_resolve", n_resolve),
            ("net_connect", n_connect),
            ("net_listen", n_listen),
            ("net_l2", n_l2),
            ("observe_tasks", o_tasks),
            ("observe_memory", o_memory),
            ("vfs_mutate", vfs_mut),
            ("config_write", cfg_w),
            ("kms_ssh_sign", k_sign),
            ("kms_ssh_admin", k_admin),
        ):
            if v not in (0, 1):
                sys.exit(f"{path}: {name} must be 0 or 1, got {v} — blob would be Invalid")
        # Not a kernel decode rule (a Permit never widens a request, and no request
        # names both paths): a bake-time lint so no policy row ever grants one Cell
        # both KMS SSH classes (code-standards.md KMS SSH Custody Boundary).
        if k_sign and k_admin:
            sys.exit(f"{path}: kms_ssh_sign and kms_ssh_admin must never both be 1")
        out.append(len(pb))
        out += pb
        caps = [bio, net, spawn, hyp, mmio, regions, pcie, plat, sup, usb, serial,
                n_resolve, n_connect, n_listen, n_l2, o_tasks, o_memory, vfs_mut, cfg_w,
                k_sign, k_admin]
        out += bytes(caps[:CAP_BYTES[VERSION]])
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
    cap_bytes = {1: 6, 2: 9, 3: 10, 4: 11, 5: 15, 6: 18, 7: 19, 8: 21}.get(version)
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
        if cap_bytes >= 11:
            for name, v in (
                ("pcie_driver", caps[6]),
                ("platform", caps[7]),
                ("supervisor", caps[8]),
                ("usb_driver", caps[9]),
                ("serial_port", caps[10]),
            ):
                if v > 1:
                    raise ValueError(f"{path}: {name}={v} out of domain")
            priv = (caps[6], caps[7], caps[8], caps[9], caps[10])
        elif cap_bytes == 10:
            for name, v in (("pcie_driver", caps[6]), ("platform", caps[7]), ("supervisor", caps[8]), ("usb_driver", caps[9])):
                if v > 1:
                    raise ValueError(f"{path}: {name}={v} out of domain")
            priv = (caps[6], caps[7], caps[8], caps[9], 0)
        elif cap_bytes == 9:
            for name, v in (("pcie_driver", caps[6]), ("platform", caps[7]), ("supervisor", caps[8])):
                if v > 1:
                    raise ValueError(f"{path}: {name}={v} out of domain")
            priv = (caps[6], caps[7], caps[8], 0, 0)
        else:
            priv = (0, 0, 0, 0, 0)
        if cap_bytes >= 15:
            for name, v in zip(("net_resolve", "net_connect", "net_listen", "net_l2"), caps[11:15]):
                if v > 1:
                    raise ValueError(f"{path}: {name}={v} out of domain")
            net_rights = tuple(caps[11:15])
        else:
            # Compatibility rule E1: a pre-v5 entry does not narrow network rights.
            net_rights = (1, 1, 1, 1)
        if cap_bytes >= 18:
            for name, v in zip(("observe_tasks", "observe_memory", "vfs_mutate"), caps[15:18]):
                if v > 1:
                    raise ValueError(f"{path}: {name}={v} out of domain")
            observe_mutate = tuple(caps[15:18])
        else:
            # Annex S2-T/S2-W §4: a pre-v6 entry does not narrow these rights.
            observe_mutate = (1, 1, 1)
        if cap_bytes >= 19:
            if caps[18] > 1:
                raise ValueError(f"{path}: config_write={caps[18]} out of domain")
            config_write = caps[18]
        else:
            # Phase 01 CP1 D-Q6: a pre-v7 entry does not narrow the Q right.
            config_write = 1
        if cap_bytes >= 21:
            for name, v in zip(("kms_ssh_sign", "kms_ssh_admin"), caps[19:21]):
                if v > 1:
                    raise ValueError(f"{path}: {name}={v} out of domain")
            kms_ssh = tuple(caps[19:21])
        else:
            # KSSH K3 O-11: a pre-v8 entry decodes both rights as 0 (never ambient),
            # so its Permit strips them — the opposite of the S2-N/S2-T/Q rule.
            kms_ssh = (0, 0)
        out.append((path, caps[0], caps[1], caps[2], caps[3], caps[4], caps[5], *priv,
                    *net_rights, *observe_mutate, config_write, *kms_ssh))
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
    # Rows may omit trailing cap columns (a 10-field row declares no usb_driver,
    # serial_port or network right, and takes the path's default S2-T/S2-W and Q
    # rights), so pad the table to the full v7 row first.
    want = [full_row(e) for e in entries]
    got = [tuple(g) for g in got]
    if got_flags != flags:
        sys.exit(f"round-trip: flags {got_flags:#04x} != {flags:#04x}")
    if got != want:
        for i, (g, w) in enumerate(zip(got, want)):
            if g != w:
                sys.exit(f"round-trip: entry {i} decoded {g}, expected {w}")
        sys.exit(f"round-trip: {len(got)} entries decoded, {len(want)} expected")


def assert_vfs_regions_folded(entries):
    """Require /bin/vfs to carry all four block regions after decode.

    The independent decoder is the right place to pin the fold: if the source
    table drifts back to 0b111 the image still bakes and the old loader raw
    grant could hide it at runtime. This check forces the baked blob itself to
    carry the cell-store bit before the loader fallback is removed.
    """
    for entry in entries:
        if entry[0] == "/bin/vfs":
            if entry[6] != 0b1111:
                sys.exit(f"/bin/vfs decoded block_regions {entry[6]:#06b}, expected 0b1111")
            return
    sys.exit("/bin/vfs missing from policy table")


def assert_bcm_display_authority(entries):
    """Require the baked BCM row to retain its mailbox and driver authority."""
    for entry in entries:
        if entry[0] == "/bin/bcm-display":
            if entry[5] != DEV_DISPLAY or entry[7] != 0:
                sys.exit(
                    "/bin/bcm-display must retain DEV_DISPLAY MMIO without pcie_driver authority"
                )
            return
    sys.exit("/bin/bcm-display missing from policy table")


CAP_SOURCE = Path(__file__).resolve().parent.parent / "kernel" / "src" / "task" / "cap.rs"


def ptrust_paths(cap_source=CAP_SOURCE):
    """Paths whose install path alone mints privileged (P-TRUST) authority.

    Read out of `CapSet::with_path_caps` in the kernel source rather than copied
    into this file: those match arms are what the running kernel obeys, and a
    second hand-maintained list is the exact drift this check exists to catch.
    Only the function body is scanned, so the doc comment above it — which names
    example paths — cannot contribute.

    Exits (never returns a partial answer) if the source or the function cannot be
    found, or if the body yields no paths: a check that silently matches nothing
    would report coverage the kernel does not have.
    """
    try:
        lines = cap_source.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        sys.exit(f"cannot read {cap_source}: {exc} — P-TRUST coverage check cannot run")
    start = next((i for i, ln in enumerate(lines) if "fn with_path_caps" in ln), None)
    if start is None:
        sys.exit(f"{cap_source}: fn with_path_caps not found — P-TRUST coverage check cannot run")
    paths, depth, opened = [], 0, False
    for ln in lines[start:]:
        code = ln.split("//")[0]  # a trailing comment may legitimately name a path
        paths += re.findall(r'"(/[^"]*)"', code)
        depth += code.count("{") - code.count("}")
        opened = opened or "{" in code
        if opened and depth <= 0:
            break
    else:
        sys.exit(f"{cap_source}: with_path_caps body never closed — P-TRUST check cannot run")
    if not paths:
        sys.exit(f"{cap_source}: no paths parsed from with_path_caps — check would be vacuous")
    return paths


def assert_ptrust_covered(entries, paths=None):
    """Require every P-TRUST-minting path to appear in the policy table.

    The kernel fails closed for these paths when a *loaded* policy has no entry:
    it strips the privileged caps and audits the gap. That stops a forgotten entry
    from handing out DMA-anywhere authority, but a driver cell without its caps is
    still a dead device — so the omission has to be caught here, at bake time,
    where the fix is one line of DEV_POLICY.

    Presence is the whole requirement. An entry that deliberately zeroes
    `pcie_driver`/`platform`/`supervisor` is an operator decision the kernel honours
    through the normal `Permit` intersection, not a mistake.
    """
    listed = {e[0] for e in entries}
    missing = sorted({p for p in (ptrust_paths() if paths is None else paths) if p not in listed})
    if missing:
        sys.exit(
            "P-TRUST paths missing from the policy table — the kernel would strip "
            f"their privileged caps at runtime: {', '.join(missing)}"
        )


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
    _, decoded = decode_body(body)
    assert_vfs_regions_folded(decoded)
    assert_bcm_display_authority(decoded)
    # Unconditional, like the round-trip: the gate has to hold for every blob that
    # gets built, not only when someone remembers a flag.
    assert_ptrust_covered(DEV_POLICY)
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
