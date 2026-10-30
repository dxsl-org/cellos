#!/usr/bin/env python3
"""Host tests for scripts/kssh_records.py: `python3 scripts/test_kssh_records.py`.

Test material is synthetic (fake keys, fake ciphertext); nothing secret.
"""
import contextlib
import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kssh_records as kr  # noqa: E402

# Goldens of cells/services/kms/src/tests/ssh/persist/record.rs, computed there
# independently in Python from the public dev root constant.
GOLDEN_PRESENT = bytes.fromhex((
    "434b4d530200010101000000000000000000000000000000000000000000000000000000000000000000000000000000"
    "06676f6c64656e0000000000000000000000000000000000000000000000000000000000000000000000000000000000"
    "000000000000000000000000000000000002b33eaef37ea2df7caa010defdea34e241f65f1b529a4f43ed14327f5c54a"
    "ab6210000000104a1fdeae8d6ba607afd69d334f8d379a00000000000000000000000000000000000000000000000000"
    "0000000000000000000000000000000000000000000000a0005cef6efc85321022c013971749a151469c3d408c6578af"
    "1b6ba9d9c8449c0c356fa04b44fc966d8e5c0b69fd8200f88af275cd2b6ceac0608f289740045ecab3f7e66c98bd3fce"
    "201fcda7064cff120937b307494ce5917e1a741ae06b77fb192e2ebf80a13a8c42e17b04e47b31b46b2ad57f23cea2e5"
    "65398e73191c12779841ec6b51c6d03f4a3e80be5a28af1c4208889577836d18ea85ce3e878105ea5698cdb0ad4c4143"
    "6a2bfb9785a7493ac9bb8ad61d7074ed4aeb92bbccdb64a302"
))
GOLDEN_TOMBSTONE = bytes.fromhex((
    "434b4d530201010002000000000000008e86847756551d61e0749ddb92ea15db032032e2d054311a0ec34cccd3210141"
    "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
    "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
    "000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
    "00000000000000000000000000000000000000000000000000def7d5addced45376f96217c53dfdd8fc4fe918f050b85"
    "82a15cbaefd247bfa2"
))
GOLDEN_KEY = "e6ee4e76ccf6c77fd5922dbeeecf818b9d7a34f58e679700c3c757749c9c16ce"
GOLDEN_PRESENT_DIGEST = "8e86847756551d61e0749ddb92ea15db032032e2d054311a0ec34cccd3210141"
GOLDEN_TOMBSTONE_DIGEST = "03196182986a66da13cc5a8fba52c6f3b0a5b1892bc64ab13ca7251d2aacc1fd"

PUB = bytes(range(1, 33))
SALT = bytes([0x11]) * 16
CT = bytes(range(256))[:160]


class Raw:
    """Every field of a record, so a test can break exactly one. `bytes()`
    carries a correct MAC over whatever the fields say (unless `key` differs)."""

    def __init__(self, **kw):
        self.magic = b"CKMS"
        self.version = 2
        self.slot = 0
        self.kind = 1
        self.state = 1
        self.revision = 1
        self.previous = bytes(32)
        self.name_len = 4
        self.name = b"ka16" + bytes(60)
        self.custody = 2
        self.public = PUB
        self.rounds = 16
        self.salt_len = 16
        self.salt = SALT + bytes(48)
        self.ct = CT
        self.ct_len = None  # None: len(ct)
        for k, v in kw.items():
            if not hasattr(self, k):
                raise AttributeError(k)
            setattr(self, k, v)

    @classmethod
    def tombstone(cls, **kw):
        base = dict(state=0, name_len=0, name=bytes(64), custody=0, public=bytes(32),
                    rounds=0, salt_len=0, salt=bytes(64), ct=b"", revision=2)
        base.update(kw)
        return cls(**base)

    def body(self):
        ct_len = len(self.ct) if self.ct_len is None else self.ct_len
        return b"".join((
            self.magic, bytes([self.version, self.slot, self.kind, self.state]),
            self.revision.to_bytes(8, "little"), self.previous, bytes([self.name_len]),
            self.name, bytes([self.custody]), self.public, self.rounds.to_bytes(4, "little"),
            bytes([self.salt_len]), self.salt, ct_len.to_bytes(2, "little"), self.ct))

    def bytes(self, key=None):
        body = self.body()
        if key is None:
            return body + kr._mac(body)
        return body + hashlib.blake2s(body, key=key, digest_size=32).digest()


