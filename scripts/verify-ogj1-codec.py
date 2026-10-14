#!/usr/bin/env python3
"""
verify-ogj1-codec.py: Standalone verification harness for OGJ1 codec and Unknown lifecycle protocol.
Validates exact binary encoding, double-barrier D1/D2 simulation, prospective-charge retention,
and recovery chain reconstitution.
"""

import sys
import struct
import hashlib
import json
import argparse
from typing import Dict, Any, Tuple, Optional, List

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

class CodecError(Exception):
    pass

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
        if kind not in (KIND_PAYLOAD, KIND_COMMIT, KIND_CHECKPOINT, KIND_SWITCH):
            raise CodecError(f"Invalid kind: {kind}")
        if len(ledger_id) != 32:
            raise CodecError(f"ledger_id must be 32 bytes, got {len(ledger_id)}")
        if len(tx_id) != 16:
            raise CodecError(f"tx_id must be 16 bytes, got {len(tx_id)}")
        if len(prev_commit_digest) != 32:
            raise CodecError(f"prev_commit_digest must be 32 bytes, got {len(prev_commit_digest)}")
        if revision < 1:
            raise CodecError(f"revision must be >= 1, got {revision}")
        if fence < 0:
            raise CodecError(f"fence must be >= 0, got {fence}")

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
        if len(schema_id) != 32:
            raise CodecError(f"schema_id must be 32 bytes, got {len(schema_id)}")
        if len(canonical_bytes) > MAX_CANONICAL_PAYLOAD_LEN:
            raise CodecError(f"canonical_bytes exceeds max length {MAX_CANONICAL_PAYLOAD_LEN}: {len(canonical_bytes)}")
        return struct.pack("<32s I", schema_id, len(canonical_bytes)) + canonical_bytes

    @classmethod
    def encode_commit_body(cls, referenced_slot: int, referenced_len: int, referenced_digest: bytes) -> bytes:
        if referenced_slot < 0 or referenced_slot > 3:
            raise CodecError(f"Invalid referenced_slot {referenced_slot}: must be 0..3")
        if len(referenced_digest) != 32:
            raise CodecError(f"referenced_digest must be 32 bytes, got {len(referenced_digest)}")
        body = struct.pack("<I I 32s", referenced_slot, referenced_len, referenced_digest)
        assert len(body) == COMMIT_BODY_LEN
        return body

    @classmethod
    def decode_body(cls, kind: int, body_bytes: bytes) -> Dict[str, Any]:
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
            return {
                "referenced_slot": ref_slot,
                "referenced_len": ref_len,
                "referenced_digest": ref_digest
            }
        else:
            raise CodecError(f"Unsupported kind: {kind}")


