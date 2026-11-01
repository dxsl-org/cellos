#!/usr/bin/env python3
"""Host tests for scripts/x86/abi_refusal_fixtures.py (Phase 06 witness tooling)."""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import abi_refusal_fixtures as fx  # noqa: E402

try:
    import tomllib
except ImportError:  # Python < 3.11
    tomllib = None


def rec(event, a=0, b=0):
    return (event, struct.pack("<II", a, b))


def admitted(tid):
    return [rec(fx.EV_SIG_VERIFIED), rec(fx.EV_MEASURE, tid, 0xAB), rec(fx.EV_CELL_SPAWN, tid)]


def fixture_stream(routes=2, tid=40, noise=True):
    out = admitted(7)  # an earlier service, outside the window
    for fixture in fx.FIXTURES:
        if noise:
            out.append(rec(1, 3, 4))  # IpcSend noise is ignored
        if fixture.expect == fx.ADMIT:
            out += admitted(tid)
            out.append(rec(9, tid, 1))  # its exit
            tid += 1
        elif fixture.expect == fx.FORMAT:
            out += [rec(fx.EV_SPAWN_DENIED, 0, 1)] * routes
        else:
            out += [rec(fx.EV_SIG_FAILED, 0, 0)] * routes
    return out


def ring_bytes(records, buf_first=True, start=1000):
    buf = bytearray(fx.AUDIT_BUF_SIZE)
    at = start
    for event, payload in records:
        for byte in bytes(8) + bytes([event, len(payload)]) + payload:
            buf[at % fx.AUDIT_BUF_SIZE] = byte
            at += 1
    words = struct.pack("<3Q", at, start, 0)
    return bytes(buf) + words if buf_first else words + bytes(buf)


class RecordLayout(unittest.TestCase):
    def test_v1_and_v2_layouts_match_frozen_abi(self):
        v2 = fx.v2_record(tier=1, flags=0x0801, cap_args_off=0, reserved=0)
        self.assertEqual(len(v2), 16)
        self.assertEqual(v2[:4], (0x5649_4345).to_bytes(4, "little"))
        self.assertEqual((v2[4], v2[5]), (2, 1))
        self.assertEqual(int.from_bytes(v2[6:8], "little"), 0x0801)
        self.assertEqual(len(fx.v1_record()), 8)
        self.assertEqual(fx.v1_record(flags=0x80)[4:], bytes([1, 0x80, 0, 0]))

    def test_v2_refusals_keep_magic_and_differ_from_the_control(self):
        current = fx.v2_record()
        for fixture in fx.FIXTURES:
            if fixture.expect == fx.FORMAT and len(fixture.record) == 16 and fixture.record[4] == 2:
                diff = [i for i in range(16) if fixture.record[i] != current[i]]
                self.assertTrue(diff, fixture.name)
                self.assertTrue(all(4 <= i for i in diff), fixture.name)

    def test_names_unique_and_last_fixture_is_an_admitted_control(self):
        names = [f.name for f in fx.FIXTURES]
        self.assertEqual(len(names), len(set(n.lower() for n in names)))
        self.assertEqual(fx.FIXTURES[-1].expect, fx.ADMIT)
        self.assertEqual([f.expect for f in fx.FIXTURES].count(fx.SIGNATURE), 1)

    @unittest.skipIf(tomllib is None, "tomllib needs Python 3.11")
    def test_autoload_lists_fixtures_in_order(self):
        document = tomllib.loads(fx.autoload_toml())
        self.assertEqual(document["version"], 1)
        self.assertEqual([c["path"] for c in document["cells"]], [f.path for f in fx.FIXTURES])
        self.assertTrue(all(c["restart"] == "never" for c in document["cells"]))


class RingParse(unittest.TestCase):
    def test_both_field_orders_parse_identically(self):
        records = fixture_stream()
        for buf_first in (True, False):
            for start in (0, 1000):  # start 0: tail == dropped == 0, as on a fresh boot
                parsed, layout = fx.parse_ring(ring_bytes(records, buf_first, start=start))
                self.assertEqual(parsed, records)
                self.assertEqual(layout["dropped"], 0)

    def test_wrapped_ring_parses(self):
        records = fixture_stream()
        parsed, _ = fx.parse_ring(ring_bytes(records, start=fx.AUDIT_BUF_SIZE - 25))
        self.assertEqual(parsed, records)

    def test_wrong_size_is_refused(self):
        with self.assertRaises(ValueError):
            fx.parse_ring(bytes(10))


class Evaluate(unittest.TestCase):
    def test_expected_stream_passes_for_one_or_two_routes(self):
        for routes in (1, 2):
            ok, lines, summary = fx.evaluate(fixture_stream(routes=routes))
            self.assertTrue(ok, lines)
            self.assertEqual(summary["routes_per_refusal"], routes)
            self.assertEqual(summary["admitted_tids"], {"abifx-v2": 40, "abifx-v1": 41, "abifx-v2b": 42})

    def test_format_refusal_after_signature_verification_fails(self):
        stream = fixture_stream()
        index = stream.index(rec(fx.EV_SPAWN_DENIED, 0, 1))
        stream.insert(index, rec(fx.EV_SIG_VERIFIED))
        ok, lines, _ = fx.evaluate(stream)
        self.assertFalse(ok)

    def test_signature_fixture_refused_as_format_fails(self):
        stream = [rec(fx.EV_SPAWN_DENIED, 0, 1) if r == rec(fx.EV_SIG_FAILED) else r
                  for r in fixture_stream()]
        ok, _, _ = fx.evaluate(stream)
        self.assertFalse(ok)

    def test_admitted_malformed_record_fails(self):
        stream = fixture_stream(routes=1)
        index = stream.index(rec(fx.EV_SPAWN_DENIED, 0, 1))
        stream[index:index + 1] = admitted(99)
        ok, _, _ = fx.evaluate(stream)
        self.assertFalse(ok)

    def test_other_denial_code_is_not_a_format_refusal(self):
        self.assertNotEqual(fx.classify([rec(fx.EV_SPAWN_DENIED, 0, 2)])[0], fx.FORMAT)


if __name__ == "__main__":
    unittest.main()