def accepts(raw, slot=None):
    try:
        kr.decode_record(raw.bytes(), slot)
        return True
    except kr.RecordError:
        return False


def strict_reason(data, slot=None):
    try:
        kr.decode_record(data, slot)
    except kr.RecordError as e:
        return e.kind, str(e)
    return None


def r1_bytes(**kw):
    args = dict(slot=0, state=1, revision=1, name="ka16", custody=2, public_key=PUB,
                kdf_rounds=16, salt=SALT, ciphertext=CT)
    args.update(kw)
    return kr.encode_record(**args)


def r2_bytes(r1, **kw):
    args = dict(slot=1, state=0, revision=2, previous=kr.digest(r1))
    args.update(kw)
    return kr.encode_record(**args)


RECORD_EXP = {"name": "ka16", "pub": PUB.hex(), "rounds": 16, "salt": SALT.hex(), "ct": CT.hex()}


class WithTmp(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="kssh-records-test-")
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def image(self, plants, size=300_000, name="img"):
        path = os.path.join(self.tmp, name)
        with open(path, "wb") as f:
            f.truncate(size)
            for off, data in plants:
                f.seek(off)
                f.write(data)
            f.truncate(size)
        return path


class Golden(unittest.TestCase):
    def test_mac_key_is_the_blake2s_of_the_public_label(self):
        self.assertEqual(kr.mac_key().hex(), GOLDEN_KEY)
        self.assertEqual(kr.DEV_ROOT_LABEL, b"CELLOS-KSSH-T1-DEV-ROOT-v1 PUBLIC CONSTANT NOT A SECRET")

    def test_golden_present_decodes_and_re_encodes_byte_for_byte(self):
        self.assertEqual(len(GOLDEN_PRESENT), 409)
        rec = kr.decode_record(GOLDEN_PRESENT, 0)
        self.assertEqual((rec["slot"], rec["state"], rec["revision"], rec["custody"]), (0, 1, 1, 2))
        self.assertEqual(rec["previous"], bytes(32))
        self.assertEqual(rec["name"], "golden")
        self.assertEqual(rec["kdf_rounds"], 16)
        self.assertEqual(len(rec["salt"]), 16)
        self.assertEqual(len(rec["ciphertext"]), 160)
        self.assertEqual(rec["digest"].hex(), GOLDEN_PRESENT_DIGEST)
        again = kr.encode_record(slot=0, state=1, revision=1, name="golden", custody=2,
                                 public_key=rec["public_key"], kdf_rounds=16, salt=rec["salt"],
                                 ciphertext=rec["ciphertext"])
        self.assertEqual(again, GOLDEN_PRESENT)

    def test_golden_layout_offsets(self):
        g = GOLDEN_PRESENT
        self.assertEqual(g[:4], b"CKMS")
        self.assertEqual(g[4:8], bytes([2, 0, 1, 1]))
        self.assertEqual(g[8:16], (1).to_bytes(8, "little"))
        self.assertEqual(g[48], 6)
        self.assertEqual(g[49:55], b"golden")
        self.assertEqual(g[113], 2)
        self.assertEqual(g[146:150], (16).to_bytes(4, "little"))
        self.assertEqual(g[150], 16)
        self.assertEqual(g[215:217], (160).to_bytes(2, "little"))
        self.assertEqual(g[217:217 + 160], kr.decode_record(g)["ciphertext"])
        body, tag = g[:-32], g[-32:]
        self.assertEqual(hashlib.blake2s(body, key=bytes.fromhex(GOLDEN_KEY), digest_size=32).digest(), tag)

    def test_golden_tombstone_chains_onto_the_golden_present(self):
        self.assertEqual(len(GOLDEN_TOMBSTONE), 249)
        enc = kr.encode_record(slot=1, state=0, revision=2, previous=bytes.fromhex(GOLDEN_PRESENT_DIGEST))
        self.assertEqual(enc, GOLDEN_TOMBSTONE)
        rec = kr.decode_record(GOLDEN_TOMBSTONE, 1)
        self.assertEqual((rec["slot"], rec["state"], rec["revision"]), (1, 0, 2))
        self.assertEqual(rec["previous"].hex(), GOLDEN_PRESENT_DIGEST)
        self.assertEqual(rec["digest"].hex(), GOLDEN_TOMBSTONE_DIGEST)
        self.assertEqual((rec["name"], rec["salt"], rec["ciphertext"]), ("", b"", b""))

    def test_expect_slot(self):
        kr.decode_record(GOLDEN_PRESENT, 0)
        kr.decode_record(GOLDEN_PRESENT, None)
        with self.assertRaises(kr.RecordError) as cm:
            kr.decode_record(GOLDEN_PRESENT, 1)
        self.assertEqual(cm.exception.kind, "strict")

    def test_round_trip_preserves_every_field(self):
        prev = bytes([7]) * 32
        data = kr.encode_record(slot=1, state=1, revision=9, previous=prev, name="a-b_C9", custody=2,
                                public_key=PUB, kdf_rounds=64, salt=bytes([5]) * 64, ciphertext=bytes([9]) * 2048)
        self.assertEqual(len(data), kr.RECORD_MAX)
        rec = kr.decode_record(data, 1)
        self.assertEqual((rec["slot"], rec["state"], rec["revision"], rec["previous"], rec["name"]),
                         (1, 1, 9, prev, "a-b_C9"))
        self.assertEqual((rec["public_key"], rec["kdf_rounds"], rec["salt"], rec["ciphertext"]),
                         (PUB, 64, bytes([5]) * 64, bytes([9]) * 2048))
        self.assertEqual(rec["digest"], hashlib.blake2s(data, digest_size=32).digest())

    def test_raw_helper_agrees_with_the_encoder(self):
        self.assertEqual(Raw().bytes(), r1_bytes())
        self.assertEqual(Raw.tombstone(slot=1, previous=bytes([3]) * 32).bytes(),
                         kr.encode_record(slot=1, state=0, revision=2, previous=bytes([3]) * 32))


