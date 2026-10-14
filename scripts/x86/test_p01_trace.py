"""Isolated decoder behavior tests. Synthetic bytes are not kernel evidence."""
import struct
import unittest

from p01_trace import InvalidTrace, METRIC_SOURCE_OWNERS, decode_record, decode_stream, ownership_evidence, validate_stream, noncpu_root_evidence, xtree_descriptor, xtree_evidence, vtd_evidence, validate_export_order
from p01_trace import (G1_EXPERIMENT, G1_HEADER_FIELDS, G1_GROUP_FIELDS,
                       G1_HART_FIELDS, G1_EVENT_FIELDS, G1_MAX,
                       validate_g1_stream)


INIT = (2, 2, 1, 1)
DRIVER = (10, 10, 3, 1)
MANIFEST = "11" * 32 + "22" * 32 + "33" * 32


def record(sequence, event, flags=3, owner=DRIVER, parent=(0, 0, 0), attempt=0,
           route=0, source=0, key=0, group=0, result=1, detail=0, values=(0, 0, 0, 0), digest="00" * 32):
    data = bytearray(160)
    struct.pack_into("<QHHHBB", data, 0, sequence, event, flags, 0, route, source)
    struct.pack_into("<9Q", data, 16, attempt, *owner, *parent, key)
    struct.pack_into("<HHI4Q", data, 88, group, result, detail, *values)
    data[128:] = bytes.fromhex(digest)
    return bytes(data)


def fixture(denied=False):
    rows = []
    def add(event, **kw):
        rows.append(record(len(rows) + 1, event, **kw))
    add(1, flags=19, owner=INIT, digest="11" * 32)
    add(3, flags=12, owner=(0, 0, 0, 0), parent=(2, 1, 1), attempt=1, route=1, source=1, result=4)
    add(4, flags=31, parent=(2, 1, 1), attempt=1, route=1, source=1, digest="22" * 32)
    for component in range(1, 9):
        add(8, flags=259, group=component, result=4)
    add(2, values=(int(denied), 0, 0, 0))
    if denied:
        add(3, flags=12, owner=(0, 0, 0, 0), parent=(10, 3, 1), attempt=2, route=3, source=2, result=4)
        add(4, flags=12, owner=(0, 0, 0, 0), parent=(10, 3, 1), attempt=2, route=3, source=2, group=1, result=2, values=(4, 0, 0, 0))
    samples = (1, 3) if denied else range(1, 7)
    for sample in samples:
        for group in range(4):
            values = (100, 10, 90, 4096) if group == 0 else ((0, 1, 0, 0) if group == 2 else (0, 0, 0, 0))
            add(10, flags=192, owner=(0, 0, 0, 0), key=sample, group=group, detail=15, values=values)
    add(11, result=4)
    for event in (5, 6, 7): add(event)
    for component in range(1, 9): add(9, flags=259, group=component)
    add(12)
    outcome = ('P01-WORKLOAD {"status":"DENIED_CONTROL","requested":1,"admitted":0,"public_result":"Err(Unknown)"}'
               if denied else 'P01-WORKLOAD {"status":"LIFECYCLE_COMPLETE","requested":0,"admitted":0,"exited":0}')
    lines = [outcome, "P01:closing"] + ["P01-RECORD " + row.hex() for row in rows]
    lines.append("P01-SUBJECT 0 " + struct.pack("<8Q", *DRIVER, 2, 1, 1, 1).hex())
    lines.append("P01-STORAGE payload=56896 core_static=60000 resource_static=2224 exporter_buffer=320")
    lines.append("P01-HEAP-ORIGIN " + struct.pack("<12Q", 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1024).hex())
    lines.append("P01-HEAP-BINDING " + struct.pack("<6Q", 1, *DRIVER, 1).hex())
    lines.append("P01-HEAP-SOURCE " + struct.pack("<6Q", 1, 1, 0, 0, 1, 10).hex())
    lines.append("P01-RETAINED-SOURCE " + struct.pack("<6Q", 1, 1, 1, 7, 0, 2048).hex())
    lines.append("P01-PEAK 0 " + struct.pack("<QQQIHH", 0, 1, 1, 1, 2, 0).hex())
    lines.append("P01-PEAK 1 " + struct.pack("<QQQIHH", 10, 1, 1, 1, 1, 0).hex())
    for component in range(1, 9): lines.append(f"P01-COMPONENT 0 {component} " + bytes(64).hex())
    for sample in samples:
        lines.append(f"P01-STAGE sample={sample} tick={sample}")
        for metric in range(16):
            lines.append(f"P01-SNAPSHOT {sample} {metric} " + struct.pack("<QQIHH", 1, 1, 1, METRIC_SOURCE_OWNERS[metric], 0).hex())
    lines.extend(["P01-MANIFEST " + MANIFEST,
                  f"P01-FINAL reserved={len(rows)} ready={len(rows)} active=0 flags=0 state=closed",
                  "P01-LATE flags=0"])
    return lines, {"manifest_hex": MANIFEST, "cpus": 1, "count": "denied" if denied else "0"}


class DecoderTests(unittest.TestCase):
    def test_explicit_offsets(self):
        data = record(1, 4, flags=31, attempt=19, parent=(2, 1, 1), route=3, source=2, digest="ab" * 32)
        row = decode_record(data)
        self.assertEqual((row["attempt"], row["tid"], row["root"], row["cell"], row["generation"]), (19, *DRIVER))
        self.assertEqual(row["digest"], "ab" * 32)
        with self.assertRaises(InvalidTrace): decode_record(data[:-1])

    def test_reserved_and_presence(self):
        for flags in (3 | 512, 1, 3 | 32):
            with self.subTest(flags=flags), self.assertRaises(InvalidTrace):
                decode_record(record(1, 5, flags=flags))

    def test_complete_zero_fixture(self):
        lines, meta = fixture()
        evidence = validate_stream("\n".join(lines).encode(), meta)
        self.assertTrue(evidence["structurally_complete"])
        self.assertFalse(evidence["fully_settled"])
        self.assertEqual(evidence["qualification_blockers"], ["heap ownership capacity/coverage unproven"])
        self.assertFalse(evidence["denied_control_accepted"])
        self.assertIsNone(evidence["denied_public_result_mapping"])

    def test_missing_duplicate_overflow_fault_late(self):
        lines, meta = fixture()
        variants = [lines[:-3] + lines[-2:], lines + [lines[0]],
                    [line.replace("reserved=", "reserved=999") if line.startswith("P01-FINAL") else line for line in lines],
                    [line.replace("active=0", "active=1") for line in lines],
                    [line.replace("P01-LATE flags=0", "P01-LATE flags=1") for line in lines],
                    [line for line in lines if not line.startswith("P01-SNAPSHOT 6 15")],
                    [line for line in lines if not line.startswith("P01-LATE")]]
        for variant in variants:
            with self.subTest(variant=variant[-4:]), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)

    def test_retention_blocks_settled_not_structural(self):
        lines, meta = fixture()
        final_index = next(i for i, line in enumerate(lines) if line.startswith("P01-RECORD") and bytes.fromhex(line.split()[1])[8] == 9)
        data = bytearray(bytes.fromhex(lines[final_index].split()[1]))
        struct.pack_into("<H", data, 90, 5)
        struct.pack_into("<Q", data, 112, 1)
        lines[final_index] = "P01-RECORD " + data.hex()
        counter_index = next(i for i, line in enumerate(lines) if line.startswith("P01-COMPONENT 0 1 "))
        counter = [0, 0, 1, 0, 0, 4096, 0, 1]
        lines[counter_index] = "P01-COMPONENT 0 1 " + struct.pack("<8Q", *counter).hex()
        evidence = validate_stream("\n".join(lines).encode(), meta)
        self.assertTrue(evidence["structurally_complete"])
        self.assertFalse(evidence["fully_settled"])

    def test_absent_metric_is_not_zero(self):
        lines, meta = fixture()
        for i, line in enumerate(lines):
            if line.startswith("P01-SNAPSHOT 6 13 "):
                lines[i] = "P01-SNAPSHOT 6 13 " + struct.pack("<QQIHH", 0, 0, 4, 0, 0).hex()
            if line.startswith("P01-RECORD "):
                data = bytearray(bytes.fromhex(line.split()[1]))
                if struct.unpack_from("<H", data, 8)[0] == 10 and struct.unpack_from("<Q", data, 80)[0] == 6 and struct.unpack_from("<H", data, 88)[0] == 3:
                    struct.pack_into("<I", data, 92, 13)
                    lines[i] = "P01-RECORD " + data.hex()
        evidence = validate_stream("\n".join(lines).encode(), meta)
        self.assertTrue(evidence["structurally_complete"])
        self.assertFalse(evidence["fully_settled"])
        self.assertIsNone(evidence["stage_metrics"]["6"]["retained_physical_frame_union"])

    def test_duplicate_sidecar_and_reserved_source(self):
        lines, meta = fixture()
        source = next(line for line in lines if line.startswith("P01-SNAPSHOT"))
        with self.assertRaises(InvalidTrace): decode_stream("\n".join(lines + [source]).encode())
        data = struct.pack("<QQIHH", 1, 1, 1, 1, 1)
        with self.assertRaises(InvalidTrace): decode_stream(("P01-SNAPSHOT 1 0 " + data.hex()).encode())

    def test_malformed_and_unknown_source(self):
        for line in ("P01-RECORD xyz", "P01-SNAPSHOT 1 0 " + struct.pack("<QQIHH", 1, 1, 1, 99, 0).hex()):
            with self.subTest(line=line), self.assertRaises(InvalidTrace):
                decode_stream(line.encode())

    def test_attempt_identity_cannot_be_reassigned(self):
        lines, meta = fixture()
        index = next(i for i, line in enumerate(lines) if line.startswith("P01-RECORD") and bytes.fromhex(line.split()[1])[8] == 4)
        data = bytearray(bytes.fromhex(lines[index].split()[1]))
        struct.pack_into("<Q", data, 16, 99)
        lines[index] = "P01-RECORD " + data.hex()
        with self.assertRaises(InvalidTrace):
            validate_stream("\n".join(lines).encode(), meta)

    def test_pending_unknown_counter_prevents_settlement(self):
        for field in (0, 3):
            lines, meta = fixture()
            index = next(i for i, line in enumerate(lines) if line.startswith("P01-RECORD") and bytes.fromhex(line.split()[1])[8] == 9)
            data = bytearray(bytes.fromhex(lines[index].split()[1]))
            struct.pack_into("<Q", data, 96 + field * 8, 1)
            lines[index] = "P01-RECORD " + data.hex()
            counter_index = next(i for i, line in enumerate(lines) if line.startswith("P01-COMPONENT 0 1 "))
            counters = [0] * 8
            counters[field] = 1
            lines[counter_index] = "P01-COMPONENT 0 1 " + struct.pack("<8Q", *counters).hex()
            evidence = validate_stream("\n".join(lines).encode(), meta)
            self.assertTrue(evidence["structurally_complete"])
            self.assertFalse(evidence["fully_settled"])