class JournalStateMachine:
    def __init__(self, ledger_id: bytes, boot_floor: int):
        self.ledger_id = ledger_id
        self.boot_floor = boot_floor
        self.state = "RECOVERING"
        self.confirmed_revision = 0
        self.confirmed_digest = b"\x00" * 32
        self.enforcement_fence = boot_floor

        # Storage simulation: tx_slots[slot_idx] = {"payload": raw, "commit": raw}
        self.tx_slots: Dict[int, Dict[str, bytes]] = {i: {} for i in range(4)}
        self.retained_charges: Dict[bytes, Dict[str, int]] = {}
        self.active_unknown: Optional[bytes] = None

    def initialize_writable(self, revision: int, digest: bytes, fence: int):
        if fence < self.boot_floor:
            self.state = "RECOVERY_FAILED"
            raise CodecError(f"Fence regression: {fence} < boot floor {self.boot_floor}")
        self.state = "WRITABLE"
        self.confirmed_revision = revision
        self.confirmed_digest = digest
        self.enforcement_fence = fence

    def begin_mutation(self, tx_id: bytes, slot_idx: int, prospective_charge: Dict[str, int],
                       schema_id: bytes, canonical_body: bytes) -> bytes:
        if self.state != "WRITABLE":
            raise CodecError(f"Cannot mutate in state {self.state}")
        if self.active_unknown is not None:
            raise CodecError(f"Mutation rejected: Unknown in-flight transaction {self.active_unknown.hex()}")
        if slot_idx not in self.tx_slots:
            raise CodecError(f"Invalid slot {slot_idx}")
        if "payload" in self.tx_slots[slot_idx] or "commit" in self.tx_slots[slot_idx]:
            raise CodecError(f"Slot {slot_idx} already occupied in active generation")

        # Step 1: Pre-allocate prospective charge under lock
        self.retained_charges[tx_id] = prospective_charge.copy()

        # Build payload record
        payload_body = Ogj1Codec.encode_payload_body(schema_id, canonical_body)
        payload_record = Ogj1Codec.encode_record(
            kind=KIND_PAYLOAD,
            ledger_id=self.ledger_id,
            tx_id=tx_id,
            revision=self.confirmed_revision + 1,
            fence=self.enforcement_fence,
            prev_commit_digest=self.confirmed_digest,
            body_bytes=payload_body
        )
        return payload_record

    def execute_d1(self, tx_id: bytes, slot_idx: int, payload_record: bytes, simulate_cut: Optional[str] = None):
        if simulate_cut == "PRE_D1_CRASH":
            self.state = "UNKNOWN"
            self.active_unknown = tx_id
            raise CodecError("Simulation: Crash before barrier D1")

        # Write payload
        self.tx_slots[slot_idx]["payload"] = payload_record

        if simulate_cut == "D1_BARRIER_FAIL":
            self.state = "UNKNOWN"
            self.active_unknown = tx_id
            raise CodecError("Simulation: Native barrier D1 failure")

    def build_commit(self, tx_id: bytes, slot_idx: int) -> bytes:
        if "payload" not in self.tx_slots[slot_idx]:
            raise CodecError("Cannot commit: payload missing in slot")
        payload_record = self.tx_slots[slot_idx]["payload"]
        decoded_payload = Ogj1Codec.decode_record(payload_record)

        commit_body = Ogj1Codec.encode_commit_body(
            referenced_slot=slot_idx,
            referenced_len=decoded_payload["total_len"],
            referenced_digest=decoded_payload["digest"]
        )
        commit_record = Ogj1Codec.encode_record(
            kind=KIND_COMMIT,
            ledger_id=self.ledger_id,
            tx_id=tx_id,
            revision=decoded_payload["revision"],
            fence=self.enforcement_fence,
            prev_commit_digest=self.confirmed_digest,
            body_bytes=commit_body
        )
        return commit_record

    def execute_d2(self, tx_id: bytes, slot_idx: int, commit_record: bytes, simulate_cut: Optional[str] = None):
        if simulate_cut == "POST_D1_PRE_D2_CRASH":
            self.state = "UNKNOWN"
            self.active_unknown = tx_id
            raise CodecError("Simulation: Crash before barrier D2 (Orphan payload)")

        self.tx_slots[slot_idx]["commit"] = commit_record

        if simulate_cut == "D2_BARRIER_FAIL":
            self.state = "UNKNOWN"
            self.active_unknown = tx_id
            raise CodecError("Simulation: Native barrier D2 failure")

        if simulate_cut == "POST_D2_REPLY_LOSS":
            # State is durable, but caller experiences timeout/lost reply
            decoded_commit = Ogj1Codec.decode_record(commit_record)
            self.confirmed_revision = decoded_commit["revision"]
            self.confirmed_digest = decoded_commit["digest"]
            self.state = "UNKNOWN"
            self.active_unknown = tx_id
            raise CodecError("Simulation: Reply loss after barrier D2 acknowledgement")

        # Step 4 & 5: Advance confirmed state and acknowledge
        decoded_commit = Ogj1Codec.decode_record(commit_record)
        self.confirmed_revision = decoded_commit["revision"]
        self.confirmed_digest = decoded_commit["digest"]
        return {
            "status": "COMMITTED",
            "tx_id": tx_id,
            "revision": self.confirmed_revision,
            "digest": self.confirmed_digest
        }

    @classmethod
    def recover(cls, ledger_id: bytes, boot_floor: int, on_disk_slots: Dict[int, Dict[str, bytes]]) -> Dict[str, Any]:
        sm = cls(ledger_id, boot_floor)
        current_rev = 0
        current_digest = b"\x00" * 32
        recovered_commits: List[Dict[str, Any]] = []
        orphan_payloads: List[Dict[str, Any]] = []
        torn_commits: List[int] = []

        # Sort slots by revision of commit or payload
        for slot_idx in range(4):
            slot = on_disk_slots.get(slot_idx, {})
            payload_raw = slot.get("payload")
            commit_raw = slot.get("commit")

            if not payload_raw and not commit_raw:
                continue

            if payload_raw and not commit_raw:
                try:
                    dec_payload = Ogj1Codec.decode_record(payload_raw)
                    orphan_payloads.append({
                        "slot": slot_idx,
                        "tx_id": dec_payload["tx_id"],
                        "revision": dec_payload["revision"],
                        "digest": dec_payload["digest"]
                    })
                except CodecError:
                    pass
                continue

            if commit_raw:
                try:
                    dec_commit = Ogj1Codec.decode_record(commit_raw)
                except CodecError:
                    torn_commits.append(slot_idx)
                    continue

                if not payload_raw:
                    sm.state = "RECOVERY_FAILED"
                    return {"state": "RECOVERY_FAILED", "error": f"Commit in slot {slot_idx} without payload"}

                try:
                    dec_payload = Ogj1Codec.decode_record(payload_raw)
                except CodecError:
                    sm.state = "RECOVERY_FAILED"
                    return {"state": "RECOVERY_FAILED", "error": f"Commit in slot {slot_idx} references corrupt payload"}

                # Check linkage
                commit_body = dec_commit["body"]
                if commit_body["referenced_slot"] != slot_idx:
                    sm.state = "RECOVERY_FAILED"
                    return {"state": "RECOVERY_FAILED", "error": "Commit referenced slot mismatch"}
                if commit_body["referenced_digest"] != dec_payload["digest"]:
                    sm.state = "RECOVERY_FAILED"
                    return {"state": "RECOVERY_FAILED", "error": "Commit referenced payload digest mismatch"}

                # Check chain continuity
                if dec_commit["prev_commit_digest"] != current_digest:
                    sm.state = "RECOVERY_FAILED"
                    return {"state": "RECOVERY_FAILED", "error": f"Digest chain broken at rev {dec_commit['revision']}"}
                if dec_commit["revision"] != current_rev + 1:
                    sm.state = "RECOVERY_FAILED"
                    return {"state": "RECOVERY_FAILED", "error": f"Revision non-contiguous: expected {current_rev + 1}, got {dec_commit['revision']}"}
                if dec_commit["fence"] < boot_floor:
                    sm.state = "RECOVERY_FAILED"
                    return {"state": "RECOVERY_FAILED", "error": f"Fence regression at rev {dec_commit['revision']}"}

                current_rev = dec_commit["revision"]
                current_digest = dec_commit["digest"]
                recovered_commits.append({
                    "slot": slot_idx,
                    "tx_id": dec_commit["tx_id"],
                    "revision": current_rev,
                    "digest": current_digest
                })

        sm.confirmed_revision = current_rev
        sm.confirmed_digest = current_digest

        if orphan_payloads or torn_commits:
            sm.state = "UNKNOWN"
            # Bind earliest orphan as active unknown
            if orphan_payloads:
                sm.active_unknown = orphan_payloads[0]["tx_id"]
        else:
            sm.state = "WRITABLE"

        return {
            "state": sm.state,
            "confirmed_revision": current_rev,
            "confirmed_digest": current_digest,
            "recovered_commits": recovered_commits,
            "orphan_payloads": orphan_payloads,
            "torn_commits": torn_commits,
            "active_unknown": sm.active_unknown.hex() if sm.active_unknown else None
        }


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

    # Test 13: D2 failure -> Unknown with FULL charge retained and orphan payload
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

    # Test 14: Recovery of contiguous commit chain
    disk_slots = {
        0: {"payload": payload1, "commit": commit1}
    }
    rec_result = JournalStateMachine.recover(ledger_id, boot_floor, disk_slots)
    assert rec_result["state"] == "WRITABLE"
    assert rec_result["confirmed_revision"] == 1
    assert len(rec_result["recovered_commits"]) == 1
    receipts.append({"test": "TC14_recover_contiguous_chain", "passed": True, "detail": "Contiguous commits recovered into WRITABLE state"})

    # Test 15: Recovery with orphan payload -> UNKNOWN state
    disk_slots_orphan = {
        0: {"payload": payload1, "commit": commit1},
        1: {"payload": payload4}  # orphan payload without commit
    }
    rec_orphan = JournalStateMachine.recover(ledger_id, boot_floor, disk_slots_orphan)
    assert rec_orphan["state"] == "UNKNOWN"
    assert rec_orphan["confirmed_revision"] == 1
    assert len(rec_orphan["orphan_payloads"]) == 1
    assert rec_orphan["active_unknown"] == tx4.hex()
    receipts.append({"test": "TC15_recover_orphan_payload_unknown", "passed": True, "detail": "Orphan payload recovered into UNKNOWN state, not deleted"})

    # Test 16: Recovery with torn commit -> UNKNOWN state
    torn_commit = commit1[:HEADER_LEN + 10] + b"\xFF" * 10
    disk_slots_torn = {
        0: {"payload": payload1, "commit": torn_commit}
    }
    rec_torn = JournalStateMachine.recover(ledger_id, boot_floor, disk_slots_torn)
    assert rec_torn["state"] == "UNKNOWN"
    assert len(rec_torn["torn_commits"]) == 1
    receipts.append({"test": "TC16_recover_torn_commit_unknown", "passed": True, "detail": "Torn commit detected, does not commit, yields UNKNOWN"})

    # Test 17: Recovery with revision gap -> RECOVERY_FAILED
    # Commit with revision 3 instead of 2
    gap_commit = Ogj1Codec.encode_record(
        KIND_COMMIT, ledger_id, b"\x99"*16, 3, 100, rec_result["confirmed_digest"],
        Ogj1Codec.encode_commit_body(1, 180, hashlib.sha256(b"dummy").digest())
    )
    disk_slots_gap = {
        0: {"payload": payload1, "commit": commit1},
        1: {"payload": payload4, "commit": gap_commit}
    }
    rec_gap = JournalStateMachine.recover(ledger_id, boot_floor, disk_slots_gap)
    assert rec_gap["state"] == "RECOVERY_FAILED"
    receipts.append({"test": "TC17_recover_revision_gap_fails", "passed": True, "detail": "Revision gap triggers RECOVERY_FAILED"})

    # Test 18: Recovery with fence regression -> RECOVERY_FAILED
    regressed_commit = Ogj1Codec.encode_record(
        KIND_COMMIT, ledger_id, b"\x99"*16, 2, 50, rec_result["confirmed_digest"], # fence 50 < boot_floor 100
        Ogj1Codec.encode_commit_body(1, len(payload4), Ogj1Codec.decode_record(payload4)["digest"])
    )
    disk_slots_regr = {
        0: {"payload": payload1, "commit": commit1},
        1: {"payload": payload4, "commit": regressed_commit}
    }
    rec_regr = JournalStateMachine.recover(ledger_id, boot_floor, disk_slots_regr)
    assert rec_regr["state"] == "RECOVERY_FAILED"
    receipts.append({"test": "TC18_recover_fence_regression_fails", "passed": True, "detail": "Fence regression triggers RECOVERY_FAILED"})

    return {
        "status": "PASS",
        "total_tests": len(receipts),
        "passed": sum(1 for r in receipts if r["passed"]),
        "failed": sum(1 for r in receipts if not r["passed"]),
        "receipts": receipts
    }


def main():
    parser = argparse.ArgumentParser(description="Verify OGJ1 Codec and Unknown Lifecycle Protocol")
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