class Bounds(unittest.TestCase):
    def test_ciphertext_length_bounds(self):
        for n, ok in ((0, False), (16, False), (128, False), (143, False), (144, True), (145, False),
                      (152, False), (160, True), (2032, True), (2048, True), (2064, False)):
            raw = Raw(ct=bytes([0xAB]) * n)
            if n <= 2048:
                self.assertEqual(accepts(raw), ok, n)
        self.assertEqual(len(Raw(ct=bytes(2048)).bytes()), 2297)
        # beyond the record bound the decoder refuses on length before any MAC work
        self.assertEqual(strict_reason(Raw(ct=bytes(2064)).bytes())[0], "length")

    def test_length_window(self):
        good = Raw().bytes()
        self.assertEqual(strict_reason(b"")[0], "length")
        self.assertEqual(strict_reason(good[:248])[0], "length")
        self.assertEqual(strict_reason(good + bytes(2300))[0], "length")
        self.assertEqual(strict_reason(good + b"\0")[0], "mac")  # trailing byte shifts the tag

    def test_ct_len_field_must_match_the_record_length(self):
        for field in (144, 160 + 16, 0, 65535):
            r = Raw(ct_len=field)
            kind, _ = strict_reason(r.bytes())
            self.assertEqual(kind, "strict", field)
        self.assertIsNone(strict_reason(Raw(ct_len=160).bytes()))

    def test_kdf_rounds_and_salt_bounds(self):
        for rounds, ok in ((0, False), (1, True), (16, True), (64, True), (65, False), (2 ** 32 - 1, False)):
            self.assertEqual(accepts(Raw(rounds=rounds)), ok, rounds)
        for n, ok in ((0, False), (15, False), (16, True), (17, True), (63, True), (64, True), (65, False), (255, False)):
            salt = bytes([0x22]) * min(n, 64) + bytes(64 - min(n, 64))
            self.assertEqual(accepts(Raw(salt_len=n, salt=salt)), ok, n)
        self.assertFalse(accepts(Raw(salt=SALT + bytes(47) + b"\x01")))  # padding byte

    def test_name_rules(self):
        def with_name(name, name_len=None):
            return Raw(name_len=len(name) if name_len is None else name_len, name=name + bytes(64 - len(name)))
        self.assertTrue(accepts(with_name(b"a")))
        self.assertTrue(accepts(with_name(b"Z" * 64)))
        self.assertTrue(accepts(with_name(b"A_z-09")))
        self.assertFalse(accepts(with_name(b"", 0)))
        for bad in (b"a b", b"a.b", b"a/b", b"a\x00b", b"\xc3\xa9", b"a\n", b"k:1"):
            self.assertFalse(accepts(with_name(bad)), bad)
        self.assertFalse(accepts(with_name(b"ka16", 65)))
        self.assertFalse(accepts(with_name(b"ka16", 3)))   # name byte left in the padding
        self.assertFalse(accepts(Raw(name=b"ka16" + bytes(59) + b"\x01")))  # nonzero padding

    def test_custody_state_revision(self):
        for c, ok in ((0, False), (1, False), (2, True), (3, False), (255, False)):
            self.assertEqual(accepts(Raw(custody=c)), ok, c)
        for s in (2, 3, 255):
            self.assertFalse(accepts(Raw(state=s)), s)
        self.assertFalse(accepts(Raw(revision=0)))
        self.assertTrue(accepts(Raw(revision=2 ** 64 - 1)))

    def test_magic_version_slot_kind(self):
        self.assertFalse(accepts(Raw(magic=b"CKMX")))
        for v in (0, 1, 3):
            self.assertFalse(accepts(Raw(version=v)), v)
        for s, ok in ((0, True), (1, True), (2, False), (255, False)):
            self.assertEqual(accepts(Raw(slot=s)), ok, s)
        for k in (0, 2, 255):
            self.assertFalse(accepts(Raw(kind=k)), k)

    def test_tombstone_rules(self):
        self.assertTrue(accepts(Raw.tombstone(slot=1)))
        self.assertTrue(accepts(Raw.tombstone(slot=0)))
        # state 0 but a value field set (re-MAC'd): strict failure, not MAC failure
        mutations = {
            "name_len": dict(name_len=4),
            "name byte": dict(name=b"ka16" + bytes(60)),
            "custody": dict(custody=2),
            "custody plain": dict(custody=1),
            "public": dict(public=PUB),
            "rounds": dict(rounds=1),
            "salt_len": dict(salt_len=16),
            "salt byte": dict(salt=SALT + bytes(48)),
            "ct": dict(ct=bytes(16)),
        }
        for what, kw in mutations.items():
            kind_reason = strict_reason(Raw.tombstone(**kw).bytes())
            self.assertEqual(kind_reason[0], "strict", what)
        # a present state with the tombstone's zero fields is refused as well
        self.assertEqual(strict_reason(Raw.tombstone(state=1).bytes())[0], "strict")

    def test_plain_custody_re_macd_is_strict_not_mac(self):
        kind, reason = strict_reason(Raw(custody=1).bytes())
        self.assertEqual(kind, "strict")
        self.assertIn("custody", reason)