class DeniedControlTests(unittest.TestCase):
    def test_exact_source_refusal_preserves_real_public_mapping(self):
        lines, metadata = fixture(denied=True)
        lines[0] = "USER: " + lines[0]
        lines[1] = "USER: " + lines[1]
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertTrue(evidence["structurally_complete"])
        self.assertTrue(evidence["denied_control_accepted"])
        self.assertFalse(evidence["fully_settled"])
        self.assertEqual(evidence["admitted"], 0)
        self.assertEqual(evidence["workload_outcome"]["public_result"], "Err(Unknown)")
        self.assertEqual(evidence["denied_public_result_mapping"], {
            "attempt": 2, "route": 3, "source": 2, "error_namespace": 1,
            "raw_source_error": 4, "source_error": "PermissionDenied",
            "encoded_return": -1, "public_result": "Err(Unknown)",
            "target_path_recorded": False,
        })

    def test_unknown_public_result_is_not_arbitrary_denial_proof(self):
        # Preserve a valid source error/category pair, but not the exact policy
        # refusal. This includes ViError::PermissionDenied from the loader.
        for namespace, error, result in ((1, 0, 3), (1, 5, 3), (1, 7, 3),
                                         (1, 10, 3), (2, 3, 2)):
            lines, metadata = fixture(denied=True)
            index = next(i for i, line in enumerate(lines) if line.startswith("P01-RECORD")
                         and decode_record(bytes.fromhex(line.split()[1]))["event"] == 4
                         and decode_record(bytes.fromhex(line.split()[1]))["attempt"] == 2)
            data = bytearray(bytes.fromhex(lines[index].split()[1]))
            struct.pack_into("<HH", data, 88, namespace, result)
            struct.pack_into("<Q", data, 96, error)
            lines[index] = "P01-RECORD " + data.hex()
            with self.subTest(namespace=namespace, error=error), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(lines).encode(), metadata)

    def test_public_result_must_match_source_mapping(self):
        for public_result in ("Err(PermissionDenied)", "Err(OutOfMemory)", "Ok(99)", "Unknown", ""):
            lines, metadata = fixture(denied=True)
            lines[0] = lines[0].replace("Err(Unknown)", public_result)
            with self.subTest(public_result=public_result), self.assertRaisesRegex(InvalidTrace, "public/source"):
                validate_stream("\n".join(lines).encode(), metadata)
        lines, metadata = fixture(denied=True)
        lines[0] = lines[0].replace(',"public_result":"Err(Unknown)"', "")
        with self.assertRaisesRegex(InvalidTrace, "public/source"):
            validate_stream("\n".join(lines).encode(), metadata)

    def test_refusal_must_bind_exact_route_and_parent_incarnation(self):
        for changed in ("route", "parent_cell", "parent_generation"):
            lines, metadata = fixture(denied=True)
            for index, line in enumerate(lines):
                if not line.startswith("P01-RECORD"):
                    continue
                data = bytearray(bytes.fromhex(line.split()[1]))
                row = decode_record(data)
                if row["event"] in (3, 4) and row["attempt"] == 2:
                    if changed == "route":
                        data[14] = 2
                    else:
                        struct.pack_into("<Q", data, 64 if changed == "parent_cell" else 72, 99)
                    lines[index] = "P01-RECORD " + data.hex()
            with self.subTest(changed=changed), self.assertRaisesRegex(InvalidTrace, "exact Pinned"):
                validate_stream("\n".join(lines).encode(), metadata)

    def test_refusal_cannot_invent_a_child(self):
        lines, metadata = fixture(denied=True)
        index = next(i for i, line in enumerate(lines) if line.startswith("P01-RECORD")
                     and decode_record(bytes.fromhex(line.split()[1]))["event"] == 4
                     and decode_record(bytes.fromhex(line.split()[1]))["attempt"] == 2)
        data = bytearray(bytes.fromhex(lines[index].split()[1]))
        struct.pack_into("<H", data, 10, 31)
        struct.pack_into("<4Q", data, 24, 99, 99, 4, 1)
        data[128:] = bytes.fromhex("33" * 32)
        lines[index] = "P01-RECORD " + data.hex()
        with self.assertRaises(InvalidTrace):
            validate_stream("\n".join(lines).encode(), metadata)

    def test_denied_control_rejects_an_independently_published_child(self):
        lines, metadata = fixture(denied=True)
        child = (99, 99, 4, 1)
        rows = [bytearray(bytes.fromhex(line.split()[1])) for line in lines if line.startswith("P01-RECORD")]
        extra = [
            record(1, 3, flags=12, owner=(0, 0, 0, 0), parent=(10, 3, 1), attempt=3, route=3, source=2, result=4),
            record(1, 4, flags=31, owner=child, parent=(10, 3, 1), attempt=3, route=3, source=2, key=1, digest="33" * 32),
        ]
        for component in range(1, 9):
            extra.append(record(1, 8, flags=259, owner=child, group=component, result=4))
        for event in (5, 6, 7):
            extra.append(record(1, event, owner=child))
        for component in range(1, 9):
            extra.append(record(1, 9, flags=259, owner=child, key=1, group=component))
        closing = next(i for i, row in enumerate(rows) if decode_record(row)["event"] == 11)
        rows[closing:closing] = [bytearray(row) for row in extra]
        for sequence, row in enumerate(rows, 1):
            struct.pack_into("<Q", row, 0, sequence)
        sidecars = [line for line in lines if not line.startswith(("P01-RECORD", "P01-FINAL", "P01-LATE", "P01-WORKLOAD", "P01:closing"))]
        sidecars.append("P01-SUBJECT 1 " + struct.pack("<8Q", *child, 10, 3, 1, 3).hex())
        for component in range(1, 9):
            sidecars.append(f"P01-COMPONENT 1 {component} " + bytes(64).hex())
        # Even a userspace outcome agreeing with the published child inventory
        # cannot turn a refusal plus one actual child into the denied control.
        lines = [lines[0].replace('"admitted":0', '"admitted":1'), "P01:closing"]
        lines += ["P01-RECORD " + row.hex() for row in rows] + sidecars
        lines += [f"P01-FINAL reserved={len(rows)} ready={len(rows)} active=0 flags=0 state=closed", "P01-LATE flags=0"]
        with self.assertRaises(InvalidTrace):
            validate_stream("\n".join(lines).encode(), metadata)

    def test_unknown_resources_do_not_reclassify_control_as_resource_measurement(self):
        lines, metadata = fixture(denied=True)
        for index, line in enumerate(lines):
            if line.startswith("P01-SNAPSHOT 3 13 "):
                lines[index] = "P01-SNAPSHOT 3 13 " + struct.pack("<QQIHH", 0, 0, 4, 0, 0).hex()
            elif line.startswith("P01-RECORD"):
                data = bytearray(bytes.fromhex(line.split()[1]))
                row = decode_record(data)
                if row["event"] == 10 and row["key"] == 3 and row["group"] == 3:
                    struct.pack_into("<I", data, 92, 13)
                    lines[index] = "P01-RECORD " + data.hex()
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertTrue(evidence["denied_control_accepted"])
        self.assertFalse(evidence["fully_settled"])
        self.assertIsNone(evidence["stage_metrics"]["3"]["retained_physical_frame_union"])
        self.assertIn("absent/incoherent/unknown metric: retained_physical_frame_union",
                      evidence["qualification_blockers"])

    def test_denied_workload_precedes_unique_closing_marker(self):
        lines, metadata = fixture(denied=True)
        variants = [lines[1:], lines[:1] + lines[2:], [lines[1], lines[0]] + lines[2:],
                    lines[:2] + [lines[1]] + lines[2:], lines[:2] + [lines[0]] + lines[2:]]
        for variant in variants:
            with self.subTest(prefix=variant[:3]), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), metadata)


class StorageAccountingTests(unittest.TestCase):
    def test_missing_resource_overhead_rejects_capture(self):
        lines, metadata = fixture()
        lines = [line.replace(" resource_static=2224", "") for line in lines]
        with self.assertRaises(InvalidTrace):
            validate_stream("\n".join(lines).encode(), metadata)

    def test_status_must_follow_payload_and_late_must_follow_final(self):
        for tag in ("P01-FINAL", "P01-LATE"):
            lines, metadata = fixture()
            status = next(line for line in lines if line.startswith(tag))
            lines.remove(status)
            lines.insert(0, status)
            with self.subTest(tag=tag), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(lines).encode(), metadata)

    def test_workload_after_final_rejects_capture(self):
        lines, metadata = fixture()
        outcome = next(line for line in lines if line.startswith("P01-WORKLOAD"))
        lines.remove(outcome)
        lines.append(outcome)
        with self.assertRaises(InvalidTrace):
            validate_stream("\n".join(lines).encode(), metadata)

    def test_provider_denial_keeps_source_error_semantics(self):
        for result, error in ((2, 0), (3, 3), (4, 8)):
            data = record(1, 4, flags=12, owner=(0, 0, 0, 0),
                          parent=(2, 1, 1), attempt=1, route=3, source=2,
                          group=2, result=result, values=(error, 0, 0, 0))
            with self.subTest(result=result, error=error), self.assertRaises(InvalidTrace):
                decode_record(data)

    def test_duplicate_workload_keys_rejected(self):
        lines, metadata = fixture(denied=True)
        lines[0] = lines[0].replace('"status":', '"status":"FAILED","status":')
        with self.assertRaises(InvalidTrace):
            validate_stream("\n".join(lines).encode(), metadata)

    def test_known_wrong_metric_source_rejected(self):
        for metric in (6, 15):
            lines, metadata = fixture()
            index = next(i for i, line in enumerate(lines) if line.startswith(f"P01-SNAPSHOT 1 {metric} "))
            lines[index] = f"P01-SNAPSHOT 1 {metric} " + struct.pack("<QQIHH", 1, 1, 1, 1, 0).hex()
            with self.subTest(metric=metric), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(lines).encode(), metadata)


class OwnershipLedgerTests(unittest.TestCase):
    @staticmethod
    def replace(lines, tag, values):
        index = next(i for i, line in enumerate(lines) if line.startswith(tag + " "))
        lines[index] = tag + " " + struct.pack("<" + "Q" * len(values), *values).hex()

    @staticmethod
    def add_range(lines, token, base, end, current=5, origin=16, owner=DRIVER):
        index = next(i for i, line in enumerate(lines) if line.startswith("P01-RETAINED-SOURCE"))
        lines.insert(index, "P01-RETAINED-RANGE " + struct.pack("<9Q", token, *owner, base, end, current, origin).hex())

    def parse(self, lines):
        raw = "\n".join(lines).encode()
        records, subjects, *_ = decode_stream(raw)
        return ownership_evidence(raw, subjects, records)

    def test_first_fault_diagnostic_never_qualifies_invalid_heap(self):
        lines, metadata = fixture()
        self.assertIsNone(self.parse(lines)["heap_first_fault"])
        self.replace(lines, "P01-HEAP-ORIGIN", (1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1024))
        self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 1, 0, 1, 10))
        lines.insert(0, "P01-HEAP-FAULT " + struct.pack("<6Q", 1, 16, 405, 1, 0, 0).hex())
        self.assertEqual(self.parse(lines)["heap_first_fault"],
                         {"version": 1, "bits": 16, "line": 405, "epoch": 1, "phase": 0, "hart": 0})
        with self.assertRaisesRegex(InvalidTrace, "heap ownership ledger invalid"):
            validate_stream("\n".join(lines).encode(), metadata)
        lines.insert(0, lines[0])
        with self.assertRaisesRegex(InvalidTrace, "duplicate heap first-fault"):
            self.parse(lines)

    def test_first_fault_diagnostic_rejects_malformed_and_forged_validity(self):
        for fault in ((0, 16, 405, 1, 0, 0), (1, 0, 405, 1, 0, 0),
                      (1, 128, 405, 1, 0, 0), (1, 16, 0, 1, 0, 0),
                      (1, 16, 405, 0, 0, 0), (1, 16, 405, 2, 0, 0),
                      (1, 16, 405, 1, 3, 0)):
            lines, _ = fixture()
            self.replace(lines, "P01-HEAP-ORIGIN", (1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1024))
            self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 1, 0, 1, 10))
            lines.insert(0, "P01-HEAP-FAULT " + struct.pack("<6Q", *fault).hex())
            with self.subTest(fault=fault), self.assertRaises(InvalidTrace):
                self.parse(lines)
        lines, _ = fixture()
        lines.insert(0, "P01-HEAP-FAULT " + struct.pack("<6Q", 1, 16, 405, 1, 0, 0).hex())
        with self.assertRaises(InvalidTrace):
            self.parse(lines)

    def test_current_and_origin_reason_survive_export(self):
        lines, _ = fixture()
        self.add_range(lines, 1, 4096, 8192)
        self.replace(lines, "P01-RETAINED-SOURCE", (1, 1, 1, 7, 1, 2048))
        evidence = self.parse(lines)
        self.assertEqual(evidence["retained_ranges"][0][-2:], [5, 16])
        self.assertEqual(evidence["retained_union_bytes"], 4096)

    def test_nested_overlapping_and_adjacent_ranges_count_union_once(self):
        lines, _ = fixture()
        for token, base, end in ((1, 4096, 16384), (2, 8192, 12288),
                                 (3, 12288, 20480), (4, 20480, 24576)):
            self.add_range(lines, token, base, end)
        self.replace(lines, "P01-RETAINED-SOURCE", (1, 1, 1, 7, 4, 2048))
        self.assertEqual(self.parse(lines)["retained_union_bytes"], 20480)

    def test_every_source_reason_and_reserved_reason_rejection(self):
        for reason in (*range(1, 8), *range(10, 18), 0, 8, 9, 18):
            lines, _ = fixture()
            self.add_range(lines, 1, 4096, 8192, current=reason)
            self.replace(lines, "P01-RETAINED-SOURCE", (1, 1, 1, 7, 1, 2048))
            with self.subTest(reason=reason):
                if reason in (0, 8, 9, 18):
                    with self.assertRaises(InvalidTrace):
                        self.parse(lines)
                else:
                    self.assertEqual(self.parse(lines)["retained_ranges"][0][-2], reason)

    def test_malformed_count_epoch_and_origin_payload_rejected(self):
        for tag, values in (
            ("P01-HEAP-ORIGIN", (1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1024)),
            ("P01-HEAP-SOURCE", (1, 2, 0, 0, 1, 10)),
            ("P01-HEAP-SOURCE", (1, 1, 0, 1, 1, 10)),
            ("P01-HEAP-SOURCE", (1, 1, 0, 0, 2, 10)),
            ("P01-HEAP-ORIGIN", (1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1024)),
            ("P01-RETAINED-SOURCE", (1, 2, 1, 7, 0, 2048)),
            ("P01-RETAINED-SOURCE", (1, 1, 1, 7, 1, 2048)),
        ):
            lines, _ = fixture()
            self.replace(lines, tag, values)
            with self.subTest(tag=tag, values=values), self.assertRaises(InvalidTrace):
                self.parse(lines)
        for suffix in ("", "zz", "00 extra"):
            lines, _ = fixture()
            lines.insert(0, "P01-HEAP-ENTRY " + suffix)
            with self.subTest(suffix=suffix), self.assertRaises(InvalidTrace):
                self.parse(lines)
        lines, _ = fixture()
        lines = [line for line in lines if not line.startswith("P01-HEAP-BINDING ")]
        self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 0, 0, 0, 10))
        with self.assertRaises(InvalidTrace):
            self.parse(lines)

    def test_exact_attempt_join_and_transferred_origin(self):
        lines, _ = fixture()
        lines.insert(0, "P01-HEAP-ENTRY " + struct.pack("<19Q", 4096, 32, 8, 5, 0, 1, 0, 0, 0, 0, *([0] * 9)).hex())
        self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 0, 1, 1, 10))
        self.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, 1, 32, 1, 0, 0, 0, 0, 1024))
        evidence = self.parse(lines)
        self.assertEqual(evidence["heap_entries"][0][3:6], [5, 0, 1])
        self.assertEqual(evidence["heap_bindings"][0][1:5], list(DRIVER))
        for binding in ((1, 10, 10, 3, 2, 1), (2, *DRIVER, 1), (1, 0, 0, 0, 0, 1)):
            variant = lines.copy()
            self.replace(variant, "P01-HEAP-BINDING", binding)
            with self.subTest(binding=binding), self.assertRaises(InvalidTrace):
                self.parse(variant)

    def test_runtime_semantic_join_and_transfer_keep_origin_separate(self):
        for semantic_kind, size, source in ((1, 32, (0,) * 4), (2, 4096, (0,) * 4),
                                            (3, 32, (0,) * 4), (4, 3, DRIVER), (5, 3, (0,) * 4),
                                            (11, 3, (0,) * 4), (13, 33, (0,) * 4),
                                            (14, 3, (0,) * 4), (15, 32, (0,) * 4),
                                            (64, 32, (0,) * 4), (65, 32, (0,) * 4),
                                            (66, 32, (0,) * 4), (67, 32, (0,) * 4),
                                            (68, 32, (0,) * 4)):
            lines, _ = fixture()
            entry = (4096, size, 1, 1, 0, 1, *([0] * 4), semantic_kind, *DRIVER, *source)
            lines.insert(0, "P01-HEAP-ENTRY " + struct.pack("<19Q", *entry).hex())
            self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 0, 1, 1, 10))
            self.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, 1, size, 1, 0, 0, 0, 0, 1024))
            with self.subTest(kind=semantic_kind):
                evidence = self.parse(lines)
                self.assertEqual(evidence["heap_entries"][0][3], 1)
                self.assertEqual(evidence["heap_entries"][0][11:15], list(DRIVER))
                self.assertEqual(evidence["heap_origin"][2:4], [0, 0])
                variants = []
                for offset, value in ((5, 0), (10, 6), (12, 99), (14, 2)):
                    forged = list(entry)
                    forged[offset] = value
                    variants.append(forged)
                forged = list(entry)
                forged[15:19] = (0,) * 4 if semantic_kind == 4 else DRIVER
                variants.append(forged)
                for forged in variants:
                    variant = lines.copy()
                    self.replace(variant, "P01-HEAP-ENTRY", forged)
                    with self.subTest(forged=forged), self.assertRaises(InvalidTrace):
                        self.parse(variant)

    def test_wire_transferred_outside_selected_scope_cannot_keep_sender_adoption(self):
        lines, _ = fixture()
        outside = (30, 30, 5, 1)
        entry = (4096, 3, 1, 1, 0, 0, *([0] * 4), 20, *outside, *DRIVER)
        lines.insert(0, "P01-HEAP-ENTRY " + struct.pack("<19Q", *entry).hex())
        self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 0, 1, 1, 10))
        self.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, 1, 3, 0, 1, 0, 0, 0, 1024))
        self.assertEqual(self.parse(lines)["heap_entries"][0][11:15], list(outside))
        for offset, value in ((5, 1), (10, 4), (11, DRIVER[0])):
            forged = list(entry)
            forged[offset] = value
            variant = lines.copy()
            self.replace(variant, "P01-HEAP-ENTRY", forged)
            with self.subTest(forged=forged), self.assertRaises(InvalidTrace):
                self.parse(variant)
        selected_entry = (4096, 3, 1, 1, 0, 1, *([0] * 4), 4, *DRIVER, *outside)
        self.replace(lines, "P01-HEAP-ENTRY", selected_entry)
        self.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, 1, 3, 1, 0, 0, 0, 0, 1024))
        self.assertEqual(self.parse(lines)["heap_entries"][0][15:19], list(outside))
        forged = list(selected_entry)
        forged[15:19] = (DRIVER[0], DRIVER[1], DRIVER[2], DRIVER[3] + 1)
        self.replace(lines, "P01-HEAP-ENTRY", forged)
        with self.assertRaises(InvalidTrace):
            self.parse(lines)
    def test_shared_kernel_semantics_preserve_origin_without_task_adoption(self):
        for semantic_kind in (6, 7, 8, 9, 10, 12, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 47):
            lines, _ = fixture()
            entry = (4096, 32, 1, 4, 1, 0, *([0] * 4), semantic_kind, *([0] * 8))
            lines.insert(0, "P01-HEAP-ENTRY " + struct.pack("<19Q", *entry).hex())
            self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 0, 1, 1, 10))
            self.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, 1, 32, 1, 0, 0, 0, 0, 1024))
            with self.subTest(kind=semantic_kind):
                self.assertEqual(self.parse(lines)["heap_entries"][0][4:6], [1, 0])
                for offset, value in ((5, 1), (10, semantic_kind | 16), (11, DRIVER[0]), (15, DRIVER[0])):
                    forged = list(entry)
                    forged[offset] = value
                    variant = lines.copy()
                    self.replace(variant, "P01-HEAP-ENTRY", forged)
                    with self.subTest(offset=offset), self.assertRaises(InvalidTrace):
                        self.parse(variant)


    def test_heap_duplicate_overlap_layout_and_summary_rejected(self):
        for entries in (
            [(4096, 32, 8, 1, 0, 0, 0, 0, 0, 0)] * 2,
            [(4096, 32, 8, 1, 0, 0, 0, 0, 0, 0), (4112, 32, 8, 1, 0, 0, 0, 0, 0, 0)],
            [(4097, 32, 8, 1, 0, 0, 0, 0, 0, 0)],
            [(4096, 32, 3, 1, 0, 0, 0, 0, 0, 0)],
            [(4096, 32, 8, 4, 2, 0, 0, 0, 0, 0)],
        ):
            lines, _ = fixture()
            for entry in entries:
                lines.insert(0, "P01-HEAP-ENTRY " + struct.pack("<19Q", *entry, *([0] * 9)).hex())
            count = len(entries)
            self.replace(lines, "P01-HEAP-SOURCE", (1, 1, 0, count, 1, 10))
            self.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, count, 32 * count, 0, count, 0, 0, 0, 1024))
            with self.subTest(entries=entries), self.assertRaises(InvalidTrace):
                self.parse(lines)

    def test_incomplete_coverage_cannot_be_settled(self):
        lines, metadata = fixture()
        self.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1024))
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertTrue(evidence["structurally_complete"])
        self.assertFalse(evidence["fully_settled"])
        self.assertIn("heap ownership capacity/coverage unproven", evidence["qualification_blockers"])

    def test_incomplete_range_source_never_exports_zero_as_success(self):
        lines, metadata = fixture()
        self.replace(lines, "P01-RETAINED-SOURCE", (1, 2, 2, 7, 0, 2048))
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertIsNone(evidence["ownership_evidence"]["retained_union_bytes"])
        self.assertFalse(evidence["fully_settled"])


