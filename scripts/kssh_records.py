#!/usr/bin/env python3
"""Host-side oracle for the K4b record v2 of `ssh-a.bin` / `ssh-b.bin` (RT1 lane).

An independent Python implementation of cells/services/kms/src/ssh/persist/
record.rs (strict decoder, encoder) plus a raw finder over a whole disk image.
Stdlib only. No key material is needed: the record MAC key is the public dev
root constant (devroot.rs), a format check and not authentication.

Layout (little endian, <= 2297 bytes):
  0 magic CKMS | 4 version 2 | 5 slot | 6 kind 1 | 7 state 0/1 | 8 revision u64
  16 previous[32] | 48 name_len | 49 name[64] | 113 custody | 114 public[32]
  146 rounds u32 | 150 salt_len | 151 salt[64] | 215 ct_len u16 | 217 ct[n]
  217+n keyed BLAKE2s-256 over [0, 217+n)

Nothing printed or returned by the finder/oracle carries secret bytes: the
ciphertext is represented by length + sha256 only.
"""
import argparse
import hashlib
import hmac
import json
import os
import subprocess
import sys

DEV_ROOT_LABEL = b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET"

MAGIC = b"CKMS"
VERSION = 2
KIND_SSH = 1
STATE_TOMBSTONE = 0
STATE_PRESENT = 1
CUSTODY_WRAPPED = 2

HEADER_LEN = 217
MAC_LEN = 32
CT_MIN = 144
CT_MAX = 2048
RECORD_MAX = HEADER_LEN + CT_MAX + MAC_LEN  # 2297
NAME_MAX = 64
SALT_MIN = 16
SALT_MAX = 64
ROUNDS_MAX = 64

OFF_SLOT = 5
OFF_KIND = 6
OFF_STATE = 7
OFF_REVISION = 8
OFF_PREVIOUS = 16
OFF_NAME_LEN = 48
OFF_NAME = 49
OFF_CUSTODY = 113
OFF_PUBLIC = 114
OFF_ROUNDS = 146
OFF_SALT_LEN = 150
OFF_SALT = 151
OFF_CT_LEN = 215

PREFIX_V2 = MAGIC + bytes([VERSION])
PREFIX_V1 = MAGIC + b"\x01"
CHUNK = 8 << 20