class MacCoverage(unittest.TestCase):
    def test_flipping_any_byte_without_re_mac_is_a_mac_failure(self):
        for good in (GOLDEN_PRESENT, GOLDEN_TOMBSTONE, r1_bytes()):
            for i in range(len(good)):
                bad = bytearray(good)
                bad[i] ^= 0x01
                kind, _ = strict_reason(bytes(bad))
                self.assertEqual(kind, "mac", i)

    def test_wrong_key_mac_is_a_mac_failure(self):
        other = hashlib.blake2s(b"another root").digest()
        self.assertEqual(strict_reason(Raw().bytes(key=other))[0], "mac")
        self.assertEqual(strict_reason(Raw().bytes(key=bytes(32)))[0], "mac")

    def test_mac_is_checked_before_fields_are_trusted(self):
        # An invalid field AND a bad MAC reports the MAC first (nothing trusted).
        data = bytearray(Raw(custody=1).bytes())
        data[-1] ^= 1
        self.assertEqual(strict_reason(bytes(data))[0], "mac")

    def test_per_field_flip_with_re_mac(self):
        """One flipped byte per header field, MAC recomputed: fields with a rule
        fail strictly, free fields are accepted (and must not be MAC failures)."""
        base = Raw(revision=2)  # 2 ^ 1 = 3: the revision flip stays >= 1
        fields = [  # (name, offset, expected outcome)
            ("magic", 0, "strict"), ("version", 4, "strict"), ("slot", 5, None),
            ("kind", 6, "strict"), ("state", 7, "strict"), ("revision", 8, None),
            ("revision hi", 15, None), ("previous", 16, None), ("previous last", 47, None),
            ("name_len", 48, "strict"), ("name first", 49, None), ("name pad", 60, "strict"),
            ("name pad last", 112, "strict"), ("custody", 113, "strict"), ("public", 114, None),
            ("public last", 145, None), ("rounds", 146, None), ("rounds hi", 149, "strict"),
            ("salt_len", 150, None), ("salt", 151, None), ("salt pad", 167, "strict"),
            ("salt pad last", 214, "strict"), ("ct_len", 215, "strict"), ("ct_len hi", 216, "strict"),
            ("ct first", 217, None), ("ct last", 217 + 159, None)]
        data = base.bytes()
        for what, off, want in fields:
            bad = bytearray(data[:-32])
            bad[off] ^= 0x01
            # name byte 0x40 ('@'-flip of 'k' -> 'j' stays valid; flip 0x01 of 'k' 0x6b -> 0x6a 'j')
            body = bytes(bad)
            res = strict_reason(body + kr._mac(body))
            if want is None:
                self.assertIsNone(res, what)
            else:
                self.assertEqual(res[0] if res else None, want, what)
            # and without the re-MAC the same flip is always a MAC failure
            self.assertEqual(strict_reason(body + data[-32:])[0], "mac", what)