class PhysicalInventoryTests(unittest.TestCase):
    @staticmethod
    def add_inventory(lines, entries, source=None):
        payload = ["P01-PHYSICAL-ENTRY " + struct.pack("<12Q", *entry).hex() for entry in entries]
        if source is None:
            source = (1, 10, 10, 1, 9, len(entries), 8192, 7, 7,
                      sum(entry[3] == 0 for entry in entries))
        payload.append("P01-PHYSICAL-SOURCE " + struct.pack("<10Q", *source).hex())
        return lines[:-2] + payload + lines[-2:]

    def test_absent_history_is_not_a_checked_zero_inventory(self):
        lines, metadata = fixture()
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertIsNone(evidence["physical_inventory"])

    def test_released_incarnation_and_shared_table_do_not_promote_metric13(self):
        lines, metadata = fixture()
        entries = [(1, 4096, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0),
                   (2, 4096, 4, 0, 1, 1, 8192, 12288, 1, 2, 1, 0),
                   (3, 16384, 5, 0, 1, 2, 0, 20480, 0, 0, 0, 24576),
                   (4, 28672, 6, 0, 0, 5, 8192, 0, 1, 1, 0, 0)]
        evidence = validate_stream("\n".join(self.add_inventory(lines, entries)).encode(), metadata)
        self.assertEqual(evidence["physical_inventory"]["entries"], [list(entry) for entry in entries])
        self.assertFalse(evidence["physical_inventory"]["coverage_proven"])
        self.assertFalse(evidence["physical_inventory"]["metric13_promoted"])
        self.assertEqual(evidence["stage_metrics"]["6"]["retained_physical_frame_union"], 0)
        self.assertIn("physical inventory semantic/topology coverage unproven", evidence["qualification_blockers"])

    def test_baseline_census_preserves_unknown_birth_and_records_background_release(self):
        lines, metadata = fixture()
        entries = [(1, 4096, 0, 11, 0, 6, 0, 0, 0, 0, 0, 10),
                   (2, 8192, 0, 0, 0, 7, 12288, 0, 1, 1, 0, 10),
                   (3, 4096, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0)]
        evidence = validate_stream("\n".join(self.add_inventory(lines, entries)).encode(), metadata)
        self.assertEqual(evidence["physical_inventory"]["entries"][0][2], 0)
        self.assertEqual(evidence["physical_inventory"]["entries"][0][11], 10)
        self.assertFalse(evidence["physical_inventory"]["coverage_proven"])
        for index, offset, value in ((0, 2, 1), (0, 3, 10), (0, 4, 1), (0, 11, 0),
                                     (0, 6, 4096), (1, 7, 4096), (1, 9, 0), (2, 2, 10)):
            variant = [list(entry) for entry in entries]
            variant[index][offset] = value
            with self.subTest(index=index, offset=offset), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(self.add_inventory(lines, variant)).encode(), metadata)

    def test_malformed_physical_source_or_incarnation_rejected(self):
        lines, metadata = fixture()
        entry = (1, 4096, 2, 0, 0, 1, 8192, 12288, 1, 1, 0, 0)
        variants = []
        for offset, value in ((0, 0), (1, 4097), (2, 0), (3, 1), (4, 99),
                              (5, 6), (6, 8193), (8, 0), (11, 4096)):
            forged = list(entry)
            forged[offset] = value
            variants.append(self.add_inventory(lines, [forged]))
        variants.append(self.add_inventory(lines, [entry, entry]))
        variants.append(self.add_inventory(lines, [entry, (2, 4096, 3, 0, *entry[4:])]))
        source = (1, 10, 10, 1, 9, 1, 8192, 7, 7, 1)
        for offset, value in ((0, 2), (2, 11), (4, 7), (5, 2), (6, 16384), (7, 0), (8, 0), (9, 0)):
            forged = list(source)
            forged[offset] = value
            variants.append(self.add_inventory(lines, [entry], forged))
        valid = self.add_inventory(lines, [entry])
        variants.append(valid[:-2] + [valid[-3]] + valid[-2:])
        variants.append(valid[:-3] + valid[-2:])
        variants.append(valid + [valid[-3]])
        for index, variant in enumerate(variants):
            with self.subTest(variant=index), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), metadata)

    def test_incoherent_physical_source_stays_unqualified(self):
        lines, metadata = fixture()
        source = (1, 10, 11, 2, 9, 0, 8192, 7, 7, 0)
        evidence = validate_stream("\n".join(self.add_inventory(lines, [], source)).encode(), metadata)
        self.assertIn("physical inventory source incomplete", evidence["qualification_blockers"])
        self.assertFalse(evidence["fully_settled"])


class HeapBaselineTests(unittest.TestCase):
    @staticmethod
    def sample():
        lines, meta = fixture()
        def entry(ptr, size, kind, semantic):
            return (ptr, size, 8, kind, 0, 0, 0, 0, 0, 0, semantic, *([0] * 8))
        baseline = [(2, *entry(4096, 32, 1, 6)), (3, *entry(8192, 64, 1, 7)),
                    (4, *entry(12288, 8, 2, 0))]
        final = [(2, entry(4096, 32, 1, 6)), (11, entry(8192, 128, 1, 7)),
                 (12, entry(16384, 16, 2, 0))]
        OwnershipLedgerTests.replace(lines, "P01-HEAP-ORIGIN", (12, 0, 0, 0, 3, 176, 0, 2, 1, 0, 0, 1024))
        OwnershipLedgerTests.replace(lines, "P01-HEAP-SOURCE", (12, 12, 0, 3, 1, 10))
        payload = ["P01-HEAP-BASELINE-SOURCE " + struct.pack("<12Q", 1, 1, 10, 0, 3, 104, 1, 200, 1000, 2, 1024, 180000).hex(),
                   "P01-HEAP-ACCOUNTING " + struct.pack("<8Q", *([12, 12, 984, 216] * 2)).hex()]
        payload += ["P01-HEAP-BASELINE-ENTRY " + struct.pack("<20Q", *row).hex() for row in baseline]
        for token, row in final:
            payload += ["P01-HEAP-ENTRY " + struct.pack("<19Q", *row).hex(),
                        "P01-HEAP-INCARNATION " + struct.pack("<2Q", row[0], token).hex()]
        lines = lines[:-2] + payload + lines[-2:]
        for index, line in enumerate(lines):
            if line.startswith("P01-RECORD "):
                data = bytearray.fromhex(line.split()[1])
                row = decode_record(data)
                if row["event"] == 10 and row["group"] == 1:
                    struct.pack_into("<2Q", data, 96, 200 if row["key"] == 1 else 984, 1000 if row["key"] == 1 else 216)
                    lines[index] = "P01-RECORD " + data.hex()
        return lines, meta

    def test_exact_incarnation_deltas_keep_used_bytes_and_raw_residual_distinct(self):
        lines, meta = self.sample()
        evidence = validate_stream("\n".join(lines).encode(), meta)
        baseline = evidence["ownership_evidence"]["heap_baseline"]
        self.assertEqual((baseline["created_requested_bytes"], baseline["released_requested_bytes"],
                          baseline["preserved_requested_bytes"]), (144, 72, 32))
        self.assertEqual((baseline["requested_byte_delta"], baseline["allocator_used_byte_delta"],
                          baseline["unmatched_allocator_used_byte_delta"]), (72, 784, 712))
        self.assertEqual(baseline["semantic_requested_byte_deltas"]["7:0:0:0:0"], 64)
        self.assertEqual([row["incarnation"] for row in baseline["preserved"]], [2])
        self.assertEqual([row["incarnation"] for row in baseline["released"]], [3, 4])
        self.assertFalse(baseline["raw_residual_accepted"])
        self.assertFalse(baseline["allocator_used_equals_requested_proven"])
        self.assertFalse(evidence["fully_settled"])
        self.assertIn("unreclaimed raw residual: actual_heap_used_bytes", evidence["qualification_blockers"])

    def test_historical_absence_is_null_and_cannot_qualify(self):
        lines, meta = fixture()
        evidence = validate_stream("\n".join(lines).encode(), meta)
        self.assertIsNone(evidence["ownership_evidence"]["heap_baseline"])
        self.assertFalse(evidence["fully_settled"])

    def test_missing_duplicate_truncated_and_forged_baseline_are_rejected(self):
        lines, meta = self.sample()
        variants = []
        for tag in ("P01-HEAP-BASELINE-SOURCE", "P01-HEAP-BASELINE-ENTRY",
                    "P01-HEAP-INCARNATION", "P01-HEAP-ACCOUNTING"):
            index = next(i for i, line in enumerate(lines) if line.startswith(tag + " "))
            variants += [lines[:index] + lines[index + 1:],
                         lines[:index] + [lines[index], lines[index]] + lines[index + 1:],
                         lines[:index] + [lines[index][:-2]] + lines[index + 1:]]
        for offset, value in ((0, 2), (1, 2), (2, 13), (3, 1), (4, 2), (5, 103),
                              (6, 0), (7, 201), (8, 999), (9, 1), (10, 2048), (11, 1)):
            variant = lines.copy()
            source = [1, 1, 10, 0, 3, 104, 1, 200, 1000, 2, 1024, 180000]
            source[offset] = value
            OwnershipLedgerTests.replace(variant, "P01-HEAP-BASELINE-SOURCE", source)
            variants.append(variant)
        for values in ((4096, 3), (8192, 9), (8192, 2), (16384, 13)):
            variant = lines.copy()
            index = next(i for i, line in enumerate(variant) if line.startswith("P01-HEAP-INCARNATION "))
            variant[index] = "P01-HEAP-INCARNATION " + struct.pack("<2Q", *values).hex()
            variants.append(variant)
        for index, variant in enumerate(variants):
            with self.subTest(variant=index), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)


