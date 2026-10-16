#!/usr/bin/env python3
"""
verify-ogj1-codec.py: Standalone, model-only OGJ1 codec/lifecycle regressions.
Digest convention here is domain + header + body, not approved persistent bytes.
No authority admission, durable lifecycle/charge reconstruction, or checkpoint runtime.
"""

import sys
import struct
import hashlib
import json
import argparse
from typing import Dict, Any, Optional

MAGIC = b"OGJ1"
VERSION = 1
DOMAIN_PREFIX = b"cellos/ordinary-journal/v1\x00"
HEADER_LEN = 112
TRAILER_LEN = 32

KIND_PAYLOAD = 1
KIND_COMMIT = 2
KIND_CHECKPOINT = 3
KIND_SWITCH = 4

MAX_CANONICAL_PAYLOAD_LEN = 256
COMMIT_BODY_LEN = 40
UINT64_MAX = (1 << 64) - 1
MIN_PAYLOAD_RECORD_LEN = HEADER_LEN + 36 + TRAILER_LEN
MAX_PAYLOAD_RECORD_LEN = MIN_PAYLOAD_RECORD_LEN + MAX_CANONICAL_PAYLOAD_LEN
MODEL_LIMITATIONS = [
    "Model inputs are not initialization, authority, signer, or floor trust evidence.",
    "Digest convention is domain + header + body; governing byte ambiguity is unresolved.",
    "Checkpoint context is an explicit model assumption, not a checkpoint implementation.",
    "No durable charge vector, accounting reconstruction, or charge adoption is implemented.",
    "Recovery never opens mutation admission; runtime/VFS/kernel and persistent cutover remain blocked.",
]

class CodecError(Exception):
    pass


def require_uint(name: str, value: int, maximum: int, minimum: int = 0):
    if type(value) is not int or not minimum <= value <= maximum:
        raise CodecError(f"{name} must be an integer in {minimum}..{maximum}")


def require_bytes(name: str, value: bytes, length: Optional[int] = None):
    if not isinstance(value, bytes) or (length is not None and len(value) != length):
        raise CodecError(f"{name} must be bytes" + (f" of length {length}" if length is not None else ""))

class Ogj1Codec:
    @staticmethod
    def compute_digest(header_bytes: bytes, body_bytes: bytes) -> bytes:
        hasher = hashlib.sha256()
        hasher.update(DOMAIN_PREFIX)
        hasher.update(header_bytes)
        hasher.update(body_bytes)
        return hasher.digest()

    @classmethod
    def encode_record(cls, kind: int, ledger_id: bytes, tx_id: bytes,
                      revision: int, fence: int, prev_commit_digest: bytes,
                      body_bytes: bytes) -> bytes:
        require_uint("kind", kind, KIND_SWITCH, KIND_PAYLOAD)
        require_bytes("ledger_id", ledger_id, 32)
        require_bytes("tx_id", tx_id, 16)
        require_bytes("prev_commit_digest", prev_commit_digest, 32)
        require_uint("revision", revision, UINT64_MAX, 1)
        require_uint("fence", fence, UINT64_MAX)
        require_bytes("body_bytes", body_bytes)
        cls.decode_body(kind, body_bytes)
        body_len = len(body_bytes)
        total_len = HEADER_LEN + body_len + TRAILER_LEN
        reserved = 0

        header_bytes = struct.pack(
            "<4s H B B I 32s 16s Q Q 32s I",
            MAGIC,
            VERSION,
            kind,
            reserved,
            total_len,
            ledger_id,
            tx_id,
            revision,
            fence,
            prev_commit_digest,
            body_len
        )
        assert len(header_bytes) == HEADER_LEN

        trailer_bytes = cls.compute_digest(header_bytes, body_bytes)
        assert len(trailer_bytes) == TRAILER_LEN

        return header_bytes + body_bytes + trailer_bytes

    @classmethod
    def decode_record(cls, raw: bytes) -> Dict[str, Any]:
        require_bytes("raw", raw)
        if len(raw) < HEADER_LEN + TRAILER_LEN:
            raise CodecError(f"Record truncated: length {len(raw)} < minimum {HEADER_LEN + TRAILER_LEN}")

        header_bytes = raw[:HEADER_LEN]
        (magic, version, kind, reserved, total_len,
         ledger_id, tx_id, revision, fence,
         prev_commit_digest, body_len) = struct.unpack(
            "<4s H B B I 32s 16s Q Q 32s I", header_bytes
        )

        if magic != MAGIC:
            raise CodecError(f"Invalid magic: expected {MAGIC}, got {magic}")
        if version != VERSION:
            raise CodecError(f"Unsupported version: expected {VERSION}, got {version}")
        if kind not in (KIND_PAYLOAD, KIND_COMMIT, KIND_CHECKPOINT, KIND_SWITCH):
            raise CodecError(f"Unknown record kind: {kind}")
        if reserved != 0:
            raise CodecError(f"Non-zero reserved field: {reserved}")
        if total_len != len(raw):
            raise CodecError(f"Total length mismatch: header declares {total_len}, got {len(raw)}")
        if total_len != HEADER_LEN + body_len + TRAILER_LEN:
            raise CodecError(f"Internal length inconsistency: declared total {total_len} != {HEADER_LEN + body_len + TRAILER_LEN}")
        require_uint("revision", revision, UINT64_MAX, 1)
        require_uint("fence", fence, UINT64_MAX)

        body_bytes = raw[HEADER_LEN:HEADER_LEN + body_len]
        trailer_bytes = raw[HEADER_LEN + body_len:HEADER_LEN + body_len + TRAILER_LEN]

        expected_digest = cls.compute_digest(header_bytes, body_bytes)
        if trailer_bytes != expected_digest:
            raise CodecError("Digest verification failure: record content or domain prefix mismatch")

        # Parse kind-specific body
        parsed_body = cls.decode_body(kind, body_bytes)

        return {
            "magic": magic,
            "version": version,
            "kind": kind,
            "reserved": reserved,
            "total_len": total_len,
            "ledger_id": ledger_id,
            "tx_id": tx_id,
            "revision": revision,
            "fence": fence,
            "prev_commit_digest": prev_commit_digest,
            "body_len": body_len,
            "digest": trailer_bytes,
            "body": parsed_body
        }

    @classmethod
    def encode_payload_body(cls, schema_id: bytes, canonical_bytes: bytes) -> bytes:
        require_bytes("schema_id", schema_id, 32)
        require_bytes("canonical_bytes", canonical_bytes)
        if len(canonical_bytes) > MAX_CANONICAL_PAYLOAD_LEN:
            raise CodecError(f"canonical_bytes exceeds max length {MAX_CANONICAL_PAYLOAD_LEN}: {len(canonical_bytes)}")
        return struct.pack("<32s I", schema_id, len(canonical_bytes)) + canonical_bytes

    @classmethod
    def encode_commit_body(cls, referenced_slot: int, referenced_len: int, referenced_digest: bytes) -> bytes:
        require_uint("referenced_slot", referenced_slot, 3)
        require_uint("referenced_len", referenced_len, MAX_PAYLOAD_RECORD_LEN, MIN_PAYLOAD_RECORD_LEN)
        require_bytes("referenced_digest", referenced_digest, 32)
        body = struct.pack("<I I 32s", referenced_slot, referenced_len, referenced_digest)
        assert len(body) == COMMIT_BODY_LEN
        return body

    @classmethod
    def decode_body(cls, kind: int, body_bytes: bytes) -> Dict[str, Any]:
        require_uint("kind", kind, KIND_SWITCH, KIND_PAYLOAD)
        require_bytes("body_bytes", body_bytes)
        if kind in (KIND_PAYLOAD, KIND_CHECKPOINT):
            if len(body_bytes) < 36:
                raise CodecError(f"Payload body truncated: {len(body_bytes)} < 36")
            schema_id, canonical_len = struct.unpack("<32s I", body_bytes[:36])
            canonical_bytes = body_bytes[36:]
            if len(canonical_bytes) != canonical_len:
                raise CodecError(f"Payload canonical length mismatch: declared {canonical_len}, got {len(canonical_bytes)}")
            if canonical_len > MAX_CANONICAL_PAYLOAD_LEN:
                raise CodecError(f"Payload canonical length {canonical_len} exceeds maximum {MAX_CANONICAL_PAYLOAD_LEN}")
            return {
                "schema_id": schema_id,
                "canonical_len": canonical_len,
                "canonical_bytes": canonical_bytes
            }
        elif kind in (KIND_COMMIT, KIND_SWITCH):
            if len(body_bytes) != COMMIT_BODY_LEN:
                raise CodecError(f"Commit body length must be {COMMIT_BODY_LEN}, got {len(body_bytes)}")
            ref_slot, ref_len, ref_digest = struct.unpack("<I I 32s", body_bytes)
            require_uint("referenced_slot", ref_slot, 1 if kind == KIND_SWITCH else 3)
            require_uint("referenced_len", ref_len, MAX_PAYLOAD_RECORD_LEN, MIN_PAYLOAD_RECORD_LEN)
            return {
                "referenced_slot": ref_slot,
                "referenced_len": ref_len,
                "referenced_digest": ref_digest
            }
        else:
            raise CodecError(f"Unsupported kind: {kind}")