_NAME_CHARS = frozenset(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-")


def mac_key() -> bytes:
    return hashlib.blake2s(DEV_ROOT_LABEL).digest()


class RecordError(Exception):
    """str(e) is the reason (never secret bytes). `kind` is the failure class:
    "length" (outside 249..=2297), "mac" (tag mismatch, before any field is
    trusted) or "strict" (MAC valid but a strict rule failed)."""

    def __init__(self, reason: str, kind: str = "strict"):
        super().__init__(reason)
        self.kind = kind


def _mac(body: bytes) -> bytes:
    return hashlib.blake2s(body, key=mac_key(), digest_size=MAC_LEN).digest()


def digest(record: bytes) -> bytes:
    return hashlib.blake2s(record, digest_size=32).digest()


def _pad(value: bytes, size: int, what: str) -> bytes:
    if len(value) > size:
        raise ValueError(f"{what} longer than {size}")
    return value + bytes(size - len(value))


def encode_record(*, slot, state, revision, previous=bytes(32), name="", custody=0,
                  public_key=bytes(32), kdf_rounds=0, salt=b"", ciphertext=b"") -> bytes:
    """Exact K4b v2 layout with the MAC appended. Does not police values the
    decoder polices (so tests can build re-MAC'd violations); it only refuses
    what cannot be laid out."""
    name_b = name.encode("ascii") if isinstance(name, str) else bytes(name)
    previous, public_key, salt, ciphertext = bytes(previous), bytes(public_key), bytes(salt), bytes(ciphertext)
    if len(previous) != 32 or len(public_key) != 32:
        raise ValueError("previous and public_key are 32 bytes")
    if len(ciphertext) > 0xFFFF:
        raise ValueError("ciphertext longer than 65535")
    body = b"".join((
        PREFIX_V2, bytes([slot, KIND_SSH, state]),
        int(revision).to_bytes(8, "little"), previous,
        bytes([len(name_b)]), _pad(name_b, NAME_MAX, "name"),
        bytes([custody]), public_key, int(kdf_rounds).to_bytes(4, "little"),
        bytes([len(salt)]), _pad(salt, SALT_MAX, "salt"),
        len(ciphertext).to_bytes(2, "little"), ciphertext,
    ))
    return body + _mac(body)


def _all_zero(data: bytes) -> bool:
    return not data.strip(b"\x00")


def decode_record(data: bytes, expect_slot=None) -> dict:
    """Strictly decode one whole record (K4b parse): exact length, MAC first,
    then enums, bounds, canonical name, zero padding. Raises RecordError."""
    data = bytes(data)
    n = len(data)
    if not HEADER_LEN + MAC_LEN <= n <= RECORD_MAX:
        raise RecordError(f"length {n} outside {HEADER_LEN + MAC_LEN}..={RECORD_MAX}", "length")
    body, tag = data[:-MAC_LEN], data[-MAC_LEN:]
    if not hmac.compare_digest(_mac(body), tag):
        raise RecordError("MAC mismatch", "mac")
    if data[:4] != MAGIC:
        raise RecordError("bad magic")
    if data[4] != VERSION:
        raise RecordError(f"version {data[4]} is not {VERSION}")
    slot = data[OFF_SLOT]
    if slot not in (0, 1):
        raise RecordError(f"slot {slot} is not 0 or 1")
    if expect_slot is not None and slot != expect_slot:
        raise RecordError(f"slot {slot} is not the expected {expect_slot}")
    if data[OFF_KIND] != KIND_SSH:
        raise RecordError(f"kind {data[OFF_KIND]} is not {KIND_SSH}")
    state = data[OFF_STATE]
    revision = int.from_bytes(data[OFF_REVISION:OFF_REVISION + 8], "little")
    if revision == 0:
        raise RecordError("revision is 0")
    previous = data[OFF_PREVIOUS:OFF_PREVIOUS + 32]
    name_len = data[OFF_NAME_LEN]
    name_field = data[OFF_NAME:OFF_NAME + NAME_MAX]
    custody = data[OFF_CUSTODY]
    public_key = data[OFF_PUBLIC:OFF_PUBLIC + 32]
    rounds = int.from_bytes(data[OFF_ROUNDS:OFF_ROUNDS + 4], "little")
    salt_len = data[OFF_SALT_LEN]
    salt_field = data[OFF_SALT:OFF_SALT + SALT_MAX]
    ct_len = int.from_bytes(data[OFF_CT_LEN:OFF_CT_LEN + 2], "little")
    if n != HEADER_LEN + ct_len + MAC_LEN:
        raise RecordError(f"ct_len {ct_len} does not match the record length {n}")
    if state == STATE_TOMBSTONE:
        if name_len != 0 or not _all_zero(name_field):
            raise RecordError("tombstone carries a name")
        if custody != 0:
            raise RecordError(f"tombstone custody {custody} is not 0")
        if not _all_zero(public_key):
            raise RecordError("tombstone carries a public key")
        if rounds != 0:
            raise RecordError("tombstone carries KDF rounds")
        if salt_len != 0 or not _all_zero(salt_field):
            raise RecordError("tombstone carries a salt")
        if ct_len != 0:
            raise RecordError("tombstone carries ciphertext")
        name, salt, ciphertext = "", b"", b""
    elif state == STATE_PRESENT:
        if not 1 <= name_len <= NAME_MAX or not set(name_field[:name_len]) <= _NAME_CHARS:
            raise RecordError("name is not 1..=64 bytes of [A-Za-z0-9_-]")
        if not _all_zero(name_field[name_len:]):
            raise RecordError("name padding is not zero")
        if custody != CUSTODY_WRAPPED:
            raise RecordError(f"custody {custody} is not 2 (wrapped)")
        if not 1 <= rounds <= ROUNDS_MAX:
            raise RecordError(f"KDF rounds {rounds} outside 1..={ROUNDS_MAX}")
        if not SALT_MIN <= salt_len <= SALT_MAX:
            raise RecordError(f"salt length {salt_len} outside {SALT_MIN}..={SALT_MAX}")
        if not _all_zero(salt_field[salt_len:]):
            raise RecordError("salt padding is not zero")
        if ct_len % 16 != 0 or not CT_MIN <= ct_len <= CT_MAX:
            raise RecordError(f"ciphertext length {ct_len} not a multiple of 16 in {CT_MIN}..={CT_MAX}")
        name = name_field[:name_len].decode("ascii")
        salt = salt_field[:salt_len]
        ciphertext = data[HEADER_LEN:HEADER_LEN + ct_len]
    else:
        raise RecordError(f"state {state} is not 0 or 1")
    return {"slot": slot, "state": state, "revision": revision, "previous": previous,
            "name": name, "custody": custody, "public_key": public_key,
            "kdf_rounds": rounds, "salt": salt, "ciphertext": ciphertext,
            "digest": digest(data)}


def _hex(value, what: str) -> bytes:
    try:
        return bytes.fromhex(value)
    except (TypeError, ValueError):
        raise ValueError(f"expected.json record.{what} is not hex") from None


def predict_r1(rec: dict) -> bytes:
    """The host's independent encoding of R1 from expected.json["record"]:
    slot 0, present, revision 1, previous zeros, custody 2."""
    return encode_record(slot=0, state=STATE_PRESENT, revision=1, previous=bytes(32),
                         name=rec["name"], custody=CUSTODY_WRAPPED,
                         public_key=_hex(rec["pub"], "pub"), kdf_rounds=int(rec["rounds"]),
                         salt=_hex(rec["salt"], "salt"), ciphertext=_hex(rec["ct"], "ct"))


# ---- finder -------------------------------------------------------------

def _candidate_header(buf: bytes, at: int):
    """(slot, kind, state) if the bytes after `CKMS\\x02` at `at` are plausible."""
    hdr = buf[at + 5:at + 8]
    if len(hdr) == 3 and hdr[0] in (0, 1) and hdr[1] == KIND_SSH and hdr[2] in (0, 1):
        return hdr
    return None


def _valid_entry(offset: int, rec: dict) -> dict:
    return {"offset": offset, "slot": rec["slot"], "state": rec["state"],
            "revision": rec["revision"], "name": rec["name"],
            "digest": rec["digest"].hex(), "ct_len": len(rec["ciphertext"]),
            "ct_sha256": hashlib.sha256(rec["ciphertext"]).hexdigest(),
            "equals_predicted": None, "previous": rec["previous"].hex(), "count": 1}


def _judge_candidate(fd: int, size: int, offset: int, found: dict, by_digest: dict) -> None:
    head = os.pread(fd, HEADER_LEN, offset)
    if len(head) < HEADER_LEN:
        found["invalid"].append({"offset": offset, "reason": "truncated: header runs past the end of the image",
                                 "mac_valid": False})
        return
    ct_len = int.from_bytes(head[OFF_CT_LEN:OFF_CT_LEN + 2], "little")
    length = HEADER_LEN + ct_len + MAC_LEN
    data = os.pread(fd, length, offset)
    if len(data) < length:
        found["invalid"].append({"offset": offset, "reason": f"truncated: record of {length} bytes runs past the end of the image",
                                 "mac_valid": False})
        return
    try:
        rec = decode_record(data)
    except RecordError as e:
        if e.kind == "length":
            # ct_len > 2048: the decoder refuses on length before the MAC. The MAC
            # of the whole span still tells a torn extent from a writer violation.
            mac_ok = hmac.compare_digest(_mac(data[:-MAC_LEN]), data[-MAC_LEN:])
            found["invalid"].append({"offset": offset, "mac_valid": mac_ok,
                                     "reason": ("strict: " if mac_ok else "mac: ") + str(e)})
        else:
            found["invalid"].append({"offset": offset, "mac_valid": e.kind == "strict",
                                     "reason": f"{e.kind}: {e}"})
        return
    hit = by_digest.get(rec["digest"])
    if hit is None:
        entry = _valid_entry(offset, rec)
        by_digest[rec["digest"]] = entry
        found["valid"].append(entry)
    else:
        hit["count"] += 1
        if offset < hit["offset"]:
            # offsets are visited in increasing order; keep the first one
            hit["offset"] = offset


def find_records(path, chunk: int = CHUNK) -> dict:
    """Raw finder over a whole image. Reads in `chunk` byte steps with an
    overlap of RECORD_MAX, skips all-zero chunks, finds `CKMS\\x02` + plausible
    slot/kind/state, derives the length from ct_len, decodes strictly.
    Returns {"valid": [...], "invalid": [...], "v1": [...]}."""
    if chunk < 1:
        raise ValueError("chunk must be positive")
    found = {"valid": [], "invalid": [], "v1": []}
    by_digest: dict = {}
    seen: set = set()
    fd = os.open(path, os.O_RDONLY)
    try:
        size = os.fstat(fd).st_size
        pos = 0
        while pos < size:
            buf = os.pread(fd, chunk + RECORD_MAX, pos)
            if not buf:
                break
            if _all_zero(buf):
                pos += chunk
                continue
            owned_end = pos + chunk
            at = buf.find(MAGIC, 0, chunk + 4)
            while at != -1:
                off = pos + at
                if off < owned_end and off not in seen:
                    ver = buf[at + 4:at + 5]
                    if ver == b"\x02" and _candidate_header(buf, at) is not None:
                        seen.add(off)
                        _judge_candidate(fd, size, off, found, by_digest)
                    elif ver == b"\x01":
                        seen.add(off)
                        found["v1"].append({"offset": off})
                at = buf.find(MAGIC, at + 1, chunk + 4)
            pos += chunk
    finally:
        os.close(fd)
    found["valid"].sort(key=lambda e: e["offset"])
    found["invalid"].sort(key=lambda e: e["offset"])
    found["v1"].sort(key=lambda e: e["offset"])
    return found


# ---- oracle -------------------------------------------------------------

def _describe(e: dict) -> str:
    kind = "tombstone" if e["state"] == STATE_TOMBSTONE else f"present name={e['name']!r}"
    return f"slot {e['slot']} {kind} rev {e['revision']} at offset {e['offset']}"


def evaluate_set(found: dict, expect: str, record_exp) -> dict:
    """Judge the VALID set against none|r1|r1r2 (amendment section 9). Fills
    `equals_predicted` of slot-0 present records. MAC-failing candidates are
    recorded elsewhere and never judged; a MAC-valid record the strict decoder
    rejected is a verdict failure."""
    if expect not in ("none", "r1", "r1r2"):
        raise ValueError(f"unknown expectation {expect!r}")
    reasons = []
    predicted = None
    if record_exp is not None:
        predicted = digest(predict_r1(record_exp)).hex()
    valid = found["valid"]
    for e in valid:
        if e["slot"] == 0 and e["state"] == STATE_PRESENT:
            e["equals_predicted"] = None if predicted is None else (e["digest"] == predicted)
        else:
            e["equals_predicted"] = None
    for e in found["invalid"]:
        if e.get("mac_valid"):
            reasons.append(f"MAC-valid record rejected by the strict decoder at offset {e['offset']}: {e['reason']}")

    def is_r1(e):
        return e["slot"] == 0 and e["state"] == STATE_PRESENT and e["equals_predicted"] is True

    if expect == "none":
        for e in valid:
            reasons.append(f"unexpected valid record: {_describe(e)}")
    else:
        if predicted is None:
            reasons.append("no host prediction of R1 (record_exp missing)")
        r1s = [e for e in valid if is_r1(e)]
        if not r1s:
            reasons.append("R1 missing: no valid record equals the host's prediction")
        elif len(r1s) > 1:  # cannot happen after digest dedupe; kept as a guard
            reasons.append("more than one distinct R1")
        r1 = r1s[0] if len(r1s) == 1 else None
        r2s = []
        for e in valid:
            if e is r1:
                continue
            if expect == "r1r2" and r1 is not None and e["slot"] == 1 and e["state"] == STATE_TOMBSTONE \
                    and e["previous"] == r1["digest"]:
                r2s.append(e)
                continue
            if e["slot"] == 0 and e["state"] == STATE_PRESENT and e["equals_predicted"] is False:
                reasons.append(f"valid record differs from the host prediction (another key, name, salt, ciphertext, previous or revision): {_describe(e)}")
            elif expect == "r1r2" and e["slot"] == 1 and e["state"] == STATE_TOMBSTONE:
                reasons.append(f"tombstone does not chain onto R1 (previous != digest(R1), or R1 missing): {_describe(e)}")
            else:
                reasons.append(f"unexpected valid record: {_describe(e)}")
        if expect == "r1r2":
            if not r2s and not any("tombstone does not chain" in r for r in reasons) and r1 is not None:
                reasons.append("R2 missing: no slot 1 tombstone chained onto R1")
            if len(r2s) > 1:
                reasons.append("more than one distinct R2")
    return {"expect": expect, "ok": not reasons, "reasons": reasons}


# ---- files --------------------------------------------------------------

def sha256_file(path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            block = f.read(1 << 20)
            if not block:
                break
            h.update(block)
    return h.hexdigest()


def sparse_copy(src, dst) -> None:
    if os.path.lexists(dst):
        raise FileExistsError(f"refusing to overwrite {dst}")
    subprocess.run(["cp", "--sparse=always", "--", os.fspath(src), os.fspath(dst)], check=True)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="K4b record v2 finder (no secrets in the output)")
    sub = ap.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("find", help="find and strictly decode records in an image; print JSON")
    f.add_argument("image")
    args = ap.parse_args(argv)
    json.dump(find_records(args.image), sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