class HeapChargedTests(unittest.TestCase):
    @staticmethod
    def sample():
        lines, meta = HeapBaselineTests.sample()
        lines = [line.replace("exporter_buffer=320", "exporter_buffer=352")
                 if line.startswith("P01-STORAGE ") else line for line in lines]
        before = (1, 1, 1, 10, 1, 200, 1000, 3, 104, 200, 0, 16, 184, 0, 3, 3, 3, 0, 1024, 400000, 0, 2)
        after = (1, 0, 2, 12, 12, 984, 216, 3, 176, 984, 0, 24, 960, 0, 3, 1, 4, 0, 1024, 400000, 0, 2)
        rows_before = ((4096, 2, 40, 1, 1), (8192, 3, 144, 1, 1), (12288, 4, 16, 1, 1))
        rows_after = ((4096, 2, 40, 1, 1), (8192, 11, 920, 11, 0), (16384, 12, 24, 12, 0))
        payload = []
        for stem, source, rows in (("P01-HEAP-BASELINE-CHARGED", before, rows_before),
                                   ("P01-HEAP-CHARGED", after, rows_after)):
            payload.append(stem + "-SOURCE " + struct.pack("<22Q", *source).hex())
            payload += [stem + "-ENTRY " + struct.pack("<5Q", *row).hex() for row in rows]
        return lines[:-2] + payload + lines[-2:], meta

    def test_measured_charges_close_allocator_not_requested_delta(self):
        lines, meta = self.sample()
        evidence = validate_stream("\n".join(lines).encode(), meta)["ownership_evidence"]
        self.assertEqual(evidence["heap_baseline"]["requested_byte_delta"], 72)
        charged = evidence["heap_charged"]
        self.assertEqual(charged["charged_byte_delta"], 784)
        self.assertEqual(charged["unmatched_allocator_used_byte_delta"], 0)
        self.assertEqual(charged["baseline_partitions"], {"unowned": 16, "shared": 184, "exclusive": 0})
        self.assertEqual((charged["prearm_allocations"], charged["prearm_live_objects"],
                          charged["measured_live_highwater"]), (3, 1, 4))
        self.assertFalse(charged["semantic_coverage_proven"] or charged["fit_proven"])

    def test_partial_truncated_duplicate_or_forged_charge_is_rejected(self):
        lines, meta = self.sample()
        for tag in ("P01-HEAP-BASELINE-CHARGED-SOURCE", "P01-HEAP-BASELINE-CHARGED-ENTRY",
                    "P01-HEAP-CHARGED-SOURCE", "P01-HEAP-CHARGED-ENTRY"):
            index = next(i for i, line in enumerate(lines) if line.startswith(tag + " "))
            for replacement in ([], [lines[index], lines[index]], [lines[index][:-2]]):
                with self.subTest(tag=tag, replacement=replacement), self.assertRaises(InvalidTrace):
                    validate_stream("\n".join(lines[:index] + replacement + lines[index + 1:]).encode(), meta)
        for offset, value in ((0, 2), (1, 1), (2, 1), (3, 13), (4, 13), (5, 983),
                              (7, 2), (8, 175), (9, 983), (10, 1), (11, 25), (14, 4),
                              (15, 2), (16, 2), (17, 1), (18, 1023), (19, 0), (20, 1), (21, 5)):
            variant = lines.copy()
            tag = "P01-HEAP-CHARGED-SOURCE"
            source = list(struct.unpack("<22Q", bytes.fromhex(next(line.split()[1] for line in lines if line.startswith(tag + " ")))))
            source[offset] = value
            OwnershipLedgerTests.replace(variant, tag, source)
            with self.subTest(offset=offset), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)
        for row in ((4096, 3, 40, 1, 1), (4096, 2, 31, 1, 1), (4096, 2, 40, 0, 1),
                    (4096, 2, 40, 1, 2), (4096, 2, 40, 2, 1)):
            variant = lines.copy()
            OwnershipLedgerTests.replace(variant, "P01-HEAP-CHARGED-ENTRY", row)
            with self.subTest(row=row), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)

    def test_historical_absence_and_nonselected_kernel_task_identity(self):
        lines, meta = HeapBaselineTests.sample()
        self.assertIsNone(validate_stream("\n".join(lines).encode(), meta)["ownership_evidence"]["heap_charged"])
        for kind in (64, 65, 66, 67, 68):
            lines, _ = fixture()
            entry = (4096, 32, 8, 1, 0, 0, *([0] * 4), kind | 16, 99, 99, 0, 0, *([0] * 4))
            lines.insert(0, "P01-HEAP-ENTRY " + struct.pack("<19Q", *entry).hex())
            OwnershipLedgerTests.replace(lines, "P01-HEAP-SOURCE", (1, 1, 0, 1, 1, 10))
            OwnershipLedgerTests.replace(lines, "P01-HEAP-ORIGIN", (1, 0, 0, 0, 1, 32, 0, 1, 0, 0, 0, 1024))
            self.assertEqual(OwnershipLedgerTests().parse(lines)["heap_entries"][0][11:15], [99, 99, 0, 0])


class HeapStageTests(unittest.TestCase):
    SOURCE = "P01-HEAP-STAGE-SOURCE"
    ENTRY = "P01-HEAP-STAGE-ENTRY"
    CHARGED = "P01-HEAP-STAGE-CHARGED-ENTRY"

    @staticmethod
    def sample():
        lines, meta = HeapChargedTests.sample()
        source = list(struct.unpack("<22Q", bytes.fromhex(next(
            line.split()[1] for line in lines if line.startswith("P01-HEAP-CHARGED-SOURCE ")))))
        raw = [line.split()[1] for line in lines if line.startswith("P01-HEAP-ENTRY ")]
        charged = [line.split()[1] for line in lines if line.startswith("P01-HEAP-CHARGED-ENTRY ")]
        payload = []
        for sample in range(2, 7):
            source[1:3] = [sample, 1]
            payload.append(f"{HeapStageTests.SOURCE} {sample} " + struct.pack("<22Q", *source).hex())
            payload += [f"{HeapStageTests.ENTRY} {sample} " + row for row in raw]
            payload += [f"{HeapStageTests.CHARGED} {sample} " + row for row in charged]
            for metric in (4, 5):
                prefix = f"P01-SNAPSHOT {sample} {metric} "
                index = next(i for i, line in enumerate(lines) if line.startswith(prefix))
                lines[index] = prefix + struct.pack("<QQIHH", 12, 12, 1, 2, 0).hex()
        return lines[:-2] + payload + lines[-2:], meta

    @staticmethod
    def mutate(lines, tag, sample, changes, row=0):
        indices = [i for i, line in enumerate(lines) if line.startswith(f"{tag} {sample} ")]
        index = indices[row]
        data = bytes.fromhex(lines[index].split()[2])
        values = list(struct.unpack(f"<{len(data) // 8}Q", data))
        for offset, value in changes.items():
            values[offset] = value
        lines[index] = f"{tag} {sample} " + struct.pack(f"<{len(values)}Q", *values).hex()

    @classmethod
    def transfer_sample(cls):
        lines, meta = cls.sample()
        cls.mutate(lines, cls.ENTRY, 3, {5: 1, 10: 64, 11: DRIVER[0], 12: DRIVER[1],
                                       13: DRIVER[2], 14: DRIVER[3]})
        cls.mutate(lines, cls.SOURCE, 3, {3: 13, 12: 920, 13: 40})
        for sample in range(4, 7):
            cls.mutate(lines, cls.SOURCE, sample, {3: 14})
        for tag, offsets in (("P01-HEAP-ORIGIN", (0,)), ("P01-HEAP-SOURCE", (0, 1)),
                             ("P01-HEAP-ACCOUNTING", (0, 4)), ("P01-HEAP-CHARGED-SOURCE", (3,))):
            encoded = next(line.split()[1] for line in lines if line.startswith(tag + " "))
            values = list(struct.unpack(f"<{len(encoded) // 16}Q", bytes.fromhex(encoded)))
            for offset in offsets:
                values[offset] = 14
            OwnershipLedgerTests.replace(lines, tag, values)
        return lines, meta

    def test_live_censuses_are_observed_only_and_match_sample_cutoffs(self):
        lines, meta = self.sample()
        evidence = validate_stream("\n".join(lines).encode(), meta)
        stages = evidence["ownership_evidence"]["heap_stages"]
        self.assertEqual(set(stages), {"2", "3", "4", "5", "6"})
        for sample, stage in stages.items():
            self.assertEqual(stage["source"][1:7], [int(sample), 1, 12, 12, 984, 216])
            self.assertEqual(len(stage["entries"]), 3)
            self.assertEqual(stage["charged_entries"][0], [4096, 2, 40, 1, 1])
            self.assertEqual(stage["partitions"], {"unowned": 24, "shared": 960, "exclusive": 0})
            self.assertEqual(stage["source_compatible"], {
                "allocator_epoch": 12, "allocator_used_bytes": 984, "allocator_free_bytes": 216})
            self.assertTrue(stage["semantic_classification_observed"])
            self.assertFalse(stage["fit_proven"] or stage["coverage_proven"] or stage["semantic_coverage_proven"])
        historical = [line for line in lines if not line.startswith("P01-HEAP-STAGE-")]
        old = validate_stream("\n".join(historical).encode(), meta)
        self.assertIsNone(old["ownership_evidence"]["heap_stages"])
        self.assertEqual(evidence["qualification_blockers"], old["qualification_blockers"])
        self.assertFalse(evidence["fully_settled"] or old["fully_settled"])

    def test_semantic_owner_transfer_changes_partition_not_incarnation_origin(self):
        lines, meta = self.transfer_sample()
        evidence = validate_stream("\n".join(lines).encode(), meta)
        stages = evidence["ownership_evidence"]["heap_stages"]
        self.assertEqual(stages["3"]["partitions"], {"unowned": 24, "shared": 920, "exclusive": 40})
        self.assertEqual(stages["2"]["entries"][0][:5], stages["3"]["entries"][0][:5])
        self.assertEqual(stages["3"]["charged_entries"][0], stages["4"]["charged_entries"][0])
        self.assertFalse(evidence["fully_settled"])

    def test_inventory_cannot_change_without_ledger_epoch_advance(self):
        lines, meta = self.transfer_sample()
        self.mutate(lines, self.SOURCE, 3, {3: 12})
        with self.assertRaisesRegex(InvalidTrace, "inventory changed without ledger epoch advance"):
            validate_stream("\n".join(lines).encode(), meta)

    def test_new_incarnation_must_be_born_after_previous_allocator_cutoff(self):
        for birth, prearm in ((12, 0), (1, 1)):
            lines, meta = self.transfer_sample()
            self.mutate(lines, self.CHARGED, 3, {1: 13, 3: birth, 4: prearm}, row=1)
            self.mutate(lines, self.SOURCE, 3, {15: 1 + prearm})
            with self.subTest(birth=birth, prearm=prearm), self.assertRaisesRegex(
                    InvalidTrace, "new allocation birth predates previous census"):
                validate_stream("\n".join(lines).encode(), meta)

    def test_partial_duplicate_truncated_and_wrong_sample_payloads_fail(self):
        lines, meta = self.sample()
        variants = []
        for tag in (self.SOURCE, self.ENTRY, self.CHARGED):
            index = next(i for i, line in enumerate(lines) if line.startswith(f"{tag} 3 "))
            for replacement in ([], [lines[index], lines[index]], [lines[index][:-2]],
                                [lines[index].replace(f"{tag} 3 ", f"{tag} 1 ")],
                                [lines[index].replace(f"{tag} 3 ", f"{tag} 03 ")]):
                variants.append(lines[:index] + replacement + lines[index + 1:])
        index = next(i for i, line in enumerate(lines) if line.startswith(self.SOURCE + " 3 "))
        variants.append(lines[:index] + [f"{self.SOURCE} 3 " + lines[index].split()[2].upper()] + lines[index + 1:])
        variants.append([line for line in lines if not line.startswith("P01-HEAP-STAGE-") or " 3 " not in line])
        variants.append([line for line in lines if not line.startswith(self.SOURCE + " ")])
        variants.append([line for line in lines if not line.startswith(self.ENTRY + " ")])
        variants.append([line for line in lines if not line.startswith(self.CHARGED + " ")])
        for index, variant in enumerate(variants):
            with self.subTest(variant=index), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)

    def test_source_schema_epoch_counts_and_partition_boundaries_fail(self):
        lines, meta = self.sample()
        for offset, value in ((0, 2), (1, 4), (2, 2), (3, 13), (4, 11), (5, 983), (6, 215),
                              (7, 2), (8, 175), (9, 983), (10, 1), (11, 25), (12, 959),
                              (13, 1), (14, 4), (15, 0), (16, 2), (17, 1), (18, 1023),
                              (19, 0), (19, 400001), (20, 1), (21, 5)):
            variant = lines.copy()
            self.mutate(variant, self.SOURCE, 3, {offset: value})
            with self.subTest(offset=offset), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)

    def test_allocator_sidecar_epoch_status_owner_and_values_must_match(self):
        lines, meta = self.sample()
        for metric in (4, 5):
            prefix = f"P01-SNAPSHOT 3 {metric} "
            index = next(i for i, line in enumerate(lines) if line.startswith(prefix))
            for values in ((11, 11, 1, 2, 0), (12, 13, 2, 2, 0), (12, 12, 1, 1, 0)):
                variant = lines.copy()
                variant[index] = prefix + struct.pack("<QQIHH", *values).hex()
                with self.subTest(metric=metric, sidecar=values), self.assertRaises(InvalidTrace):
                    validate_stream("\n".join(variant).encode(), meta)
            variant = lines.copy()
            index = next(i for i, line in enumerate(variant) if line.startswith("P01-RECORD ")
                         and decode_record(bytes.fromhex(line.split()[1]))["event"] == 10
                         and decode_record(bytes.fromhex(line.split()[1]))["key"] == 3
                         and decode_record(bytes.fromhex(line.split()[1]))["group"] == 1)
            data = bytearray.fromhex(variant[index].split()[1])
            struct.pack_into("<Q", data, 96 + (metric - 4) * 8, 983 if metric == 4 else 215)
            variant[index] = "P01-RECORD " + data.hex()
            with self.subTest(metric=metric, value="mismatch"), self.assertRaisesRegex(InvalidTrace, "heap stage/sample"):
                validate_stream("\n".join(variant).encode(), meta)

    def test_reused_incarnation_cannot_change_invariant_or_charge_birth(self):
        lines, meta = self.sample()
        variants = []
        for change in ({1: 24}, {2: 16}, {3: 2}, {3: 4, 4: 1},
                       {3: 3, 6: DRIVER[0], 7: DRIVER[1], 8: DRIVER[2], 9: DRIVER[3]}):
            variant = lines.copy()
            self.mutate(variant, self.ENTRY, 3, change)
            if 1 in change:
                self.mutate(variant, self.SOURCE, 3, {8: 168})
            variants.append(variant)
        variant = lines.copy()
        self.mutate(variant, self.ENTRY, 3, {0: 20480})
        self.mutate(variant, self.CHARGED, 3, {0: 20480})
        variants.append(variant)
        variant = lines.copy()
        self.mutate(variant, self.CHARGED, 3, {2: 48})
        self.mutate(variant, self.CHARGED, 3, {2: 912}, row=1)
        variants.append(variant)
        variant = lines.copy()
        self.mutate(variant, self.CHARGED, 3, {3: 2})
        variants.append(variant)
        variant = lines.copy()
        self.mutate(variant, self.CHARGED, 3, {4: 0})
        self.mutate(variant, self.SOURCE, 3, {15: 0})
        variants.append(variant)
        for index, variant in enumerate(variants):
            with self.subTest(variant=index), self.assertRaisesRegex(InvalidTrace, "preserved heap stage"):
                validate_stream("\n".join(variant).encode(), meta)
        for changes in ({1: 11}, {1: 0}, {1: 13}, {2: 31}, {3: 0}, {3: 13}, {4: 2}):
            variant = lines.copy()
            self.mutate(variant, self.CHARGED, 3, changes)
            with self.subTest(charged=changes), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)
        variant = lines.copy()
        self.mutate(variant, self.CHARGED, 2, {1: 5}, row=1)
        with self.assertRaisesRegex(InvalidTrace, "new incarnation predates previous census"):
            validate_stream("\n".join(variant).encode(), meta)

    def test_stage_semantic_owners_use_existing_exact_owner_rules(self):
        lines, meta = self.sample()
        for change in ({10: 99}, {10: 64, 11: 99, 12: 99},
                       {5: 1}, {10: 6, 11: DRIVER[0], 12: DRIVER[1]}):
            variant = lines.copy()
            self.mutate(variant, self.ENTRY, 3, change)
            with self.subTest(change=change), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), meta)

    def test_stage_payload_after_final_status_is_rejected(self):
        lines, meta = self.sample()
        for tag in (self.SOURCE, self.ENTRY, self.CHARGED):
            payload = next(line for line in lines if line.startswith(tag + " "))
            variant = [line for line in lines if line != payload] + [payload]
            with self.subTest(tag=tag), self.assertRaisesRegex(InvalidTrace, "immutable payload after final"):
                validate_stream("\n".join(variant).encode(), meta)

    def test_denied_control_requires_only_actual_stage_three_even_when_empty(self):
        lines, meta = fixture(denied=True)
        lines = [line.replace("exporter_buffer=320", "exporter_buffer=352")
                 if line.startswith("P01-STORAGE ") else line for line in lines]
        source = (1, 3, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1024, 400000, 0, 2)
        payload = self.SOURCE + " 3 " + struct.pack("<22Q", *source).hex()
        lines.insert(len(lines) - 2, payload)
        evidence = validate_stream("\n".join(lines).encode(), meta)
        self.assertEqual(set(evidence["ownership_evidence"]["heap_stages"]), {"3"})
        self.assertEqual(evidence["ownership_evidence"]["heap_stages"]["3"]["entries"], [])
        self.assertTrue(evidence["denied_control_accepted"])
        self.assertFalse(evidence["fully_settled"])
        extra = list(source)
        extra[1] = 2
        lines.insert(len(lines) - 2, self.SOURCE + " 2 " + struct.pack("<22Q", *extra).hex())
        with self.assertRaisesRegex(InvalidTrace, "missing/extra heap stage census"):
            validate_stream("\n".join(lines).encode(), meta)