class JournalStateMachine:
    """In-memory barrier model only; recovery cannot reconstruct durable charges."""

    def __init__(self, ledger_id: bytes, boot_floor: int):
        require_bytes("ledger_id", ledger_id, 32)
        require_uint("boot_floor", boot_floor, UINT64_MAX)
        self.ledger_id = ledger_id
        self.boot_floor = boot_floor
        self.state = "RECOVERING"
        self.confirmed_revision = 0
        self.confirmed_digest = b"\x00" * 32
        self.enforcement_fence = boot_floor
        self.tx_slots: Dict[int, Dict[str, bytes]] = {i: {} for i in range(4)}
        self.retained_charges: Dict[bytes, Dict[str, int]] = {}
        self.active_unknown: Optional[bytes] = None
        self.active: Optional[Dict[str, Any]] = None

    def initialize_writable(self, revision: int, digest: bytes, fence: int):
        # Explicit synthetic setup, never a durable initialization/admission path.
        if self.state != "RECOVERING":
            raise CodecError("Model setup allowed only on a fresh machine")
        require_uint("revision", revision, UINT64_MAX)
        require_bytes("digest", digest, 32)
        require_uint("fence", fence, UINT64_MAX)
        if revision == 0 and digest != b"\x00" * 32:
            raise CodecError("Model genesis digest must be zero")
        if revision > 0 and digest == b"\x00" * 32:
            raise CodecError("Non-genesis model checkpoint needs a digest")
        if fence < self.boot_floor:
            self.state = "RECOVERY_FAILED"
            raise CodecError(f"Fence regression: {fence} < boot floor {self.boot_floor}")
        self.state = "WRITABLE"
        self.confirmed_revision = revision
        self.confirmed_digest = digest
        self.enforcement_fence = fence

    def _require_active(self, tx_id: bytes, slot_idx: int, phase: str):
        require_bytes("tx_id", tx_id, 16)
        require_uint("slot_idx", slot_idx, 3)
        if self.state != "WRITABLE" or self.active is None:
            raise CodecError("No writable reserved mutation")
        if self.active["tx_id"] != tx_id or self.active["slot"] != slot_idx:
            raise CodecError("Reserved active identity mismatch")
        if self.active["phase"] != phase:
            raise CodecError(f"Mutation phase must be {phase}, got {self.active['phase']}")

    def _unknown(self, message: str):
        self.state = "UNKNOWN"
        self.active_unknown = self.active["tx_id"]
        self.active["phase"] = "UNKNOWN"
        raise CodecError(message)

    def begin_mutation(self, tx_id: bytes, slot_idx: int, prospective_charge: Dict[str, int],
                       schema_id: bytes, canonical_body: bytes) -> bytes:
        if self.state != "WRITABLE":
            raise CodecError(f"Cannot mutate in state {self.state}")
        if self.active is not None:
            raise CodecError("Reserved mutation already active")
        require_bytes("tx_id", tx_id, 16)
        require_uint("slot_idx", slot_idx, 3)
        if tx_id in self.retained_charges:
            raise CodecError("Duplicate transaction identity")
        if self.tx_slots[slot_idx]:
            raise CodecError(f"Slot {slot_idx} already occupied in active generation")
        if not isinstance(prospective_charge, dict) or not prospective_charge:
            raise CodecError("Prospective charge must be a nonempty model vector")
        for name, amount in prospective_charge.items():
            if not isinstance(name, str) or not name:
                raise CodecError("Charge dimension must be a nonempty string")
            require_uint("charge", amount, UINT64_MAX)

        # All encoding/validation precedes mutation of charge/reservation state.
        payload_body = Ogj1Codec.encode_payload_body(schema_id, canonical_body)
        payload_record = Ogj1Codec.encode_record(
            KIND_PAYLOAD, self.ledger_id, tx_id, self.confirmed_revision + 1,
            self.enforcement_fence, self.confirmed_digest, payload_body)
        self.retained_charges[tx_id] = prospective_charge.copy()
        self.active = {"tx_id": tx_id, "slot": slot_idx, "phase": "RESERVED",
                       "payload": payload_record}
        return payload_record

    def execute_d1(self, tx_id: bytes, slot_idx: int, payload_record: bytes, simulate_cut: Optional[str] = None):
        self._require_active(tx_id, slot_idx, "RESERVED")
        if simulate_cut not in (None, "PRE_D1_CRASH", "D1_BARRIER_FAIL"):
            raise CodecError("Unknown D1 simulation cut")
        if payload_record != self.active["payload"]:
            raise CodecError("Payload differs from reserved encoding")
        if self.tx_slots[slot_idx]:
            raise CodecError("D1 cannot overwrite occupied slot")
        if simulate_cut == "PRE_D1_CRASH":
            self._unknown("Simulation: Crash before barrier D1")
        self.tx_slots[slot_idx]["payload"] = payload_record
        if simulate_cut == "D1_BARRIER_FAIL":
            self._unknown("Simulation: Native barrier D1 failure")
        self.active["phase"] = "D1_ACKNOWLEDGED"

    def build_commit(self, tx_id: bytes, slot_idx: int) -> bytes:
        self._require_active(tx_id, slot_idx, "D1_ACKNOWLEDGED")
        payload_record = self.tx_slots[slot_idx].get("payload")
        if payload_record != self.active["payload"]:
            raise CodecError("Reserved payload missing or replaced")
        payload = Ogj1Codec.decode_record(payload_record)
        body = Ogj1Codec.encode_commit_body(slot_idx, len(payload_record), payload["digest"])
        return Ogj1Codec.encode_record(
            KIND_COMMIT, self.ledger_id, tx_id, payload["revision"],
            payload["fence"], payload["prev_commit_digest"], body)

    def execute_d2(self, tx_id: bytes, slot_idx: int, commit_record: bytes, simulate_cut: Optional[str] = None):
        self._require_active(tx_id, slot_idx, "D1_ACKNOWLEDGED")
        if simulate_cut not in (None, "POST_D1_PRE_D2_CRASH", "D2_BARRIER_FAIL", "POST_D2_REPLY_LOSS"):
            raise CodecError("Unknown D2 simulation cut")
        if commit_record != self.build_commit(tx_id, slot_idx):
            raise CodecError("Commit differs from reserved payload binding")
        if "commit" in self.tx_slots[slot_idx]:
            raise CodecError("D2 cannot overwrite commit")
        if simulate_cut == "POST_D1_PRE_D2_CRASH":
            self._unknown("Simulation: Crash before barrier D2 (orphan payload)")
        self.tx_slots[slot_idx]["commit"] = commit_record
        if simulate_cut == "D2_BARRIER_FAIL":
            self._unknown("Simulation: Native barrier D2 failure; bytes may have been accepted")
        commit = Ogj1Codec.decode_record(commit_record)
        self.confirmed_revision = commit["revision"]
        self.confirmed_digest = commit["digest"]
        if simulate_cut == "POST_D2_REPLY_LOSS":
            self._unknown("Simulation: Reply loss after modeled barrier D2 acknowledgement")
        self.active = None
        return {"status": "COMMITTED", "tx_id": tx_id,
                "revision": self.confirmed_revision, "digest": self.confirmed_digest}

    @classmethod
    def recover(cls, ledger_id: bytes, boot_floor: int,
                on_disk_slots: Dict[int, Dict[str, bytes]], *,
                model_initialized_context: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Inspect a bounded model chain; context is assumed, not authenticated.

        Context requires exactly ledger_id, checkpoint_revision, checkpoint_digest,
        checkpoint_fence. Slots require exactly integer keys 0..3; each slot has
        only optional 'payload'/'commit' names. Absence is a missing name, never b''.
        Even a valid chain stays mutation-closed: no durable charge codec exists.
        """
        evidence = {"model_only": True, "mutation_admission": False,
                    "charge_recovery": "UNIMPLEMENTED",
                    "accounting_recovered": False, "limitations": MODEL_LIMITATIONS.copy()}
        try:
            sm = cls(ledger_id, boot_floor)
            expected_context = {"ledger_id", "checkpoint_revision", "checkpoint_digest", "checkpoint_fence"}
            if not isinstance(model_initialized_context, dict) or set(model_initialized_context) != expected_context:
                raise CodecError("Explicit model initialized recovery context required with exact fields")
            context = model_initialized_context
            require_bytes("context ledger_id", context["ledger_id"], 32)
            if context["ledger_id"] != ledger_id:
                raise CodecError("Context ledger mismatch")
            sm.initialize_writable(context["checkpoint_revision"], context["checkpoint_digest"],
                                   context["checkpoint_fence"])
            current_rev = sm.confirmed_revision
            current_digest = sm.confirmed_digest
            current_fence = sm.enforcement_fence
            if (not isinstance(on_disk_slots, dict) or
                    any(type(key) is not int for key in on_disk_slots) or
                    set(on_disk_slots) != set(range(4))):
                raise CodecError("Recovery requires exact slot set 0..3")

            candidates = []
            seen_transactions = set()
            for slot_idx, slot in on_disk_slots.items():
                if not isinstance(slot, dict) or not set(slot) <= {"payload", "commit"}:
                    raise CodecError(f"Invalid record name set in slot {slot_idx}")
                if not slot:
                    continue
                if "payload" not in slot:
                    raise CodecError(f"Commit in slot {slot_idx} without payload")
                payload = Ogj1Codec.decode_record(slot["payload"])
                if payload["kind"] != KIND_PAYLOAD or payload["ledger_id"] != ledger_id:
                    raise CodecError("Payload kind/ledger mismatch")
                if payload["tx_id"] in seen_transactions:
                    raise CodecError("Duplicate transaction identity in recovery")
                seen_transactions.add(payload["tx_id"])
                commit = None
                if "commit" in slot:
                    commit = Ogj1Codec.decode_record(slot["commit"])
                    if commit["kind"] != KIND_COMMIT or commit["ledger_id"] != ledger_id:
                        raise CodecError("Commit kind/ledger mismatch")
                    for field in ("tx_id", "revision", "fence", "prev_commit_digest"):
                        if commit[field] != payload[field]:
                            raise CodecError(f"Payload/commit {field} binding mismatch")
                    ref = commit["body"]
                    if ref["referenced_slot"] != slot_idx:
                        raise CodecError("Commit referenced slot mismatch")
                    if ref["referenced_len"] != payload["total_len"]:
                        raise CodecError("Commit referenced payload length mismatch")
                    if ref["referenced_digest"] != payload["digest"]:
                        raise CodecError("Commit referenced payload digest mismatch")
                candidates.append((payload["revision"], slot_idx, payload, commit))

            recovered_commits = []
            orphan_payloads = []
            for revision, slot_idx, payload, commit in sorted(candidates, key=lambda entry: entry[0]):
                if orphan_payloads:
                    raise CodecError("Fork or successor after unresolved orphan")
                if revision != current_rev + 1:
                    raise CodecError(f"Revision non-contiguous: expected {current_rev + 1}, got {revision}")
                if payload["prev_commit_digest"] != current_digest:
                    raise CodecError(f"Digest chain broken at rev {revision}")
                if payload["fence"] < current_fence:
                    raise CodecError(f"Fence regression at rev {revision}")
                entry = {"slot": slot_idx, "tx_id": payload["tx_id"],
                         "revision": revision, "digest": payload["digest"]}
                if commit is None:
                    orphan_payloads.append(entry)
                    continue
                current_rev = revision
                current_digest = commit["digest"]
                current_fence = commit["fence"]
                recovered_commits.append({**entry, "digest": current_digest,
                                          "model_result": "COMMITTED"})
            return {**evidence,
                    "state": "UNKNOWN" if orphan_payloads else "RECOVERED_MUTATION_CLOSED",
                    "confirmed_revision": current_rev, "confirmed_digest": current_digest,
                    "enforcement_fence": current_fence,
                    "recovered_commits": recovered_commits, "orphan_payloads": orphan_payloads,
                    "active_unknown": orphan_payloads[0]["tx_id"].hex() if orphan_payloads else None}
        except CodecError as error:
            return {**evidence, "state": "RECOVERY_FAILED", "error": str(error)}


def run_all_tests() -> Dict[str, Any]:
    receipts = []
    ledger_id = hashlib.sha256(b"cellos-test-ledger-001").digest()
    boot_floor = 100

    # Test 1: Codec encode/decode roundtrip for all 4 kinds
    for kind, name in [(KIND_PAYLOAD, "Payload"), (KIND_COMMIT, "Commit"),
                       (KIND_CHECKPOINT, "Checkpoint"), (KIND_SWITCH, "Switch")]:
        tx_id = hashlib.md5(f"tx-{name}".encode()).digest()
        schema_id = hashlib.sha256(f"schema-{name}".encode()).digest()
        if kind in (KIND_PAYLOAD, KIND_CHECKPOINT):
            body = Ogj1Codec.encode_payload_body(schema_id, b"canonical test payload bytes")
        else:
            body = Ogj1Codec.encode_commit_body(1, 180, hashlib.sha256(b"dummy").digest())

        raw = Ogj1Codec.encode_record(kind, ledger_id, tx_id, 1, 105, b"\x00"*32, body)
        decoded = Ogj1Codec.decode_record(raw)
        assert decoded["magic"] == MAGIC
        assert decoded["version"] == VERSION
        assert decoded["kind"] == kind
        assert decoded["revision"] == 1
        assert decoded["fence"] == 105
    receipts.append({"test": "TC01_roundtrip_all_kinds", "passed": True, "detail": "All 4 kinds encoded and decoded cleanly"})

    # Test 2: Rejection of invalid magic
    raw = Ogj1Codec.encode_record(KIND_PAYLOAD, ledger_id, b"\x01"*16, 1, 100, b"\x00"*32, Ogj1Codec.encode_payload_body(b"\x02"*32, b"test"))
    corrupt_magic = b"BAD1" + raw[4:]
    try:
        Ogj1Codec.decode_record(corrupt_magic)
        assert False, "Should have failed on bad magic"
    except CodecError as e:
        assert "Invalid magic" in str(e)
    receipts.append({"test": "TC02_reject_invalid_magic", "passed": True, "detail": "Corrupt magic rejected"})

    # Test 3: Rejection of unsupported version
    corrupt_ver = raw[:4] + struct.pack("<H", 2) + raw[6:]
    try:
        Ogj1Codec.decode_record(corrupt_ver)
        assert False, "Should have failed on bad version"
    except CodecError as e:
        assert "Unsupported version" in str(e)
    receipts.append({"test": "TC03_reject_unsupported_version", "passed": True, "detail": "Version 2 rejected"})

    # Test 4: Rejection of non-zero reserved byte
    corrupt_res = raw[:7] + b"\x01" + raw[8:]
    try:
        Ogj1Codec.decode_record(corrupt_res)
        assert False, "Should have failed on nonzero reserved"
    except CodecError as e:
        assert "Non-zero reserved" in str(e)
    receipts.append({"test": "TC04_reject_nonzero_reserved", "passed": True, "detail": "Nonzero reserved rejected"})

    # Test 5: Rejection of tampered body (bit flip)
    tampered_body = raw[:HEADER_LEN + 10] + bytes([raw[HEADER_LEN + 10] ^ 0xFF]) + raw[HEADER_LEN + 11:]
    try:
        Ogj1Codec.decode_record(tampered_body)
        assert False, "Should have failed on digest mismatch"
    except CodecError as e:
        assert "Digest verification failure" in str(e)
    receipts.append({"test": "TC05_reject_tampered_body", "passed": True, "detail": "Bit flip in body triggers digest verification failure"})

    # Test 6: Rejection of tampered header (modified revision without digest update)
    tampered_header = raw[:0x3C] + struct.pack("<Q", 999) + raw[0x3C + 8:]
    try:
        Ogj1Codec.decode_record(tampered_header)
        assert False, "Should have failed on digest mismatch"
    except CodecError as e:
        assert "Digest verification failure" in str(e)
    receipts.append({"test": "TC06_reject_tampered_header", "passed": True, "detail": "Modified revision detected by trailer digest"})

    # Test 7: Rejection of truncated record
    try:
        Ogj1Codec.decode_record(raw[:HEADER_LEN + 5])
        assert False, "Should have failed on truncation"
    except CodecError as e:
        assert "truncated" in str(e) or "Total length mismatch" in str(e)
    receipts.append({"test": "TC07_reject_truncated_record", "passed": True, "detail": "Truncated record rejected"})

    # Test 8: Rejection of canonical body > 256 bytes
    try:
        Ogj1Codec.encode_payload_body(b"\x00"*32, b"X" * 257)
        assert False, "Should have failed on payload > 256"
    except CodecError as e:
        assert "exceeds max length" in str(e)
    receipts.append({"test": "TC08_reject_oversized_payload", "passed": True, "detail": "Canonical payload > 256 bytes rejected"})

    # Test 9: Rejection of commit body != 40 bytes
    try:
        Ogj1Codec.decode_body(KIND_COMMIT, b"\x00" * 39)
        assert False, "Should have failed on commit != 40 bytes"
    except CodecError as e:
        assert "Commit body length must be 40" in str(e)
    receipts.append({"test": "TC09_reject_invalid_commit_body_len", "passed": True, "detail": "Commit body != 40 bytes rejected"})

    # Test 10: State Machine & Double-Barrier Invariants
    sm = JournalStateMachine(ledger_id, boot_floor)
    sm.initialize_writable(revision=0, digest=b"\x00"*32, fence=100)
    tx1 = b"\x11" * 16
    charge1 = {"cpu_ticks": 5000, "mem_bytes": 65536, "quota_blocks": 4}

    # Successful D1 and D2
    payload1 = sm.begin_mutation(tx1, 0, charge1, b"\xaa"*32, b"first state change")
    sm.execute_d1(tx1, 0, payload1)
    commit1 = sm.build_commit(tx1, 0)
    res1 = sm.execute_d2(tx1, 0, commit1)
    assert res1["status"] == "COMMITTED"
    assert sm.confirmed_revision == 1
    assert sm.confirmed_digest != b"\x00"*32
    assert sm.retained_charges[tx1] == charge1
    receipts.append({"test": "TC10_successful_double_barrier", "passed": True, "detail": "D1 then D2 advances revision and confirms"})

    # Test 11: D1 failure -> Unknown with FULL charge retained
    tx2 = b"\x22" * 16
    charge2 = {"cpu_ticks": 3000, "mem_bytes": 32768, "quota_blocks": 2}
    payload2 = sm.begin_mutation(tx2, 1, charge2, b"\xbb"*32, b"second state change")
    try:
        sm.execute_d1(tx2, 1, payload2, simulate_cut="D1_BARRIER_FAIL")
    except CodecError:
        pass
    assert sm.state == "UNKNOWN"
    assert sm.active_unknown == tx2
    assert sm.retained_charges[tx2] == charge2, "Full prospective charge MUST be retained on D1 failure"
    receipts.append({"test": "TC11_d1_failure_unknown_charge_retained", "passed": True, "detail": "D1 barrier fail transitions to UNKNOWN, charge retained"})

    # Test 12: Refusal of mutation while in UNKNOWN
    tx3 = b"\x33" * 16
    try:
        sm.begin_mutation(tx3, 2, {"cpu_ticks": 1000}, b"\xcc"*32, b"third")
        assert False, "Should have refused mutation during Unknown"
    except CodecError as e:
        assert "Unknown in-flight" in str(e) or "Cannot mutate in state" in str(e)
    receipts.append({"test": "TC12_refuse_mutation_during_unknown", "passed": True, "detail": "Conflicting mutation refused while Unknown active"})

    # Test 13: Post-D1/pre-D2 crash -> Unknown, full charge and orphan retained
    sm_fresh = JournalStateMachine(ledger_id, boot_floor)
    sm_fresh.initialize_writable(revision=1, digest=sm.confirmed_digest, fence=100)
    tx4 = b"\x44" * 16
    charge4 = {"cpu_ticks": 4000, "mem_bytes": 16384, "quota_blocks": 1}
    payload4 = sm_fresh.begin_mutation(tx4, 2, charge4, b"\xdd"*32, b"fourth state change")
    sm_fresh.execute_d1(tx4, 2, payload4)
    commit4 = sm_fresh.build_commit(tx4, 2)
    try:
        sm_fresh.execute_d2(tx4, 2, commit4, simulate_cut="POST_D1_PRE_D2_CRASH")
    except CodecError:
        pass
    assert sm_fresh.state == "UNKNOWN"
    assert sm_fresh.active_unknown == tx4
    assert sm_fresh.retained_charges[tx4] == charge4
    receipts.append({"test": "TC13_post_d1_pre_d2_crash_charge_retained", "passed": True, "detail": "Post-D1 crash creates orphan payload, retains charge"})

    # Recovery inputs explicitly assume initialization, but cannot recover charges.
    context = {"ledger_id": ledger_id, "checkpoint_revision": 0,
               "checkpoint_digest": b"\x00" * 32, "checkpoint_fence": boot_floor}

    def slots(entries=None):
        return {i: dict((entries or {}).get(i, {})) for i in range(4)}

    def recover(entries, model_context=context):
        return JournalStateMachine.recover(
            ledger_id, boot_floor, entries, model_initialized_context=model_context)

    def receipt(name, detail):
        receipts.append({"test": name, "passed": True, "detail": detail})

    def rejects(action, message=None):
        try:
            action()
        except CodecError as error:
            if message is not None:
                assert message in str(error), str(error)
            return
        raise AssertionError("Expected CodecError")

    def failed(entries, message=None, model_context=context):
        result = recover(entries, model_context)
        assert result["state"] == "RECOVERY_FAILED", result
        assert result["mutation_admission"] is False
        if message is not None:
            assert message in result["error"], result
        return result

    def pair(slot, tx, revision, predecessor, fence=boot_floor, ledger=ledger_id):
        payload = Ogj1Codec.encode_record(
            KIND_PAYLOAD, ledger, tx, revision, fence, predecessor,
            Ogj1Codec.encode_payload_body(b"\xaa" * 32, b"modeled state change"))
        commit = Ogj1Codec.encode_record(
            KIND_COMMIT, ledger, tx, revision, fence, predecessor,
            Ogj1Codec.encode_commit_body(slot, len(payload), payload[-TRAILER_LEN:]))
        return {"payload": payload, "commit": commit}

    def rewrite(record, **changes):
        decoded = Ogj1Codec.decode_record(record)
        values = {key: decoded[key] for key in
                  ("kind", "ledger_id", "tx_id", "revision", "fence", "prev_commit_digest")}
        values["body_bytes"] = record[HEADER_LEN:-TRAILER_LEN]
        values.update(changes)
        return Ogj1Codec.encode_record(**values)

    disk_slots = slots({0: {"payload": payload1, "commit": commit1}})
    rec_result = recover(disk_slots)
    assert rec_result["state"] == "RECOVERED_MUTATION_CLOSED"
    assert rec_result["confirmed_revision"] == 1
    assert len(rec_result["recovered_commits"]) == 1
    assert rec_result["recovered_commits"][0]["tx_id"] == tx1
    assert rec_result["charge_recovery"] == "UNIMPLEMENTED"
    assert rec_result["accounting_recovered"] is False
    assert rec_result["mutation_admission"] is False
    receipt("TC14_recover_contiguous_chain_mutation_closed",
            "Committed model result inspectable; no reconstructed charge or writable admission")

    rec_orphan = recover(slots({0: disk_slots[0], 1: {"payload": payload4}}))
    assert rec_orphan["state"] == "UNKNOWN"
    assert rec_orphan["confirmed_revision"] == 1
    assert len(rec_orphan["orphan_payloads"]) == 1
    assert rec_orphan["active_unknown"] == tx4.hex()
    receipt("TC15_recover_orphan_payload_unknown",
            "Valid next-revision orphan remains Unknown; mutation admission stays closed")

    torn_commit = commit1[:HEADER_LEN + 10] + b"\xFF" * 10
    failed(slots({0: {"payload": payload1, "commit": torn_commit}}))
    receipt("TC16_recover_torn_commit_fails_closed",
            "Present malformed commit is RecoveryFailed, not a valid Unknown orphan")

    gap = pair(1, b"\x99" * 16, 3, rec_result["confirmed_digest"])
    failed(slots({0: disk_slots[0], 1: gap}), "Revision non-contiguous: expected 2, got 3")
    receipt("TC17_recover_revision_gap_fails",
            "Valid-checksum, fully bound pair reaches and fails the revision continuity check")

    regression = pair(1, b"\x99" * 16, 2, rec_result["confirmed_digest"], fence=50)
    failed(slots({0: disk_slots[0], 1: regression}), "Fence regression")
    high = pair(3, b"\x71" * 16, 1, b"\x00" * 32, fence=120)
    lower = pair(0, b"\x72" * 16, 2, high["commit"][-TRAILER_LEN:], fence=110)
    failed(slots({3: high, 0: lower}), "Fence regression")
    failed(slots(), "Fence regression", {**context, "checkpoint_fence": 99})
    checkpoint = {**context, "checkpoint_revision": 7,
                  "checkpoint_digest": hashlib.sha256(b"model-checkpoint").digest(),
                  "checkpoint_fence": 120}
    failed(slots({0: pair(0, b"\x73" * 16, 8, checkpoint["checkpoint_digest"], fence=110)}),
           "Fence regression", checkpoint)
    receipt("TC18_recover_fence_regression_fails",
            "Rejects fence below boot floor, initialized checkpoint, or earlier revision")

    for cut in ("D2_BARRIER_FAIL", "POST_D2_REPLY_LOSS"):
        machine = JournalStateMachine(ledger_id, boot_floor)
        machine.initialize_writable(0, b"\x00" * 32, boot_floor)
        tx = hashlib.md5(cut.encode()).digest()
        charge = {"cpu_ticks": 17, "quota_blocks": 2}
        payload = machine.begin_mutation(tx, 3, charge, b"\xaa" * 32, b"second barrier")
        machine.execute_d1(tx, 3, payload)
        commit = machine.build_commit(tx, 3)
        rejects(lambda: machine.execute_d2(tx, 3, commit, simulate_cut=cut))
        assert machine.state == "UNKNOWN" and machine.active_unknown == tx
        assert machine.retained_charges == {tx: charge}
        assert machine.tx_slots[3] == {"payload": payload, "commit": commit}
        assert machine.confirmed_revision == (0 if cut == "D2_BARRIER_FAIL" else 1)
        rejects(lambda: machine.begin_mutation(b"\xfe" * 16, 0, charge, b"\xaa" * 32, b"conflict"))
        result = recover(machine.tx_slots)
        assert result["state"] == "RECOVERED_MUTATION_CLOSED"
        assert result["recovered_commits"][0]["tx_id"] == tx
        assert result["recovered_commits"][0]["model_result"] == "COMMITTED"
        assert result["accounting_recovered"] is False
        receipt("TC19_" + cut.lower(),
                "Actual modeled D2 cut retains all charge; matching commit inspectable, no durable charge claim")

    for entries in (slots(), disk_slots, slots({0: {"payload": b"corrupt"}})):
        failed(entries, "Explicit model initialized", None)
    assert recover(slots())["state"] == "RECOVERED_MUTATION_CLOSED"
    for bad_context in (
            {}, {**context, "extra": True}, {**context, "ledger_id": b"x" * 32},
            {**context, "ledger_id": b""}, {**context, "checkpoint_revision": -1},
            {**context, "checkpoint_revision": 1 << 64},
            {**context, "checkpoint_revision": True},
            {**context, "checkpoint_digest": b""},
            {**context, "checkpoint_digest": b"x" * 32},
            {**context, "checkpoint_revision": 1},
            {**context, "checkpoint_fence": 1 << 64},
            {**context, "checkpoint_fence": "100"}):
        failed(slots(), model_context=bad_context)
    checkpoint_result = recover(slots(), checkpoint)
    assert checkpoint_result["confirmed_revision"] == 7
    assert checkpoint_result["confirmed_digest"] == checkpoint["checkpoint_digest"]
    assert checkpoint_result["enforcement_fence"] == 120
    receipt("TC20_explicit_bounded_model_initialization",
            "Absent/invalid context fails closed, including empty/corrupt media; checkpoint is only a model assumption")

    for bad_slots in ({}, {0: {}}, {**slots(), 4: {}}, {**slots(), "0": {}},
                      {False: {}, 1: {}, 2: {}, 3: {}},
                      slots({0: {"unexpected": payload1}}), {**slots(), 0: []},
                      slots({0: {"commit": commit1}})):
        failed(bad_slots)
    for name in ("payload", "commit"):
        for bad_raw in (b"", b"broken", None, "", payload1[:-1]):
            slot = {"payload": payload1, name: bad_raw}
            failed(slots({0: slot}))
    for corrupt_payload in (b"", b"broken", payload1[:-1]):
        failed(slots({0: {"payload": corrupt_payload}}))
    receipt("TC21_exact_layout_and_present_corruption",
            "Exact slot/name set enforced; malformed and zero-length present records cannot disappear")

    binding_pair = pair(0, b"\x81" * 16, 1, b"\x00" * 32)
    for name, kind in (("payload", KIND_CHECKPOINT), ("commit", KIND_SWITCH)):
        failed(slots({0: {**binding_pair, name: rewrite(binding_pair[name], kind=kind)}}),
               "kind/ledger mismatch")
    for name in ("payload", "commit"):
        failed(slots({0: {**binding_pair, name: rewrite(binding_pair[name], ledger_id=b"z" * 32)}}),
               "kind/ledger mismatch")
    for field, value in (("tx_id", b"\x82" * 16), ("revision", 2), ("fence", 101),
                         ("prev_commit_digest", b"z" * 32)):
        failed(slots({0: {**binding_pair, "commit": rewrite(binding_pair["commit"], **{field: value})}}),
               field + " binding mismatch")
    decoded_payload = Ogj1Codec.decode_record(binding_pair["payload"])
    for slot_idx, length, digest, message in (
            (1, decoded_payload["total_len"], decoded_payload["digest"], "slot mismatch"),
            (0, decoded_payload["total_len"] + 1, decoded_payload["digest"], "length mismatch"),
            (0, decoded_payload["total_len"], b"z" * 32, "digest mismatch")):
        bad_body = Ogj1Codec.encode_commit_body(slot_idx, length, digest)
        failed(slots({0: {**binding_pair, "commit": rewrite(binding_pair["commit"], body_bytes=bad_body)}}),
               message)
    failed(slots({0: pair(0, b"\x83" * 16, 1, b"z" * 32)}), "Digest chain broken")
    failed(slots({0: {"payload": rewrite(payload1, ledger_id=b"z" * 32)}}), "kind/ledger")
    failed(slots({0: {"payload": rewrite(payload1, kind=KIND_CHECKPOINT)}}), "kind/ledger")
    failed(slots({0: {"payload": rewrite(payload1, fence=99)}}), "Fence regression")
    failed(slots({0: {"payload": rewrite(payload1, revision=2)}}), "Revision non-contiguous")
    failed(slots({0: {"payload": rewrite(payload1, prev_commit_digest=b"z" * 32)}}), "Digest chain broken")
    receipt("TC22_adversarial_valid_checksum_binding",
            "Rechecksummed foreign ledger/kind/T/revision/fence/predecessor and reference slot/length/digest rejected")

    first = pair(3, b"\x91" * 16, 1, b"\x00" * 32)
    second = pair(0, b"\x92" * 16, 2, first["commit"][-TRAILER_LEN:])
    reordered = recover(slots({3: first, 0: second}))
    assert reordered["state"] == "RECOVERED_MUTATION_CLOSED"
    assert [entry["slot"] for entry in reordered["recovered_commits"]] == [3, 0]
    assert reordered["confirmed_revision"] == 2
    receipt("TC23_revision_order_not_slot_order", "Valid chain in slots 3 then 0 is processed by revision")

    duplicate = pair(0, b"\x91" * 16, 2, first["commit"][-TRAILER_LEN:])
    failed(slots({3: first, 0: duplicate}), "Duplicate transaction")
    fork = pair(0, b"\x93" * 16, 1, b"\x00" * 32)
    failed(slots({3: first, 0: fork}), "Revision non-contiguous")
    failed(slots({3: {"payload": first["payload"]}, 0: fork}), "Revision non-contiguous")
    failed(slots({3: {"payload": first["payload"]}, 0: second}), "Fork or successor")
    orphan_duplicate = {"payload": rewrite(second["payload"], tx_id=b"\x91" * 16)}
    failed(slots({3: first, 0: orphan_duplicate}), "Duplicate transaction")
    receipt("TC24_duplicate_T_forks_and_orphan_successor",
            "Valid-checksum duplicate T, competing revisions and unresolved-orphan successors fail closed")

    # Bypass encoder intentionally to exercise decoder validation with valid trailers.
    def wire_record(kind, body, revision=1):
        header = struct.pack("<4s H B B I 32s 16s Q Q 32s I",
                             MAGIC, VERSION, kind, 0, HEADER_LEN + len(body) + TRAILER_LEN,
                             ledger_id, b"\xa1" * 16, revision, boot_floor, b"\x00" * 32, len(body))
        return header + body + Ogj1Codec.compute_digest(header, body)

    body = Ogj1Codec.encode_payload_body(b"\xaa" * 32, b"valid")
    for field in ("revision", "fence"):
        for invalid in (-1, 1 << 64, True, 1.0, None):
            arguments = {"kind": KIND_PAYLOAD, "ledger_id": ledger_id, "tx_id": b"x" * 16,
                         "revision": 1, "fence": 100, "prev_commit_digest": b"\x00" * 32,
                         "body_bytes": body}
            arguments[field] = invalid
            rejects(lambda: Ogj1Codec.encode_record(**arguments))
    rejects(lambda: Ogj1Codec.encode_record(KIND_PAYLOAD, ledger_id, b"x" * 16, 0, 100, b"\x00" * 32, body))
    rejects(lambda: Ogj1Codec.decode_record(wire_record(KIND_PAYLOAD, body, revision=0)))
    boundary = Ogj1Codec.encode_record(KIND_PAYLOAD, ledger_id, b"x" * 16,
                                       UINT64_MAX, UINT64_MAX, b"\x00" * 32, body)
    assert Ogj1Codec.decode_record(boundary)["revision"] == UINT64_MAX
    for kind in (KIND_PAYLOAD, KIND_CHECKPOINT):
        for invalid_body in (b"", b"x" * 35, struct.pack("<32s I", b"x" * 32, 1),
                             struct.pack("<32s I", b"x" * 32, 257) + b"x" * 257):
            rejects(lambda: Ogj1Codec.encode_record(kind, ledger_id, b"x" * 16, 1, 100, b"\x00" * 32, invalid_body))
            rejects(lambda: Ogj1Codec.decode_record(wire_record(kind, invalid_body)))
    for kind in (KIND_COMMIT, KIND_SWITCH):
        for invalid_body in (b"x" * 39, b"x" * 41,
                             struct.pack("<I I 32s", 4, MIN_PAYLOAD_RECORD_LEN, b"x" * 32),
                             struct.pack("<I I 32s", 0, MIN_PAYLOAD_RECORD_LEN - 1, b"x" * 32),
                             struct.pack("<I I 32s", 0, MAX_PAYLOAD_RECORD_LEN + 1, b"x" * 32)):
            rejects(lambda: Ogj1Codec.encode_record(kind, ledger_id, b"x" * 16, 1, 100, b"\x00" * 32, invalid_body))
            rejects(lambda: Ogj1Codec.decode_record(wire_record(kind, invalid_body)))
    for slot_idx in (2, 3):
        switch_body = Ogj1Codec.encode_commit_body(slot_idx, MIN_PAYLOAD_RECORD_LEN, b"x" * 32)
        rejects(lambda: Ogj1Codec.encode_record(KIND_SWITCH, ledger_id, b"x" * 16, 1, 100, b"\x00" * 32, switch_body))
        rejects(lambda: Ogj1Codec.decode_record(wire_record(KIND_SWITCH, switch_body)))
        assert Ogj1Codec.decode_body(KIND_COMMIT, switch_body)["referenced_slot"] == slot_idx
    switch_boundary = Ogj1Codec.encode_commit_body(1, MIN_PAYLOAD_RECORD_LEN, b"x" * 32)
    assert Ogj1Codec.decode_record(wire_record(KIND_SWITCH, switch_boundary))["body"]["referenced_slot"] == 1
    for slot_idx in (-1, 4, True, 1.0):
        rejects(lambda: Ogj1Codec.encode_commit_body(slot_idx, MIN_PAYLOAD_RECORD_LEN, b"x" * 32))
    for length in (-1, 0, MIN_PAYLOAD_RECORD_LEN - 1, MAX_PAYLOAD_RECORD_LEN + 1, 1 << 32, True, 180.0):
        rejects(lambda: Ogj1Codec.encode_commit_body(0, length, b"x" * 32))
    for length in (MIN_PAYLOAD_RECORD_LEN, MAX_PAYLOAD_RECORD_LEN):
        encoded = Ogj1Codec.encode_commit_body(3, length, b"x" * 32)
        assert Ogj1Codec.decode_body(KIND_COMMIT, encoded)["referenced_len"] == length
    receipt("TC25_codec_numeric_and_kind_bounds",
            "Encode/decode reject bad bounds/body/slot/reference lengths with CodecError; valid boundaries accepted")

    machine = JournalStateMachine(ledger_id, boot_floor)
    machine.initialize_writable(0, b"\x00" * 32, boot_floor)
    tx = b"\xb1" * 16
    charge = {"quota_blocks": 4}
    for bad_schema, bad_body in ((b"", b"valid"), (b"x" * 32, b"x" * 257)):
        rejects(lambda: machine.begin_mutation(tx, 0, charge, bad_schema, bad_body))
        assert machine.retained_charges == {} and machine.active is None
    payload = machine.begin_mutation(tx, 0, charge, b"x" * 32, b"valid")
    charge["quota_blocks"] = 0
    assert machine.retained_charges[tx] == {"quota_blocks": 4}
    rejects(lambda: machine.begin_mutation(b"\xb2" * 16, 1, {"quota_blocks": 1}, b"x" * 32, b"second"))
    rejects(lambda: machine.build_commit(tx, 0), "phase")
    rejects(lambda: machine.execute_d2(tx, 0, b""), "phase")
    rejects(lambda: machine.execute_d1(b"\xb2" * 16, 0, payload), "identity")
    rejects(lambda: machine.execute_d1(tx, 1, payload), "identity")
    rejects(lambda: machine.execute_d1(tx, 0, rewrite(payload, tx_id=b"\xb2" * 16)), "reserved encoding")
    assert machine.tx_slots == slots()
    machine.execute_d1(tx, 0, payload)
    rejects(lambda: machine.execute_d1(tx, 0, payload), "phase")
    rejects(lambda: machine.build_commit(b"\xb2" * 16, 0), "identity")
    commit = machine.build_commit(tx, 0)
    rejects(lambda: machine.execute_d2(tx, 1, commit), "identity")
    rejects(lambda: machine.execute_d2(tx, 0, rewrite(commit, tx_id=b"\xb2" * 16)), "binding")
    assert "commit" not in machine.tx_slots[0]
    machine.execute_d2(tx, 0, commit)
    rejects(lambda: machine.execute_d2(tx, 0, commit))
    rejects(lambda: machine.initialize_writable(0, b"\x00" * 32, 100))
    rejects(lambda: machine.begin_mutation(tx, 1, {"quota_blocks": 0}, b"x" * 32, b"overwrite"), "Duplicate")
    rejects(lambda: machine.begin_mutation(b"\xb2" * 16, 0, {"quota_blocks": 1}, b"x" * 32, b"overwrite"), "occupied")
    assert machine.retained_charges == {tx: {"quota_blocks": 4}}
    assert machine.tx_slots[0] == {"payload": payload, "commit": commit}
    receipt("TC26_reserved_identity_and_phase",
            "Pre-encode before charge mutation; one active identity, D1 before D2, no duplicate T/slot overwrite")

    # Every possibly accepted charge survives subsequent uncertainty, even before D1.
    for cut in ("PRE_D1_CRASH", "D1_BARRIER_FAIL", "POST_D1_PRE_D2_CRASH",
                "D2_BARRIER_FAIL", "POST_D2_REPLY_LOSS"):
        machine = JournalStateMachine(ledger_id, boot_floor)
        machine.initialize_writable(0, b"\x00" * 32, 100)
        old_tx, new_tx = b"\xc1" * 16, b"\xc2" * 16
        old_charge, new_charge = {"quota_blocks": 3}, {"quota_blocks": 7}
        old_payload = machine.begin_mutation(old_tx, 0, old_charge, b"x" * 32, b"old")
        machine.execute_d1(old_tx, 0, old_payload)
        machine.execute_d2(old_tx, 0, machine.build_commit(old_tx, 0))
        new_payload = machine.begin_mutation(new_tx, 1, new_charge, b"x" * 32, b"new")
        if cut in ("PRE_D1_CRASH", "D1_BARRIER_FAIL"):
            rejects(lambda: machine.execute_d1(new_tx, 1, new_payload, simulate_cut=cut))
        else:
            machine.execute_d1(new_tx, 1, new_payload)
            new_commit = machine.build_commit(new_tx, 1)
            rejects(lambda: machine.execute_d2(new_tx, 1, new_commit, simulate_cut=cut))
        assert machine.state == "UNKNOWN" and machine.active_unknown == new_tx
        assert machine.retained_charges == {old_tx: old_charge, new_tx: new_charge}
        rejects(lambda: machine.begin_mutation(b"\xc3" * 16, 2, new_charge, b"x" * 32, b"blocked"))
    receipt("TC27_all_possibly_accepted_charges_retained",
            "All five uncertainty cuts preserve prior and active full model charges; no abort/release inference")

    return {
        "status": "PASS",
        "model_only": True,
        "limitations": MODEL_LIMITATIONS.copy(),
        "total_tests": len(receipts),
        "passed": sum(1 for r in receipts if r["passed"]),
        "failed": sum(1 for r in receipts if not r["passed"]),
        "receipts": receipts
    }


def main():
    parser = argparse.ArgumentParser(description="Verify model-only OGJ1 codec/lifecycle regressions (not runtime approval)")
    parser.add_argument("--out", type=str, default=None, help="Output JSON receipt path")
    args = parser.parse_args()

    results = run_all_tests()
    print(f"OGJ1 Codec Verification: {results['passed']}/{results['total_tests']} tests passed.")
    for r in results["receipts"]:
        print(f"  [PASS] {r['test']}: {r['detail']}")

    if args.out:
        with open(args.out, "w") as f:
            json.dump(results, f, indent=2)
        print(f"Wrote receipt to {args.out}")

    if results["failed"] > 0:
        sys.exit(1)
    sys.exit(0)

if __name__ == "__main__":
    main()