class Predict(unittest.TestCase):
    def test_predict_r1_is_the_encoder_with_the_fixed_fields(self):
        self.assertEqual(kr.predict_r1(RECORD_EXP), r1_bytes())
        rec = kr.decode_record(kr.predict_r1(RECORD_EXP), 0)
        self.assertEqual((rec["slot"], rec["state"], rec["revision"], rec["previous"], rec["custody"]),
                         (0, 1, 1, bytes(32), 2))

    def test_predict_r1_rejects_non_hex(self):
        with self.assertRaises(ValueError):
            kr.predict_r1(dict(RECORD_EXP, ct="zz"))


class Finder(WithTmp):
    def test_empty_and_zero_images(self):
        self.assertEqual(kr.find_records(self.image([])), {"valid": [], "invalid": [], "v1": []})

    def test_entry_shape_has_no_secret_bytes(self):
        r1 = r1_bytes()
        found = kr.find_records(self.image([(4097, r1)]))
        (e,) = found["valid"]
        self.assertEqual(e, {"offset": 4097, "slot": 0, "state": 1, "revision": 1, "name": "ka16",
                             "digest": kr.digest(r1).hex(), "ct_len": 160,
                             "ct_sha256": hashlib.sha256(CT).hexdigest(), "equals_predicted": None,
                             "previous": "00" * 32, "count": 1})
        text = json.dumps(found)
        self.assertNotIn(CT.hex(), text)
        self.assertNotIn(SALT.hex(), text)

    def test_planted_records_across_chunk_boundaries_and_unaligned_offsets(self):
        r1 = r1_bytes()
        r2 = r2_bytes(r1)
        for chunk in (97, 512, 1000, 4096, 65536, 150_000):
            # prefix straddling a boundary, record straddling a boundary, unaligned offsets
            for off1, off2 in ((chunk - 2, 150_001), (chunk - 5, chunk * 3 - 1), (chunk, 77_777 + 3),
                               (1, chunk * 2 + 1), (chunk - 130, chunk * 2 - 30)):
                if off1 < off2 + len(r2) and off2 < off1 + len(r1):
                    continue  # the plants would overwrite each other
                path = self.image([(off1, r1), (off2, r2)], size=max(200_001, off1 + len(r1), off2 + len(r2)) + off2 % 7)
                found = kr.find_records(path, chunk=chunk)
                self.assertEqual([(e["offset"], e["slot"], e["state"]) for e in found["valid"]],
                                 sorted([(off1, 0, 1), (off2, 1, 0)]), (chunk, off1, off2))
                self.assertEqual(found["invalid"], [], (chunk, off1, off2))
                self.assertEqual({e["count"] for e in found["valid"]}, {1})

    def test_zero_chunks_are_skipped_but_data_after_them_is_found(self):
        r1 = r1_bytes()
        path = self.image([(5 * 1024 + 11, r1)], size=40 * 1024)
        found = kr.find_records(path, chunk=1024)
        self.assertEqual([e["offset"] for e in found["valid"]], [5 * 1024 + 11])

    def test_record_at_the_very_end_and_truncated_candidates(self):
        r1 = r1_bytes()
        path = self.image([(1000, r1)], size=1000 + len(r1))
        self.assertEqual(len(kr.find_records(path, chunk=256)["valid"]), 1)
        # cut one byte off the tail: truncated candidate, recorded not judged
        path = self.image([(1000, r1)], size=1000 + len(r1) - 1, name="cut")
        found = kr.find_records(path, chunk=256)
        self.assertEqual(found["valid"], [])
        self.assertEqual(len(found["invalid"]), 1)
        self.assertEqual(found["invalid"][0]["offset"], 1000)
        self.assertIn("truncated", found["invalid"][0]["reason"])
        self.assertFalse(found["invalid"][0]["mac_valid"])
        # header itself cut
        path = self.image([(1000, r1[:100])], size=1100, name="cuthdr")
        self.assertIn("truncated", kr.find_records(path)["invalid"][0]["reason"])

    def test_stale_duplicates_collapse_by_digest_keeping_the_first_offset(self):
        r1 = r1_bytes()
        r2 = r2_bytes(r1)
        path = self.image([(120_000, r1), (3_001, r1), (60_000, r1), (9_999, r2), (150_000, r2)])
        found = kr.find_records(path, chunk=4096)
        by = {e["slot"]: e for e in found["valid"]}
        self.assertEqual(len(found["valid"]), 2)
        self.assertEqual((by[0]["offset"], by[0]["count"]), (3_001, 3))
        self.assertEqual((by[1]["offset"], by[1]["count"]), (9_999, 2))
        # a different record (other revision) is a different member of the set
        r1b = r1_bytes(revision=3)
        found = kr.find_records(self.image([(2000, r1), (9000, r1b)], name="two"))
        self.assertEqual(len(found["valid"]), 2)

    def test_mac_failing_candidate_is_invalid_with_offset_and_reason_never_valid(self):
        r1 = r1_bytes()
        torn = bytearray(r1)
        torn[300] ^= 0xFF  # a ciphertext byte
        found = kr.find_records(self.image([(7000, bytes(torn)), (20_000, r1)]))
        self.assertEqual([e["offset"] for e in found["valid"]], [20_000])
        (bad,) = found["invalid"]
        self.assertEqual(bad["offset"], 7000)
        self.assertTrue(bad["reason"].startswith("mac:"), bad["reason"])
        self.assertIs(bad["mac_valid"], False)

    def test_implausible_prefixes_are_not_candidates_and_v1_is_listed(self):
        junk = [(1000, b"CKMS\x02\x02\x01\x01"), (2000, b"CKMS\x02\x00\x02\x01"), (3000, b"CKMS\x02\x00\x01\x02"),
                (4000, b"CKMS\x03\x00\x01\x01"), (5000, b"CKMS"), (6000, b"CKMS\x01\x00\x01\x01" + bytes(178)),
                (8000, b"CKMS\x01")]
        found = kr.find_records(self.image(junk))
        self.assertEqual(found["valid"], [])
        self.assertEqual(found["invalid"], [])
        self.assertEqual([e["offset"] for e in found["v1"]], [6000, 8000])

    def test_fake_header_inside_a_ciphertext_is_one_more_invalid_candidate(self):
        ct = bytearray(CT)
        ct[40:48] = b"CKMS\x02\x00\x01\x01"
        r1 = r1_bytes(ciphertext=bytes(ct))
        found = kr.find_records(self.image([(5000, r1)]), chunk=512)
        self.assertEqual([e["offset"] for e in found["valid"]], [5000])
        self.assertEqual([e["offset"] for e in found["invalid"]], [5000 + 217 + 40])

    def test_mac_valid_but_strict_invalid_candidates_are_flagged(self):
        plain = Raw(custody=1).bytes()
        found = kr.find_records(self.image([(10_000, plain)]))
        self.assertEqual(found["valid"], [])
        (bad,) = found["invalid"]
        self.assertIs(bad["mac_valid"], True)
        self.assertTrue(bad["reason"].startswith("strict:"), bad["reason"])
        self.assertEqual(bad["offset"], 10_000)

    def test_oversized_ct_len_candidates(self):
        big = Raw(ct=bytes(4096)).bytes()
        found = kr.find_records(self.image([(10_000, big)], size=20_000))
        (bad,) = found["invalid"]
        self.assertIs(bad["mac_valid"], True)  # MAC-valid: a writer violation
        torn = bytearray(big)
        torn[500] ^= 1
        found = kr.find_records(self.image([(10_000, bytes(torn))], size=20_000, name="torn"))
        (bad,) = found["invalid"]
        self.assertIs(bad["mac_valid"], False)

    def test_secret_free_json_through_the_cli(self):
        r1 = r1_bytes()
        path = self.image([(4097, r1)])
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(kr.main(["find", path]), 0)
        text = out.getvalue()
        data = json.loads(text)
        self.assertEqual(data["valid"][0]["offset"], 4097)
        self.assertNotIn(CT.hex(), text)
        self.assertNotIn(PUB.hex(), text)