class CpuTopologyTests(unittest.TestCase):
    @staticmethod
    def graph():
        roots = [(4096, 0, 0, 0, 0, 0, 0, 1),
                 (4096, *DRIVER, 0, 0, 1),
                 (20480, 20, 20, 4, 1, 7, 3, 2)]
        nodes = [(4096, 4, 1, 1, 0), (8192, 3, 2, 1, 2),
                 (12288, 2, 3, 1, 1), (16384, 1, 4, 1, 1),
                 (20480, 4, 5, 1, 0)]
        edges = [(4096, 0, 8193, 8192, 0, 1),
                 (20480, 0, 8193, 8192, 0, 1),
                 (8192, 0, 12289, 12288, 0, 1),
                 (8192, 1, (1 << 30) | 129 | 4096, 1 << 30, 1 << 30, 2),
                 (12288, 0, 16385, 16384, 0, 1),
                 (12288, 1, (1 << 21) | 129, 1 << 21, 1 << 21, 2),
                 (16384, 0, 24576 | 129, 24576, 4096, 1),
                 (16384, 1, 24577, 24576, 4096, 1)]
        return roots, nodes, edges

    @staticmethod
    def add_graph(lines, roots, nodes, edges, samples=(1, 2, 3, 4, 5, 6),
                  source_changes=None, epoch=1, histories=None):
        if histories is None:
            histories = [(i, i * 4096, 0, 0, 0, 6, 0, 0, 0, 0, 0, 1)
                         for i in range(1, 11)]
        lines = PhysicalInventoryTests.add_inventory(lines, histories)
        payload = []
        for sample in samples:
            source = [1, sample, epoch, 10, 1, len(roots), len(nodes), len(edges),
                      64, 1024, 16384, 1, 0, 7, len(nodes), len(edges)]
            for offset, value in (source_changes or {}).items():
                source[offset] = value
            for tag, rows in (("SOURCE", [source]), ("ROOT", roots),
                              ("NODE", nodes), ("EDGE", edges)):
                payload.extend(f"P01-TOPOLOGY-{tag} {sample} "
                               + struct.pack("<" + "Q" * len(row), *row).hex()
                               for row in rows)
        return lines[:-2] + payload + lines[-2:]

    def decode(self, roots=None, nodes=None, edges=None, denied=False, **kwargs):
        default_roots, default_nodes, default_edges = self.graph()
        lines, metadata = fixture(denied)
        lines = self.add_graph(lines, default_roots if roots is None else roots,
                               default_nodes if nodes is None else nodes,
                               default_edges if edges is None else edges, **kwargs)
        return validate_stream("\n".join(lines).encode(), metadata)

    def test_historical_absence_is_unknown_and_preserves_gates(self):
        lines, metadata = fixture()
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertIsNone(evidence["ownership_evidence"]["cpu_topology"])
        self.assertEqual(evidence["qualification_blockers"], ["heap ownership capacity/coverage unproven"])

    def test_huge_leaves_PAT_and_shared_graph_are_observation_only(self):
        evidence = self.decode()
        topology = evidence["ownership_evidence"]["cpu_topology"]
        self.assertTrue(topology["cpu_graph_observed_complete"])
        self.assertTrue(topology["snapshot_storage_fit_proven"])
        self.assertFalse(topology["physical_semantic_coverage_proven"])
        self.assertFalse(topology["workload_capacity_fit_proven"])
        self.assertFalse(evidence["fully_settled"])
        self.assertIn("physical inventory semantic/topology coverage unproven",
                      evidence["qualification_blockers"])
        stage = topology["stages"]["1"]
        shared = {row["table_pa"]: row for row in stage["shared_tables"]}
        self.assertEqual(shared[8192]["incoming_descriptor_count"], 2)
        self.assertEqual(shared[16384]["mapping_path_count"], 2)
        self.assertFalse(shared[8192]["semantic_owner_proven"])
        aliases = {row["start_pa"]: row for row in stage["leaf_alias_ranges"]}
        self.assertEqual(aliases[24576]["mapping_alias_count"], 4)
        self.assertEqual(aliases[1 << 30]["end_pa"], 2 << 30)
        self.assertEqual(aliases[1 << 21]["end_pa"], 2 << 21)
        self.assertEqual(evidence["stage_metrics"]["6"]["retained_physical_frame_union"], 0)

    def test_SAS_owner_references_do_not_invent_leaf_aliases(self):
        roots, nodes, edges = self.graph()
        roots = roots[:2] + [(4096, 21, 10, 3, 1, 0, 0, 1)]
        nodes = [node for node in nodes if node[0] != 20480]
        nodes[1] = (*nodes[1][:4], 1)
        edges = [edge for edge in edges if edge[0] != 20480 and edge[:2] != (16384, 1)]
        stage = self.decode(roots, nodes, edges)["ownership_evidence"]["cpu_topology"]["stages"]["1"]
        self.assertEqual(stage["leaf_alias_ranges"], [])
        table = next(row for row in stage["shared_tables"] if row["table_pa"] == 8192)
        self.assertEqual(table["mapping_path_count"], 1)
        self.assertEqual(len(table["root_references"]), 3)
        self.assertFalse(table["semantic_owner_proven"])

    def test_same_root_multiple_paths_are_real_aliases(self):
        roots, nodes, edges = self.graph()
        roots = roots[:2]
        nodes = [node for node in nodes if node[0] != 20480]
        edges = [edge for edge in edges if edge[0] != 20480]
        edges.append((4096, 1, 8193, 8192, 0, 1))
        stage = self.decode(roots, nodes, edges)["ownership_evidence"]["cpu_topology"]["stages"]["1"]
        alias = next(row for row in stage["leaf_alias_ranges"] if row["start_pa"] == 1 << 30)
        self.assertEqual(alias["mapping_alias_count"], 2)
        self.assertEqual(alias["root_pas"], [4096])

    def test_denied_graph_uses_actual_stage_set(self):
        topology = self.decode(denied=True, samples=(1, 3))["ownership_evidence"]["cpu_topology"]
        self.assertEqual(set(topology["stages"]), {"1", "3"})
        with self.assertRaises(InvalidTrace):
            self.decode(denied=True)
        with self.assertRaises(InvalidTrace):
            self.decode(samples=(1, 2, 3, 4, 5))

    def test_capacity_and_other_exclusions_never_qualify(self):
        for exclusion in (1, 2, 4, 8, 16):
            with self.subTest(exclusion=exclusion):
                evidence = self.decode([], [], [], source_changes={11: 4, 12: exclusion, 14: 1025, 15: 16385})
                topology = evidence["ownership_evidence"]["cpu_topology"]
                self.assertFalse(topology["cpu_graph_observed_complete"])
                self.assertFalse(topology["snapshot_storage_fit_proven"])
                self.assertFalse(topology["stages"]["1"]["snapshot_storage_fit_proven"])
                self.assertIn("CPU topology graph incomplete/excluded", evidence["qualification_blockers"])
                self.assertFalse(evidence["fully_settled"])
        for changes in ({11: 4}, {12: 4}, {11: 4, 12: 4}):
            with self.subTest(changes=changes), self.assertRaises(InvalidTrace):
                self.decode(source_changes=changes)

    def test_source_counts_schema_epochs_and_export_order_are_strict(self):
        for offset, value in ((0, 2), (1, 6), (2, 2), (3, 9), (4, 2), (5, 0),
                              (6, 0), (7, 0), (8, 65), (9, 1025), (10, 16385),
                              (11, 2), (12, 32), (13, 0), (14, 6), (15, 9)):
            with self.subTest(offset=offset), self.assertRaises(InvalidTrace):
                self.decode(source_changes={offset: value})
        lines, metadata = fixture()
        lines = self.add_graph(lines, *self.graph())
        source = next(line for line in lines if line.startswith("P01-TOPOLOGY-SOURCE"))
        for variant in (lines[:-2] + [source] + lines[-2:], lines + [source],
                        [line.upper() if line == source else line for line in lines],
                        lines[:-2] + ["P01-TOPOLOGY-UNKNOWN 1 00"] + lines[-2:]):
            with self.subTest(variant=variant[-3:]), self.assertRaises(InvalidTrace):
                validate_stream("\n".join(variant).encode(), metadata)

    def test_root_identity_missing_root_and_domain_conflicts_rejected(self):
        roots, nodes, edges = self.graph()
        for index, offset, value in ((0, 0, 24576), (0, 5, 1), (1, 2, 11),
                                     (1, 3, 4), (1, 4, 2), (1, 5, 1),
                                     (2, 5, 0), (2, 6, 0), (2, 7, 1)):
            changed = [list(row) for row in roots]
            changed[index][offset] = value
            with self.subTest(index=index, offset=offset), self.assertRaises(InvalidTrace):
                self.decode(changed, nodes, edges)
        for changed in (roots[1:], roots + [roots[1]],
                        roots + [(4096, 22, 22, 5, 1, 7, 3, 2)],
                        roots + [(4096, 21, 10, 4, 1, 0, 0, 1)]):
            with self.subTest(roots=changed), self.assertRaises(InvalidTrace):
                self.decode(changed, nodes, edges)

    def test_nodes_incoming_reachability_and_incarnations_rejected(self):
        roots, nodes, edges = self.graph()
        for index, offset, value in ((0, 0, 4097), (0, 1, 3), (0, 2, 99),
                                     (0, 2, 0), (0, 3, 3), (1, 4, 1),
                                     (2, 1, 3)):
            changed = [list(row) for row in nodes]
            changed[index][offset] = value
            with self.subTest(index=index, offset=offset), self.assertRaises(InvalidTrace):
                self.decode(roots, changed, edges)
        for changed in (nodes + [nodes[0]], nodes[:-1],
                        nodes + [(28672, 1, 7, 1, 0)],
                        [(4096, 4, 0, 2, 0)] + nodes[1:]):
            with self.subTest(nodes=changed), self.assertRaises(InvalidTrace):
                self.decode(roots, changed, edges)

    def test_descriptor_geometry_cycles_and_free_targets_rejected(self):
        roots, nodes, edges = self.graph()
        for index, offset, value in ((0, 1, 512), (0, 2, 8192), (0, 2, 8193 | 128),
                                     (0, 3, 4096), (0, 4, 4096), (0, 5, 3),
                                     (3, 2, (1 << 30) | 129 | 8192),
                                     (3, 3, (1 << 30) + 4096), (3, 4, 4096),
                                     (3, 5, 1), (6, 3, 28672), (6, 5, 3)):
            changed = [list(row) for row in edges]
            changed[index][offset] = value
            with self.subTest(index=index, offset=offset), self.assertRaises(InvalidTrace):
                self.decode(roots, nodes, changed)
        with self.assertRaises(InvalidTrace):
            self.decode(roots, nodes, edges + [edges[0]])

    def test_direct_map_free_leaf_is_not_a_table_or_semantic_owner(self):
        roots, nodes, edges = self.graph()
        changed = [list(row) for row in edges]
        changed[6][2] = 49152 | (changed[6][2] & 4095)
        changed[6][3] = 49152
        changed[6][5] = 3
        topology = self.decode(roots, nodes, changed)["ownership_evidence"]["cpu_topology"]
        self.assertTrue(topology["cpu_graph_observed_complete"])
        self.assertFalse(topology["physical_semantic_coverage_proven"])

    def test_leaf_runs_preserve_extents_and_reject_overlap_and_wrong_geometry(self):
        roots, nodes, edges = self.graph()
        runs = [list(row) + [1] for row in edges[:-2]]
        runs.append([16384, 0, 24577, 24576, 4096, 1, 2])
        topology = self.decode(roots, nodes, runs, source_changes={0: 2})["ownership_evidence"]["cpu_topology"]
        self.assertTrue(topology["cpu_graph_observed_complete"])
        self.assertTrue(topology["stages"]["1"]["leaf_runs_encoded"])
        for span in (0, 513):
            changed = [row.copy() for row in runs]
            changed[-1][6] = span
            with self.assertRaises(InvalidTrace):
                self.decode(roots, nodes, changed, source_changes={0: 2})
        for changed in (runs + [[16384, 1, 28673, 28672, 4096, 1, 1]],
                        [runs[0][:6] + [2]] + runs[1:],
                        [runs[0][:2] + [runs[0][2] | 0x20] + runs[0][3:]] + runs[1:]):
            with self.assertRaises(InvalidTrace):
                self.decode(roots, nodes, changed, source_changes={0: 2})

    def test_leaf_run_classification_checks_every_represented_frame(self):
        roots, nodes, edges = self.graph()
        prefix = [list(row) + [1] for row in edges[:-2]]
        for run in ([16384, 0, 40961, 40960, 4096, 1, 2],
                    [16384, 0, 1, 0, 4096, 3, 2]):
            with self.assertRaises(InvalidTrace):
                self.decode(roots, nodes, prefix + [run], source_changes={0: 2})

    def test_physical_history_reuse_joins_actual_cutoff_not_final_live(self):
        roots, nodes, edges = self.graph()
        histories = [(i, i * 4096, 0, 2 if i == 1 else 0, 0, 6, 0, 0, 0, 0, 0, 1)
                     for i in range(1, 11)]
        histories.append((11, 4096, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0))
        # Sample1 precedes release/reuse and must retain the old real token,
        # even though a different incarnation is live at final census.
        self.assertTrue(self.decode(histories=histories)["ownership_evidence"]["cpu_topology"]["cpu_graph_observed_complete"])
        lines, metadata = fixture()
        lines = [line.replace(struct.pack("<QQIHH", 1, 1, 1, 1, 0).hex(),
                              struct.pack("<QQIHH", 3, 3, 1, 1, 0).hex())
                 if line.startswith("P01-SNAPSHOT") and line.split()[2] in ("0", "1", "2", "3")
                 else line for line in lines]
        stale = self.add_graph(lines, roots, nodes, edges, epoch=3, histories=histories)
        with self.assertRaises(InvalidTrace):
            validate_stream("\n".join(stale).encode(), metadata)
        nodes[0] = (4096, 4, 11, 1, 0)
        current = self.add_graph(lines, roots, nodes, edges, epoch=3, histories=histories)
        self.assertTrue(validate_stream("\n".join(current).encode(), metadata)["ownership_evidence"]["cpu_topology"]["cpu_graph_observed_complete"])


class NonCpuRootTests(unittest.TestCase):
    @staticmethod
    def inputs(roots=None, refs=None, changes=None):
        if roots is None:
            roots = [[1, 4096, 1, 1, 1, 0, 0, 0]]
        if refs is None:
            refs = []
        payload = []
        for sample in (1, 2):
            source = [1, sample, 1, 1, 10, 1, len(roots), len(refs), 128, 256, 1, 0, 0, 1, 7, 0]
            for offset, value in (changes or {}).get(sample, {}).items():
                source[offset] = value
            for tag, rows in (("SOURCE", [source]), ("ROOT", roots), ("REFERENCE", refs)):
                payload.extend(f"P01-NONCPU-{tag} {sample} " + struct.pack("<" + "Q" * len(row), *row).hex()
                               for row in rows)
        physical = {"source": [0, 9, 0, 1],
                    "entries": [[i, i * 4096, 0, 0, 0, 6, 0, 0, 0, 0, 0, 1] for i in range(1, 11)]}
        sidecars = {(sample, 1): (1, 1, 1, 1) for sample in (1, 2)}
        metrics = {str(sample): {"used_frames": 10, "page_size": 4096} for sample in (1, 2)}
        ownership = {"heap_baseline": {"entries": []},
                     "heap_stages": {"2": {"entries": [], "charged_entries": []}}}
        return "\n".join(payload).encode(), physical, sidecars, metrics, ownership

    def test_unpublished_live_root_is_present_not_inferred_absent_from_registry(self):
        evidence = noncpu_root_evidence(*self.inputs())
        self.assertTrue(evidence["allocation_lifetime_census_complete"])
        self.assertEqual(evidence["stages"]["1"]["unreferenced_allocated_root_count"], 1)
        self.assertFalse(evidence["physical_tree_topology_proven"])
        self.assertFalse(evidence["physical_semantic_coverage_proven"])

    def test_empty_closed_census_requires_zero_lifetimes_not_just_zero_references(self):
        with self.assertRaisesRegex(InvalidTrace, "lifetime source"):
            noncpu_root_evidence(*self.inputs(roots=[]))
        evidence = noncpu_root_evidence(*self.inputs(roots=[], changes={1: {2: 0, 13: 0}, 2: {2: 0, 13: 0}}))
        self.assertTrue(evidence["allocation_lifetime_census_complete"])

    def test_heap_root_requires_actual_allocation_incarnation_and_layout(self):
        args = list(self.inputs(roots=[[1, 4096, 4, 1, 2, 4096, 7, 0]]))
        args[4]["heap_baseline"]["entries"] = [[7, 4096, 4096, 4096]]
        args[4]["heap_stages"]["2"] = {"entries": [[4096, 4096, 4096]], "charged_entries": [[4096, 7]]}
        self.assertTrue(noncpu_root_evidence(*args)["allocation_lifetime_census_complete"])
        args[4]["heap_stages"]["2"]["charged_entries"][0][1] = 8
        with self.assertRaisesRegex(InvalidTrace, "heap root stale"):
            noncpu_root_evidence(*args)

    def test_wrong_physical_incarnation_and_fake_heap_identity_are_rejected(self):
        for root in ([1, 4096, 1, 9, 1, 0, 0, 0], [1, 4096, 1, 1, 1, 4096, 7, 0]):
            with self.assertRaises(InvalidTrace):
                noncpu_root_evidence(*self.inputs(roots=[root]))

    def test_registry_reference_requires_matching_family_identity_and_count(self):
        root = [1, 4096, 1, 1, 1, 0, 0, 1]
        reference = [1, 1, 10, 3, 0]
        evidence = noncpu_root_evidence(*self.inputs(roots=[root], refs=[reference]))
        self.assertTrue(evidence["published_registry_references_complete"])
        for bad in ([2, 1, 10, 3, 0], [1, 4, 10, 3, 0], [1, 1, 10, 0, 0]):
            with self.assertRaises(InvalidTrace):
                noncpu_root_evidence(*self.inputs(roots=[root], refs=[bad]))
        with self.assertRaises(InvalidTrace):
            noncpu_root_evidence(*self.inputs(roots=[root], refs=[reference, reference]))

    def test_inflight_and_partial_refusal_never_mint_complete_census(self):
        with self.assertRaisesRegex(InvalidTrace, "in-flight"):
            noncpu_root_evidence(*self.inputs(changes={1: {12: 1}}))
        with self.assertRaisesRegex(InvalidTrace, "partial"):
            noncpu_root_evidence(*self.inputs(changes={1: {10: 4, 11: 32, 12: 1}}))
        evidence = noncpu_root_evidence(*self.inputs(roots=[], changes={
            1: {2: 0, 10: 4, 11: 32, 12: 1, 13: 0}, 2: {2: 0, 10: 4, 11: 32, 12: 1, 13: 0}}))
        self.assertFalse(evidence["allocation_lifetime_census_complete"])

    def test_missing_duplicate_and_extra_cutoffs_are_rejected(self):
        args = list(self.inputs())
        lines = args[0].decode().splitlines()
        for changed in ([line for line in lines if " 2 " not in line],
                        lines + [lines[0]],
                        lines + [lines[0].replace("SOURCE 1 ", "SOURCE 3 ")]):
            args[0] = "\n".join(changed).encode()
            with self.assertRaises(InvalidTrace):
                noncpu_root_evidence(*args)

    def test_historical_absence_preserves_unknown_scope(self):
        lines, metadata = fixture()
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertIsNone(evidence["ownership_evidence"]["noncpu_roots"])


    def test_reused_frame_root_requires_new_physical_and_root_incarnations(self):
        args = list(self.inputs())
        changed = []
        for line in args[0].decode().splitlines():
            tag, sample, payload = line.split()
            row = list(struct.unpack("<" + "Q" * (len(payload) // 16), bytes.fromhex(payload)))
            if sample == "2":
                if tag.endswith("SOURCE"):
                    row[2], row[3], row[13] = 3, 3, 2
                else:
                    row[0], row[2], row[3] = 3, 2, 11
            changed.append(f"{tag} {sample} " + struct.pack("<" + "Q" * len(row), *row).hex())
        args[0] = "\n".join(changed).encode()
        args[1]["entries"][0][3] = 2
        args[1]["entries"].append([11, 4096, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        args[2][(2, 1)] = (3, 3, 1, 1)
        evidence = noncpu_root_evidence(*args)
        self.assertEqual(evidence["stages"]["2"]["roots"][0][0:4], [3, 4096, 2, 11])
        args[1]["entries"][-1][2] = 0
        args[1]["entries"][-1][5] = 6
        args[1]["entries"][-1][11] = 3
        with self.assertRaisesRegex(InvalidTrace, "predates previous physical"):
            noncpu_root_evidence(*args)

    def test_nonfreeing_iommu_root_cannot_disappear_even_when_other_epochs_advance(self):
        args = list(self.inputs(roots=[[1, 4096, 4, 1, 2, 4096, 7, 0]]))
        args[4]["heap_baseline"]["entries"] = [[7, 4096, 4096, 4096]]
        args[4]["heap_stages"]["2"] = {
            "entries": [[4096, 4096, 4096], [8192, 4096, 4096]],
            "charged_entries": [[4096, 7], [8192, 8]]}
        changed = []
        for line in args[0].decode().splitlines():
            tag, sample, payload = line.split()
            row = list(struct.unpack("<" + "Q" * (len(payload) // 16), bytes.fromhex(payload)))
            if sample == "2":
                if tag.endswith("SOURCE"):
                    row[2], row[13] = 3, 2
                else:
                    row = [2, 8192, 3, 2, 2, 8192, 8, 0]
            changed.append(f"{tag} {sample} " + struct.pack("<" + "Q" * len(row), *row).hex())
        args[0] = "\n".join(changed).encode()
        with self.assertRaisesRegex(InvalidTrace, "non-freeing IOMMU allocation disappeared"):
            noncpu_root_evidence(*args)


class NonCpuTreeTests(unittest.TestCase):
    @staticmethod
    def inputs():
        root_args = NonCpuRootTests.inputs()
        physical, ownership = root_args[1], root_args[4]
        ownership["noncpu_roots"] = noncpu_root_evidence(*root_args)
        ownership["cpu_topology"] = None
        nodes = [(4096, 1, 4, 1, 0), (8192, 1, 3, 2, 1),
                 (12288, 1, 2, 3, 1), (16384, 1, 1, 4, 1)]
        edges = [(4096, 0, 8193, 8192, 0, 1, 1), (8192, 0, 12289, 12288, 0, 1, 1),
                 (12288, 0, 16385, 16384, 0, 1, 1), (16384, 0, 24577, 24576, 4096, 2, 1)]
        lines = []
        for sample in (1, 2):
            source = [1, sample, 1, 10, 1, 4, 4, 128, 1024, 16384, 1, 0, 15, 4, 4, 1]
            for tag, rows in (("SOURCE", [source]), ("NODE", nodes), ("EDGE", edges)):
                lines.extend(f"P01-XTREE-{tag} {sample} " + struct.pack("<" + "Q" * len(row), *row).hex()
                             for row in rows)
        return "\n".join(lines).encode(), physical, ownership

    def test_exact_tree_reachability_and_cross_plane_alias_extents(self):
        raw, physical, ownership = self.inputs()
        cpu = {"cpu_graph_observed_complete": True,
               "roots": [[20480, 0, 0, 0, 0, 0, 0, 1]],
               "nodes": [[20480, 4, 5, 1, 0], [32768, 3, 8, 1, 1],
                         [36864, 2, 9, 1, 1], [40960, 1, 10, 1, 1]],
               "edges": [[20480, 0, 32769, 32768, 0, 1, 1],
                         [32768, 0, 36865, 36864, 0, 1, 1],
                         [36864, 0, 40961, 40960, 0, 1, 1],
                         [40960, 0, 24577, 24576, 4096, 1, 2]]}
        ownership["cpu_topology"] = {"stages": {"1": cpu, "2": cpu}}
        evidence = xtree_evidence(raw, physical, ownership)
        self.assertEqual(evidence["stages"]["1"]["software_alias_ranges"], [
            {"start": 24576, "end": 32768, "mapping_path_count": 2,
             "planes": ["cpu", "noncpu"], "hardware_authority_proven": False}])
        self.assertFalse(evidence["hardware_publication_proven"])
        self.assertFalse(evidence["workload_capacity_fit_proven"])

    def test_corrupt_run_incarnation_and_normalized_flags_refuse(self):
        raw, physical, ownership = self.inputs()
        lines = raw.decode().splitlines()
        for tag, index, value in (("EDGE", 5, 0), ("NODE", 3, 99), ("EDGE", 2, 8193 | 0x20)):
            changed = lines.copy()
            position = next(i for i, line in enumerate(changed) if line.startswith(f"P01-XTREE-{tag} 1 "))
            fields = changed[position].split()
            row = list(struct.unpack("<" + "Q" * (len(fields[2]) // 16), bytes.fromhex(fields[2])))
            row[index] = value
            changed[position] = f"{fields[0]} {fields[1]} " + struct.pack("<" + "Q" * len(row), *row).hex()
            with self.assertRaises(InvalidTrace):
                xtree_evidence("\n".join(changed).encode(), physical, ownership)

    def test_sv39_ept_and_npt_huge_pat_geometry_remain_distinct(self):
        self.assertEqual(xtree_descriptor(2, 1, 0x8037), (0x8037, 0x8000, 4096))
        self.assertEqual(xtree_descriptor(3, 1, (8 << 10) | 0xc7), ((8 << 10) | 7, 0x8000, 4096))
        self.assertEqual(xtree_descriptor(1, 2, 0x201081), (0x201081, 0x200000, 2097152))
        for kind, level, raw in ((2, 2, 0x2010b7), (3, 1, (8 << 10) | 5), (2, 1, 0x8002)):
            with self.assertRaises(InvalidTrace):
                xtree_descriptor(kind, level, raw)


class VtdStructureTests(unittest.TestCase):
    @staticmethod
    def records(tag, sample, rows):
        return [f"P01-{tag} {sample} " + struct.pack("<" + "Q" * len(row), *row).hex() for row in rows]

    @classmethod
    def inputs(cls, referenced=True, empty=False):
        roots = [] if empty else [
            [1, 4096, 5, 1, 2, 4096, 7, int(referenced)],
            [2, 8192, 6, 2, 2, 8192, 8, int(referenced)],
            [3, 12288, 4, 3, 2, 12288, 9, int(referenced)]]
        refs = [] if empty or not referenced else [
            [1, 2, 0, 0, 0], [2, 3, 255, 0, 4096], [3, 4, 20, 7, 4096]]
        births = len(roots)
        args = list(NonCpuRootTests.inputs(roots=roots, refs=refs, changes={
            sample: {0: 2, 2: births, 13: births} for sample in (1, 2)}))
        args[4]["heap_baseline"]["entries"] = [[7 + i, (i + 1) * 4096, 4096, 4096] for i in range(5)]
        args[4]["heap_stages"]["2"] = {
            "entries": [[(i + 1) * 4096, 4096, 4096] for i in range(5)],
            "charged_entries": [[(i + 1) * 4096, 7 + i] for i in range(5)]}
        physical, ownership = args[1], args[4]
        ownership["noncpu_roots"] = noncpu_root_evidence(*args)
        ownership["cpu_topology"] = None
        nodes = roots[:2]
        edges = [] if empty else [
            [4096, 255, 1, 8193, 0, 8192, 2, 0],
            [8192, 255, 2, 12289, (7 << 8) | 1, 12288, 3, 7]]
        lines = []
        for sample in (1, 2):
            source = [1, sample, 1, 10, births, int(not empty), int(not empty), len(edges),
                      128, 4096, 1, 0, 1, len(edges), len(roots), len(refs)]
            for tag, rows in (("VTD-SOURCE", [source]), ("VTD-NODE", nodes), ("VTD-EDGE", edges)):
                lines.extend(cls.records(tag, sample, rows))
        return "\n".join(lines).encode(), physical, ownership

    @staticmethod
    def change(raw, tag, offsets, occurrence=0, sample=1):
        lines = raw.decode().splitlines()
        index = [i for i, line in enumerate(lines) if line.startswith(f"P01-{tag} {sample} ")][occurrence]
        fields = lines[index].split()
        row = list(struct.unpack("<" + "Q" * (len(fields[2]) // 16), bytes.fromhex(fields[2])))
        for offset, value in offsets.items():
            row[offset] = value
        lines[index] = f"{fields[0]} {sample} " + struct.pack("<" + "Q" * len(row), *row).hex()
        return "\n".join(lines).encode()

    def test_exact_birth_heap_physical_and_slot255_graph(self):
        evidence = vtd_evidence(*self.inputs())
        self.assertTrue(evidence["software_structure_observed_complete"])
        self.assertTrue(evidence["snapshot_storage_fit_proven"])
        stage = evidence["stages"]["1"]
        self.assertEqual(stage["incoming_routing_references"], [
            {"pa": 8192, "birth_token": 2, "incoming": 1},
            {"pa": 12288, "birth_token": 3, "incoming": 1}])
        for key in ("hardware_publication_proven", "physical_semantic_coverage_proven", "workload_capacity_fit_proven"):
            self.assertFalse(evidence[key])
            self.assertFalse(stage[key])

    def test_unsupported_high_word_did_aw_tt_and_low_flags_reject(self):
        raw, physical, ownership = self.inputs()
        for occurrence, offsets in (
                (0, {4: 1}), (0, {7: 1}), (0, {3: 8193 | 2}),
                (1, {4: (7 << 8) | 2}), (1, {4: (8 << 8) | 1}),
                (1, {4: (7 << 8) | 1 | (1 << 24)}), (1, {7: 0}),
                (1, {7: 65536}), (1, {3: 12289 | 4}), (1, {3: 12289 | (1 << 52)}),
                (1, {3: 12288}), (1, {1: 256})):
            with self.subTest(occurrence=occurrence, offsets=offsets), self.assertRaises(InvalidTrace):
                vtd_evidence(self.change(raw, "VTD-EDGE", offsets, occurrence), physical, ownership)

    def test_target_family_birth_and_node_full_identity_reject(self):
        raw, physical, ownership = self.inputs()
        for occurrence, offsets in ((0, {5: 12288, 3: 12289, 6: 3}),
                                    (1, {5: 8192, 3: 8193, 6: 2}), (1, {6: 2}),
                                    (1, {5: 16384, 3: 16385, 6: 4})):
            with self.subTest(offsets=offsets), self.assertRaisesRegex(InvalidTrace, "target birth"):
                vtd_evidence(self.change(raw, "VTD-EDGE", offsets, occurrence), physical, ownership)
        for offset, value in ((0, 99), (2, 5), (3, 9), (4, 1), (5, 16384), (6, 99), (7, 0)):
            with self.subTest(offset=offset), self.assertRaisesRegex(InvalidTrace, "node lifetime"):
                vtd_evidence(self.change(raw, "VTD-NODE", {offset: value}, 1), physical, ownership)

    def test_missing_duplicate_source_node_edge_and_registry_connection_reject(self):
        raw, physical, ownership = self.inputs()
        lines = raw.decode().splitlines()
        for tag in ("SOURCE", "NODE", "EDGE"):
            index = next(i for i, line in enumerate(lines) if line.startswith(f"P01-VTD-{tag} 1 "))
            for changed in (lines[:index] + lines[index + 1:], lines + [lines[index]]):
                with self.subTest(tag=tag), self.assertRaises(InvalidTrace):
                    vtd_evidence("\n".join(changed).encode(), physical, ownership)
        changed = self.change(raw, "VTD-EDGE", {1: 254})
        with self.assertRaisesRegex(InvalidTrace, "registry context/root"):
            vtd_evidence(changed, physical, ownership)
        # Even coherent row counts cannot hide a duplicate slot/node or omitted census node.
        changed = self.change(raw, "VTD-EDGE", {0: 4096, 1: 255, 2: 1, 3: 8193, 4: 0, 5: 8192, 6: 2, 7: 0}, 1)
        with self.assertRaisesRegex(InvalidTrace, "parent/slot"):
            vtd_evidence(changed, physical, ownership)
        changed = self.change(raw, "VTD-NODE", dict(enumerate(ownership["noncpu_roots"]["stages"]["1"]["roots"][0])), 1)
        with self.assertRaisesRegex(InvalidTrace, "node lifetime"):
            vtd_evidence(changed, physical, ownership)
        changed = "\n".join(line for line in lines if not line.startswith("P01-VTD-NODE 1 ")).encode()
        changed = self.change(changed, "VTD-SOURCE", {5: 0, 6: 0})
        with self.assertRaisesRegex(InvalidTrace, "structure census omitted"):
            vtd_evidence(changed, physical, ownership)

    def test_refusal_requires_empty_rows_and_complete_requires_closed_root_census(self):
        raw, physical, ownership = self.inputs()
        partial = self.change(raw, "VTD-SOURCE", {10: 4, 11: 2})
        with self.assertRaisesRegex(InvalidTrace, "refused partial"):
            vtd_evidence(partial, physical, ownership)
        refused = "\n".join(line for line in raw.decode().splitlines() if "VTD-SOURCE" in line).encode()
        for sample in (1, 2):
            refused = self.change(refused, "VTD-SOURCE", {5: 0, 6: 0, 7: 0, 10: 4, 11: 2}, sample=sample)
        self.assertFalse(vtd_evidence(refused, physical, ownership)["software_structure_observed_complete"])
        empty, physical, ownership = self.inputs(empty=True)
        self.assertTrue(vtd_evidence(empty, physical, ownership)["software_structure_observed_complete"])
        ownership["noncpu_roots"]["stages"]["1"]["allocation_lifetime_census_complete"] = False
        with self.assertRaisesRegex(InvalidTrace, "complete preflight"):
            vtd_evidence(empty, physical, ownership)

    def test_exact_cutoff_and_preflight_counts_required(self):
        raw, physical, ownership = self.inputs()
        for offsets in ({2: 2}, {3: 9}, {4: 4}, {8: 129}, {9: 4095}, {12: 2},
                        {13: 3}, {14: 2}, {15: 2}, {11: 32}):
            with self.subTest(offsets=offsets), self.assertRaises(InvalidTrace):
                vtd_evidence(self.change(raw, "VTD-SOURCE", offsets), physical, ownership)

    def test_unreferenced_lifetime_structures_need_not_be_registry_published_or_reachable(self):
        raw, physical, ownership = self.inputs(referenced=False)
        lines = [line for line in raw.decode().splitlines() if "VTD-EDGE" not in line]
        changed = "\n".join(lines).encode()
        for sample in (1, 2):
            changed = self.change(changed, "VTD-SOURCE", {7: 0, 13: 0}, sample=sample)
        self.assertTrue(vtd_evidence(changed, physical, ownership)["software_structure_observed_complete"])

    def test_root_versions_distinguish_actual_context_family(self):
        args = list(NonCpuRootTests.inputs(
            roots=[[1, 4096, 5, 1, 2, 4096, 7, 1], [2, 8192, 5, 2, 2, 8192, 8, 1]],
            refs=[[1, 2, 0, 0, 0], [2, 3, 255, 0, 4096]],
            changes={sample: {2: 2, 13: 2} for sample in (1, 2)}))
        args[4]["heap_baseline"]["entries"] = [[7, 4096, 4096, 4096], [8, 8192, 4096, 4096]]
        args[4]["heap_stages"]["2"] = {
            "entries": [[4096, 4096, 4096], [8192, 4096, 4096]], "charged_entries": [[4096, 7], [8192, 8]]}
        self.assertTrue(noncpu_root_evidence(*args)["allocation_lifetime_census_complete"])
        for sample in (1, 2):
            args[0] = self.change(args[0], "NONCPU-SOURCE", {0: 2}, sample=sample)
        with self.assertRaisesRegex(InvalidTrace, "wrong root family"):
            noncpu_root_evidence(*args)

    def test_v2_xtree_filters_structure_seeds_without_routing_alias_multiplication(self):
        raw, physical, ownership = self.inputs()
        # Two requester slots route to the same independently counted SLPT root.
        more = self.records("VTD-EDGE", 1, [[8192, 0, 2, 12289, 1793, 12288, 3, 7]])
        raw = raw + b"\n" + "\n".join(more).encode()
        raw = self.change(raw, "VTD-SOURCE", {7: 3, 13: 3})
        self.assertTrue(vtd_evidence(raw, physical, ownership)["software_structure_observed_complete"])
        nodes = [[12288, 4, 3, 3, 0], [16384, 4, 2, 4, 1], [20480, 4, 1, 5, 1]]
        edges = [[12288, 0, 16387, 16384, 0, 1, 4], [16384, 0, 20483, 20480, 0, 1, 4],
                 [20480, 0, 24579, 24576, 4096, 1, 4]]
        lines = []
        for sample in (1, 2):
            source = [2, sample, 1, 10, 1, 3, 3, 128, 1024, 16384, 1, 0, 15, 3, 3, 3]
            for tag, rows in (("XTREE-SOURCE", [source]), ("XTREE-NODE", nodes), ("XTREE-EDGE", edges)):
                lines.extend(self.records(tag, sample, rows))
        tree_raw = "\n".join(lines).encode()
        graph = xtree_evidence(tree_raw, physical, ownership)
        self.assertTrue(graph["software_tree_observed_complete"])
        self.assertEqual(graph["stages"]["1"]["software_alias_ranges"], [])
        self.assertEqual(graph["stages"]["1"]["shared_tables"], [])
        with self.assertRaisesRegex(InvalidTrace, "source coherence"):
            xtree_evidence(self.change(tree_raw, "XTREE-SOURCE", {4: 3}), physical, ownership)

    def test_readiness_stronger_closure_requires_both_graphs(self):
        from unittest.mock import patch
        import p01_budget_readiness as readiness
        raw, physical, ownership = self.inputs()
        vtd = vtd_evidence(raw, physical, ownership)
        tree_lines = []
        for sample in (1, 2):
            source = [2, sample, 1, 10, 1, 1, 0, 128, 1024, 16384, 1, 0, 15, 1, 0, 3]
            tree_lines.extend(self.records("XTREE-SOURCE", sample, [source]))
            tree_lines.extend(self.records("XTREE-NODE", sample, [[12288, 4, 3, 3, 0]]))
        tree_raw = "\n".join(tree_lines).encode()
        tree = xtree_evidence(tree_raw, physical, ownership)
        refused_vtd_raw = "\n".join(line for line in raw.decode().splitlines() if "VTD-SOURCE" in line).encode()
        refused_tree_raw = "\n".join(line for line in tree_lines if "XTREE-SOURCE" in line).encode()
        for sample in (1, 2):
            refused_vtd_raw = self.change(
                refused_vtd_raw, "VTD-SOURCE", {5: 0, 6: 0, 7: 0, 10: 4, 11: 2}, sample=sample)
            refused_tree_raw = self.change(
                refused_tree_raw, "XTREE-SOURCE", {5: 0, 10: 4, 11: 2}, sample=sample)
        refused_vtd = vtd_evidence(refused_vtd_raw, physical, ownership)
        refused_tree = xtree_evidence(refused_tree_raw, physical, ownership)
        lines, metadata = fixture()
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        metadata.update({
            "capture_uuid": "00000000-0000-0000-0000-000000000001", "replica": 0,
            "build_sha256": "11" * 32, "image_sha256": "22" * 32, "command": ["qemu", "-cdrom", "image.iso"],
            "qemu_binary_sha256": "33" * 32, "runner_sha256": "44" * 32, "parser_sha256": "55" * 32,
            "qemu_version": "fixture"})
        matrix = {"cpus": 1, "build_sha256": metadata["build_sha256"],
                  "captures": [{key: metadata[key] for key in ("count", "replica", "capture_uuid")}]}
        # Only capture I/O is replaced; parser-produced graph states and the
        # analyzer's real summary/cohort/closure logic remain under test.
        with patch.object(readiness, "load_json", side_effect=lambda path: metadata if path.name == "capture.json" else matrix), \
                patch.object(readiness, "digest", return_value="66" * 32), \
                patch.object(readiness, "parse_capture", return_value=evidence):
            for tree_graph, vtd_graph, complete in (
                    (tree, None, False), (None, vtd, False), (tree, refused_vtd, False),
                    (refused_tree, vtd, False), (tree, vtd, True)):
                evidence["ownership_evidence"]["noncpu_topology"] = tree_graph
                evidence["ownership_evidence"]["vtd_structures"] = vtd_graph
                report = readiness.analyze("fixture-matrix.json")
                self.assertEqual(report["observed_noncpu_software_trees_complete"], complete)
                self.assertEqual(report["observed_vtd_software_structures_complete"],
                                 vtd_graph is not None and vtd_graph["software_structure_observed_complete"])
                self.assertEqual(report["runs"][0]["vtd_structures"], vtd_graph)
                self.assertFalse(report["budget_selection_ready"])

    def test_malformed_payload_and_historical_family_cannot_claim_vtd_graph(self):
        raw, physical, ownership = self.inputs()
        first = raw.decode().splitlines()[0]
        for extra in ("P01-VTD-UNKNOWN 1 " + "00" * 64,
                      "P01-VTD-NODE 1 " + "00" * 63, "P01-VTD-EDGE 7 " + "00" * 64,
                      first + " extra", first.upper()):
            with self.subTest(extra=extra), self.assertRaises(InvalidTrace):
                vtd_evidence(raw + b"\n" + extra.encode(), physical, ownership)
        ownership["noncpu_roots"]["stages"]["1"]["source"][0] = 1
        with self.assertRaisesRegex(InvalidTrace, "source coherence"):
            vtd_evidence(raw, physical, ownership)

    def test_absence_unknown_and_immutable_export_order(self):
        self.assertIsNone(vtd_evidence(b"", None, {"noncpu_roots": None}))
        lines, metadata = fixture()
        evidence = validate_stream("\n".join(lines).encode(), metadata)
        self.assertIsNone(evidence["ownership_evidence"]["vtd_structures"])
        for tag, size in (("SOURCE", 16), ("NODE", 8), ("EDGE", 8)):
            late = "\n".join(lines + self.records("VTD-" + tag, 1, [[0] * size])).encode()
            with self.assertRaisesRegex(InvalidTrace, "immutable payload after final"):
                validate_export_order(late)


class G1DecoderTests(unittest.TestCase):
    """Consumer-visible exact accounting regressions; synthetic, not boot evidence."""

    @staticmethod
    def fixture():
        events = []
        def event(now, kind, cell=3, generation=1, hart=G1_MAX, value0=0, value1=0):
            events.append(dict(zip(G1_EVENT_FIELDS, (len(events), now, kind, cell, generation, hart, value0, value1))))
        event(0, 1, 0, 0, value0=0xfed00000, value1=(1_000_000 << 32) | (1 << 13))
        event(0, 16, 0, 0, value0=1, value1=0)
        event(0, 2, value0=10, value1=20_000_000)
        event(0, 3, hart=0, value0=10, value1=1)
        event(1, 4, 0, 0, hart=0, value1=1)
        event(10, 3, hart=0, value0=11, value1=2)
        event(11, 4, hart=0, value0=10, value1=2)
        event(20, 3, hart=0, value0=12, value1=3)
        event(21, 4, hart=0, value0=11, value1=3)
        event(30, 3, 0, 0, hart=0, value1=4)
        event(31, 4, hart=0, value0=12, value1=4)
        for index in range(16):
            allowance = (20_000_000 if index == 0 else 39_999_970 if index == 1
                         else 59_999_970 if index == 2 else 60_000_000)
            event((index + 1) * 100_000_000, 6, value0=30 if index == 0 else 0, value1=allowance)
        event(1_600_000_000, 3, hart=0, value0=10, value1=5)
        event(1_600_000_001, 4, 0, 0, hart=0, value1=5)
        event(1_600_000_002, 3, 0, 0, hart=0, value1=6)
        event(1_600_000_003, 4, hart=0, value0=10, value1=6)
        event(1_600_000_003, 9, value0=10)
        event(1_600_000_003, 10, value0=10)
        event(31, 17, 0, generation=0, hart=0, value0=10)
        event(1_600_000_003, 17, 0, generation=16, hart=0, value0=1)
        event(1_600_000_004, 12, 0, 0, value0=1)
        header = dict.fromkeys(G1_HEADER_FIELDS, 0)
        header.update(state=3, boot_session=10, tick_period_fs=1_000_000,
                      last_ns=1_600_000_004, period_ns=100_000_000, period_index=16,
                      next_boundary_ns=1_700_000_000, control_charge_ns=1_600_000_004 - 32,
                      next_switch_serial=7, event_count=len(events), admitted_hart_mask=1,
                      service_gap_limit_ns=20_000_000, switch_delay_limit_ns=20_000_000,
                      exhaustion_to_departure_limit_ns=20_000_000,
                      last_raw_counter=1_600_000_004, timebase_kind=1)
        retired = dict.fromkeys(G1_GROUP_FIELDS, 0)
        retired.update(cell_id=3, generation=1, root_tid=10, state=5, policy_revision=1,
                       entitlement_ns=20_000_000, burst_cap_ns=40_000_000,
                       burst_bank_ns=40_000_000, period_charge_ns=2, lifetime_charge_ns=32,
                       last_period_allowance_ns=60_000_000, terminal_serial=6, charged_period_count=1)
        def packed(tag, index, names, row):
            return f"{tag} index={index} hex=" + struct.pack("<" + "Q" * len(names), *(row[name] for name in names)).hex()
        lines = [
            packed("G1-HEADER", 0, G1_HEADER_FIELDS, header),
            packed("G1-RETIRED", 0, G1_GROUP_FIELDS, retired),
            packed("G1-HART", 0, G1_HART_FIELDS, dict.fromkeys(G1_HART_FIELDS, 0)),
            packed("G1-HART", 1, G1_HART_FIELDS, dict.fromkeys(G1_HART_FIELDS, 0)),
            "G1-MANIFEST hex=" + MANIFEST,
            f"G1-CLOCK-BOOT raw=0 period_fs=1000000 base=4275044352 caps={(1_000_000 << 32) | (1 << 13)} config=1 ticks_per_ms=1000 initial=10000 lvt=131104",
            f"G1-CLOCK raw=1600000004 period_fs=1000000 base=4275044352 capabilities={(1_000_000 << 32) | (1 << 13)} configuration=1",
            f"G1-CUTOFF events={len(events)} retired=1 pending=0 state=3 error=0 payload=41304",
            "G1-WORKLOAD mode=yield workers=2 explicit_yields=8 delay_source=hpet-period16 max_chunks=1000000 status=start",
        ]
        for worker in (0, 1):
            tid = worker + 11
            lines.extend([f"G1-ACTION worker={worker} action=admit tid={tid} ms=0",
                          f"G1-ACTION worker={worker} action=release tid={tid} ms=0",
                          f"G1-ACTION worker={worker} action=start sender_tid=10 sender_cell=3 sender_generation=1 ms=0"])
            lines.extend(f"G1-ACTION worker={worker} action=yield ordinal={ordinal} ms=0" for ordinal in range(4))
            lines.extend([f"G1-ACTION worker={worker} action=exit ms=0",
                          f"G1-ACTION worker={worker} action=joined tid={tid} ms=0"])
        lines.extend(["G1-ACTION worker=2 action=delay-start ms=0",
                      "G1-ACTION worker=2 action=delay-end ms=1600",
                      "G1-WORKLOAD mode=yield workers=2 status=terminal ms=1600"])
        lines.extend(packed("G1-EVENT", index, G1_EVENT_FIELDS, row) for index, row in enumerate(events))
        return lines, {"g1": True, "g1_experiment": G1_EXPERIMENT, "count": "g1-yield", "cpus": 1, "manifest_hex": MANIFEST}

    @staticmethod
    def mutate(lines, tag, index, names, field, value):
        prefix = f"{tag} index={index} hex="
        output = list(lines)
        position = next(i for i, line in enumerate(output) if line.startswith(prefix))
        values = list(struct.unpack("<" + "Q" * len(names), bytes.fromhex(output[position][len(prefix):])))
        values[names.index(field)] = value
        output[position] = prefix + struct.pack("<" + "Q" * len(names), *values).hex()
        return output


    def test_charge_and_recurrence_are_not_pass_text(self):
        lines, meta = self.fixture()
        for tag, index, names, field, value in (
            ("G1-EVENT", 11, G1_EVENT_FIELDS, "value0", 31),
            ("G1-EVENT", 12, G1_EVENT_FIELDS, "value1", 40_000_000),
            ("G1-RETIRED", 0, G1_GROUP_FIELDS, "lifetime_charge_ns", 0),
            ("G1-RETIRED", 0, G1_GROUP_FIELDS, "burst_bank_ns", 0),
        ):
            with self.subTest(field=field), self.assertRaises(InvalidTrace):
                changed = self.mutate(lines, tag, index, names, field, value)
                validate_g1_stream(("\n".join(changed) + "\nPASS").encode(), meta)

    def test_unknown_stale_unstaged_and_duplicate_tid(self):
        lines, meta = self.fixture()
        for index, field, value in ((5, "generation", 2), (6, "value1", 99),
                                     (6, "value0", 99), (6, "kind", 99),
                                     (5, "hart", 1), (6, "sequence", 5)):
            with self.subTest(index=index, field=field), self.assertRaises(InvalidTrace):
                validate_g1_stream("\n".join(self.mutate(lines, "G1-EVENT", index, G1_EVENT_FIELDS, field, value)).encode(), meta)

    def test_refusal_unknown_overflow_and_pending_remain_invalid(self):
        lines, meta = self.fixture()
        variants = [
            [line.replace("events=36", "events=257") for line in lines],
            [line.replace("pending=0", "pending=1") for line in lines],
            [line.replace("error=0", "error=1") for line in lines],
            lines + ["G1-WORKLOAD invalid=work-bound"],
            self.mutate(lines, "G1-EVENT", 5, G1_EVENT_FIELDS, "kind", 11),
            self.mutate(lines, "G1-HEADER", 0, G1_HEADER_FIELDS, "control_charge_ns", G1_MAX),
        ]
        for changed in variants:
            with self.subTest(changed=changed[-1]), self.assertRaises(InvalidTrace):
                validate_g1_stream("\n".join(changed).encode(), meta)

    def test_clock_identity_and_missing_rows(self):
        lines, meta = self.fixture()
        variants = [
            [line.replace("configuration=1", "configuration=0") for line in lines],
            [line.replace(f"capabilities={(1_000_000 << 32) | (1 << 13)}", "capabilities=0") for line in lines],
            [line.replace("raw=1600000004", "raw=0") for line in lines],
            [line.replace("initial=10000", "initial=62500") for line in lines],
            [line.replace("lvt=131104", "lvt=0") for line in lines],
            [line for line in lines if not line.startswith("G1-RETIRED")],
            [line for line in lines if not line.startswith("G1-MANIFEST")],
            lines + ["G1-FUTURE value=1"],
        ]
        for changed in variants:
            with self.subTest(changed=changed[-1]), self.assertRaises(InvalidTrace):
                validate_g1_stream("\n".join(changed).encode(), meta)

    def test_immutable_initial_clock_tuple_mismatch_refuses(self):
        lines, meta = self.fixture()
        for field, value in (("value0", 3), ("value1", 1), ("kind", 1), ("cell_id", 3)):
            with self.subTest(field=field), self.assertRaises(InvalidTrace):
                validate_g1_stream("\n".join(self.mutate(lines, "G1-EVENT", 1, G1_EVENT_FIELDS, field, value)).encode(), meta)

    def test_idle_skip_exact_recurrence_and_negative_count(self):
        lines, meta = self.fixture()
        first = next(i for i, line in enumerate(lines) if line.startswith("G1-EVENT index=12 "))
        after = next(i for i, line in enumerate(lines) if line.startswith("G1-EVENT index=27 "))
        skip = dict(zip(G1_EVENT_FIELDS, (12, 1_600_000_000, 14, 3, 1, G1_MAX, 15, 60_000_000)))
        changed = lines[:first] + ["G1-EVENT index=12 hex=" + struct.pack("<8Q", *(skip[name] for name in G1_EVENT_FIELDS)).hex()] + lines[after:]
        renumbered, number = [], 0
        for line in changed:
            if line.startswith("G1-EVENT "):
                payload = list(struct.unpack("<8Q", bytes.fromhex(line.split("hex=")[1])))
                payload[0] = number
                line = f"G1-EVENT index={number} hex=" + struct.pack("<8Q", *payload).hex()
                number += 1
            renumbered.append(line)
        renumbered = [line.replace("events=36", f"events={number}") for line in renumbered]
        renumbered = self.mutate(renumbered, "G1-HEADER", 0, G1_HEADER_FIELDS, "event_count", number)
        self.assertTrue(validate_g1_stream("\n".join(renumbered).encode(), meta)["g1_accounting_valid"])
        for field, value in (("value0", G1_MAX), ("value1", 20_000_000), ("value0", 16)):
            with self.subTest(field=field, value=value), self.assertRaises(InvalidTrace):
                validate_g1_stream("\n".join(self.mutate(renumbered, "G1-EVENT", 12, G1_EVENT_FIELDS, field, value)).encode(), meta)


    def test_per_hart_service_gap_is_not_a_peer_global_sample(self):
        lines, meta = self.fixture()
        changed = self.mutate(lines, "G1-EVENT", 33, G1_EVENT_FIELDS, "value0", 20_000_001)
        with self.assertRaisesRegex(InvalidTrace, "per-hart"):
            validate_g1_stream("\n".join(changed).encode(), meta)

    def test_release_sender_and_service_summary_are_not_guessed(self):
        lines, meta = self.fixture()
        variants = [
            [line.replace("sender_tid=10", "sender_tid=11") for line in lines],
            [line.replace("sender_cell=3", "sender_cell=4") for line in lines],
            [line.replace("sender_generation=1", "sender_generation=2") for line in lines],
            self.mutate(lines, "G1-EVENT", 33, G1_EVENT_FIELDS, "hart", 1),
            self.mutate(lines, "G1-EVENT", 33, G1_EVENT_FIELDS, "generation", 1),
            self.mutate(lines, "G1-EVENT", 33, G1_EVENT_FIELDS, "kind", 13),
        ]
        for changed in variants:
            with self.subTest(changed=changed[-1]), self.assertRaises(InvalidTrace):
                validate_g1_stream("\n".join(changed).encode(), meta)

    def test_two_cpu_boot_without_worker_overlap_does_not_qualify(self):
        lines, meta = self.fixture()
        meta = dict(meta, cpus=2, count="g1-overlap")
        lines = [line.replace("mode=yield", "mode=overlap").replace("explicit_yields=8", "explicit_yields=0")
                 for line in lines if " action=yield " not in line]
        lines = self.mutate(lines, "G1-HEADER", 0, G1_HEADER_FIELDS, "admitted_hart_mask", 3)
        lines = self.mutate(lines, "G1-HEADER", 0, G1_HEADER_FIELDS, "control_charge_ns", 2 * 1_600_000_004 - 32)
        with self.assertRaises(InvalidTrace):
            validate_g1_stream("\n".join(lines).encode(), meta)


if __name__ == "__main__":
    unittest.main()