class Oracle(WithTmp):
    def found(self, plants, **kw):
        return kr.find_records(self.image(plants, **kw))

    def verdict(self, plants, expect, record=RECORD_EXP):
        return kr.evaluate_set(self.found(plants), expect, record)

    def setUp(self):
        super().setUp()
        self.r1 = r1_bytes()
        self.r2 = r2_bytes(self.r1)

    def test_none(self):
        v = self.verdict([], "none")
        self.assertEqual(v, {"expect": "none", "ok": True, "reasons": []})
        self.assertFalse(self.verdict([(1000, self.r1)], "none")["ok"])
        self.assertFalse(self.verdict([(1000, self.r2)], "none")["ok"])

    def test_none_ignores_mac_failing_candidates(self):
        torn = bytearray(self.r1)
        torn[400] ^= 1
        self.assertTrue(self.verdict([(1000, bytes(torn))], "none")["ok"])

    def test_r1(self):
        found = self.found([(1000, self.r1)])
        v = kr.evaluate_set(found, "r1", RECORD_EXP)
        self.assertTrue(v["ok"], v)
        self.assertIs(found["valid"][0]["equals_predicted"], True)
        self.assertFalse(self.verdict([], "r1")["ok"])
        # R1 plus stale duplicates of itself is still R1
        self.assertTrue(self.verdict([(1000, self.r1), (90_000, self.r1)], "r1")["ok"])
        # R1 and R2 is not {R1}
        self.assertFalse(self.verdict([(1000, self.r1), (9000, self.r2)], "r1")["ok"])

    def test_r1_whole_record_equality(self):
        cases = {
            "name": dict(name="ka17"),
            "key": dict(public_key=bytes([9]) * 32),
            "rounds": dict(kdf_rounds=17),
            "salt": dict(salt=bytes([0x12]) * 16),
            "ciphertext": dict(ciphertext=CT[:-1] + b"\xee"),
            "ct longer": dict(ciphertext=CT + bytes(16)),
            "revision": dict(revision=2),
            "previous": dict(previous=bytes([1]) * 32),
            "slot": dict(slot=1),
        }
        for what, kw in cases.items():
            found = self.found([(1000, r1_bytes(**kw))])
            v = kr.evaluate_set(found, "r1", RECORD_EXP)
            self.assertFalse(v["ok"], what)
            if what != "slot":
                self.assertIs(found["valid"][0]["equals_predicted"], False, what)
            self.assertTrue(v["reasons"], what)

    def test_r1_without_a_host_prediction_cannot_pass(self):
        v = kr.evaluate_set(self.found([(1000, self.r1)]), "r1", None)
        self.assertFalse(v["ok"])
        self.assertTrue(kr.evaluate_set(self.found([]), "none", None)["ok"])

    def test_r1r2(self):
        found = self.found([(1000, self.r1), (9000, self.r2), (50_000, self.r1), (80_000, self.r2)])
        v = kr.evaluate_set(found, "r1r2", RECORD_EXP)
        self.assertEqual(v, {"expect": "r1r2", "ok": True, "reasons": []})
        eq = {(e["slot"], e["state"]): e["equals_predicted"] for e in found["valid"]}
        self.assertEqual(eq, {(0, 1): True, (1, 0): None})

    def test_r1r2_negative_cases(self):
        r1, r2 = self.r1, self.r2
        cases = {
            "only r1": [(1000, r1)],
            "only r2": [(1000, r2)],
            "empty": [],
            "r2 wrong slot": [(1000, r1), (9000, r2_bytes(r1, slot=0))],
            "r2 wrong previous": [(1000, r1), (9000, r2_bytes(r1, previous=bytes(32)))],
            "r2 previous is the wrong digest": [(1000, r1), (9000, r2_bytes(r1, previous=kr.digest(r2)))],
            "r2 is present": [(1000, r1), (9000, r1_bytes(slot=1, revision=2, previous=kr.digest(r1)))],
            "two distinct r2": [(1000, r1), (9000, r2), (20_000, r2_bytes(r1, revision=3))],
            "extra valid record": [(1000, r1), (9000, r2), (20_000, r1_bytes(name="other"))],
            "r1 differs": [(1000, r1_bytes(name="ka17")), (9000, r2_bytes(r1_bytes(name="ka17")))],
        }
        for what, plants in cases.items():
            v = self.verdict(plants, "r1r2")
            self.assertFalse(v["ok"], what)
            self.assertTrue(v["reasons"], what)

    def test_r2_revision_is_recorded_not_judged(self):
        for rev in (2, 7, 2 ** 40):
            found = self.found([(1000, self.r1), (9000, r2_bytes(self.r1, revision=rev))])
            self.assertTrue(kr.evaluate_set(found, "r1r2", RECORD_EXP)["ok"], rev)
            self.assertIn(rev, [e["revision"] for e in found["valid"]])

    def test_mac_valid_strict_invalid_record_is_a_verdict_failure_in_every_set(self):
        plain = Raw(custody=1).bytes()
        for expect, plants in (("none", [(500, plain)]), ("r1", [(500, plain), (9000, self.r1)]),
                               ("r1r2", [(500, plain), (9000, self.r1), (20_000, self.r2)])):
            v = self.verdict(plants, expect)
            self.assertFalse(v["ok"], expect)
            self.assertTrue(any("strict decoder" in r and "offset 500" in r for r in v["reasons"]), v)
        # ...whereas the same plants without the plain record pass
        self.assertTrue(self.verdict([(9000, self.r1), (20_000, self.r2)], "r1r2")["ok"])

    def test_torn_copies_do_not_disturb_a_valid_set(self):
        torn = bytearray(self.r1)
        torn[250] ^= 1
        found = self.found([(1000, bytes(torn)), (9000, self.r1), (20_000, self.r2)])
        self.assertEqual(len(found["invalid"]), 1)
        self.assertTrue(kr.evaluate_set(found, "r1r2", RECORD_EXP)["ok"])

    def test_v1_records_are_observations_only(self):
        found = self.found([(1000, self.r1), (30_000, b"CKMS\x01\x00\x01\x01" + bytes(178))])
        self.assertEqual(len(found["v1"]), 1)
        self.assertTrue(kr.evaluate_set(found, "r1", RECORD_EXP)["ok"])

    def test_unknown_expectation_is_refused(self):
        with self.assertRaises(ValueError):
            kr.evaluate_set(self.found([]), "r2", RECORD_EXP)

    def test_verdict_text_carries_no_secret_bytes(self):
        v = self.verdict([(1000, r1_bytes(name="ka17")), (9000, r1_bytes(ciphertext=bytes([0x77]) * 160))], "r1")
        text = json.dumps(v)
        self.assertNotIn(CT.hex(), text)
        self.assertNotIn(PUB.hex(), text)


class Files(WithTmp):
    def test_sha256_file(self):
        path = os.path.join(self.tmp, "f")
        with open(path, "wb") as f:
            f.write(b"x" * 3_000_000)
        self.assertEqual(kr.sha256_file(path), hashlib.sha256(b"x" * 3_000_000).hexdigest())

    def test_sparse_copy_copies_and_never_overwrites(self):
        src = self.image([(1_000_000, b"data"), (10, b"head")], size=4_000_000, name="src")
        dst = os.path.join(self.tmp, "dst")
        kr.sparse_copy(src, dst)
        self.assertEqual(kr.sha256_file(src), kr.sha256_file(dst))
        self.assertLess(os.stat(dst).st_blocks * 512, 2_000_000)  # holes preserved
        with self.assertRaises(FileExistsError):
            kr.sparse_copy(src, dst)
        self.assertEqual(kr.sha256_file(src), kr.sha256_file(dst))


if __name__ == "__main__":
    unittest.main()
