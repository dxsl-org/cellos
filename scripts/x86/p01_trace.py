"""Explicit private P01 v1 decoding. Structural validity and accounting are separate.

No pointers, name-based identities, missing-as-zero counters, or inferred throughput.
"""
import hashlib
import json
from pathlib import Path
import re
import struct
import uuid

EVENTS = {1: "CaptureArm", 2: "PreBoundary", 3: "LaunchIntent", 4: "LaunchResult", 5: "TaskTerminal", 6: "OwnerSlotResult", 7: "QuotaReleaseResult", 8: "ComponentStart", 9: "ComponentFinal", 10: "SnapshotGroup", 11: "Closing", 12: "CaptureFinal"}
RESULTS = {0: "absent", 1: "success", 2: "denied", 3: "unpublished_rollback", 4: "pending", 5: "retained", 6: "unknown", 7: "incomplete"}
ROUTES = {0: "absent", 1: "Elf", 2: "Path", 3: "Pinned"}
SOURCES = {0: "absent", 1: "caller_ELF", 2: "kernel_path"}
ERRORS = {
    1: dict(enumerate(("BrokenPipe", "InvalidDriverId", "InvalidCommand", "BufferTooSmall", "PermissionDenied", "FileNotFound", "TryAgain", "Unknown", "NotSupported", "InvalidInput", "OutOfMemory"))),
    2: dict(enumerate(("OutOfMemory", "InvalidArgument", "NotFound", "PermissionDenied", "AlreadyExists", "WouldBlock", "NotSupported", "IO", "InvalidInput", "IsADirectory", "NotADirectory", "Unknown"))),
}
SOURCE_OWNERS = {0: "absent", 1: "frame_allocator", 2: "kernel_heap_allocator", 3: "scheduler", 4: "private_observer", 5: "cell_quota", 6: "resource_object_ledger", 7: "retained_range_ledger"}
METRICS = ("total_frames", "used_frames", "free_frames", "page_size", "actual_heap_used_bytes", "actual_heap_free_bytes", "execution_context_quota_charge", "allocated_stack_bytes", "segment_bytes", "admitted_live_subjects", "active_operations", "reserved_completions", "outstanding_cleanup_obligations", "retained_physical_frame_union", "unknown_retained_obligations", "capture_invalid_flags")
METRIC_SOURCE_OWNERS = (1, 1, 1, 1, 2, 2, 5, 6, 6, 3, 6, 6, 6, 7, 6, 4)
SUBJECT, OWNER, PARENT, ATTEMPT, DIGEST, WORKER, GLOBAL, SNAPSHOT, COUNTER = (1 << i for i in range(9))


class InvalidTrace(ValueError):
    pass


def need(condition, reason):
    if not condition:
        raise InvalidTrace(reason)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        need(key not in result, "duplicate JSON object key")
        result[key] = value
    return result


def hexbytes(text, size):
    need(bool(re.fullmatch("[0-9a-fA-F]{" + str(size * 2) + "}", text)), "malformed fixed hex payload")
    return bytes.fromhex(text)


def words(data):
    return struct.unpack("<" + "Q" * (len(data) // 8), data)


def decode_record(data):
    need(len(data) == 160, "record length is not160")
    # Concrete offsets, not a native layout/transmute.
    row = {"sequence": struct.unpack_from("<Q", data, 0)[0],
           "event": struct.unpack_from("<H", data, 8)[0],
           "flags": struct.unpack_from("<H", data, 10)[0],
           "cpu": struct.unpack_from("<H", data, 12)[0],
           "route": data[14], "source": data[15]}
    for offset, key in ((16, "attempt"), (24, "tid"), (32, "root"), (40, "cell"), (48, "generation"), (56, "parent_tid"), (64, "parent_cell"), (72, "parent_generation"), (80, "key")):
        row[key] = struct.unpack_from("<Q", data, offset)[0]
    row.update(group=struct.unpack_from("<H", data, 88)[0], result=struct.unpack_from("<H", data, 90)[0], detail=struct.unpack_from("<I", data, 92)[0], values=list(struct.unpack_from("<4Q", data, 96)), digest=data[128:160].hex())
    need(row["event"] in EVENTS and row["result"] in RESULTS and row["route"] in ROUTES and row["source"] in SOURCES, "unknown event/result/route/source")
    flags = row["flags"]
    need(flags & ~0x1ff == 0, "reserved presence bits")
    need(not flags & WORKER, "worker outside root-only scope")
    for bit, keys in ((SUBJECT, ("tid",)), (OWNER, ("root", "cell", "generation")), (PARENT, ("parent_tid", "parent_cell", "parent_generation")), (ATTEMPT, ("attempt",))):
        if flags & bit:
            need(all(row[key] != 0 for key in keys), "missing present identity")
        else:
            need(all(row[key] == 0 for key in keys), "nonzero absent identity")
    need(bool(flags & DIGEST) == (data[128:160] != bytes(32)), "digest presence mismatch")
    if flags & SUBJECT and flags & OWNER:
        need(row["tid"] == row["root"], "non-root subject")
    expected_flags = {1: 19, 2: 3, 3: 12, 5: 3, 6: 3, 7: 3, 8: 259, 9: 259, 10: 192, 11: 3, 12: 3}
    if row["event"] == 4:
        need(flags == (31 if row["result"] == 1 else 12), "launch reserved flags")
    else:
        need(flags == expected_flags[row["event"]], "event reserved flags")
    need(row["event"] in (3, 4) or row["route"] == row["source"] == 0, "non-launch route/source reserved")
    need(row["event"] == 10 or row["detail"] == 0, "reserved event detail")
    need(row["event"] in (4, 8, 9, 10) or row["group"] == 0, "reserved group")
    if row["event"] in (3, 4):
        need(row["route"] in (1, 2, 3) and row["source"] == (1 if row["route"] == 1 else 2), "route/source mismatch")
    if row["event"] == 3:
        need(flags & (PARENT | ATTEMPT) == PARENT | ATTEMPT and not flags & (SUBJECT | OWNER | DIGEST), "launch intent identity flags")
        need(row["route"] != 0 and row["source"] != 0 and row["result"] == 4, "launch intent mappings")
    if row["event"] == 4:
        need(flags & (PARENT | ATTEMPT) == PARENT | ATTEMPT, "launch result identity flags")
        need(row["result"] in (1, 2, 3), "launch result category")
        if row["result"] == 1:
            need(flags & (SUBJECT | OWNER | DIGEST) == SUBJECT | OWNER | DIGEST, "publication required flags")
        else:
            need(not flags & (SUBJECT | OWNER | DIGEST), "failure invented child")
            need(row["group"] in ERRORS and row["values"][0] in ERRORS[row["group"]], "unknown launch source error")
            need(row["values"][1:] == [0, 0, 0], "reserved launch error values")
            row["source_error"] = ERRORS[row["group"]][row["values"][0]]
            need((row["result"] == 2) == (row["source_error"] == "PermissionDenied"), "source denial/result mismatch")
    if row["event"] in (5, 6, 7, 8, 9):
        need(flags & (SUBJECT | OWNER) == SUBJECT | OWNER, "lifecycle owner absent")
        need(row["result"] != 0, "lifecycle result absent")
    if row["event"] in (8, 9):
        need(1 <= row["group"] <= 8, "component class outside1..8")
        if row["event"] == 9:
            need(flags & COUNTER, "component counter sidecar absent")
    if row["event"] == 10:
        need(flags & SNAPSHOT and flags & GLOBAL, "snapshot classification/sidecar absent")
        need(not flags & (SUBJECT | OWNER | DIGEST | ATTEMPT | PARENT | COUNTER), "snapshot fabricated owner")
        need(0 <= row["group"] < 4 and 1 <= row["key"] <= 6, "snapshot sample/group")
        need(row["detail"] & ~15 == 0, "snapshot reserved bitmap")
        for index, value in enumerate(row["values"]):
            need(row["detail"] & (1 << index) or value == 0, "absent metric carries value")
    if row["event"] in (1, 3, 5, 6, 7, 8, 11) or row["event"] == 4 and row["result"] == 1:
        need(row["values"] == [0, 0, 0, 0], "reserved event values")
    if row["event"] in (1, 2, 3, 11, 12):
        need(row["key"] == 0, "reserved control key")
    if row["event"] == 12:
        need(row["values"][1:] == [0, 0, 0], "reserved final values")
    allowed_results = {1: (1,), 2: (1,), 3: (4,), 5: (1, 5, 6, 7), 6: (1, 5, 6, 7), 7: (1, 5, 6, 7), 8: (4,), 9: (1, 5, 6, 7), 10: (1,), 11: (4,), 12: (1, 7)}
    if row["event"] != 4:
        need(row["result"] in allowed_results[row["event"]], "event/result mapping")
    row["event_name"] = EVENTS[row["event"]]
    row["result_name"] = RESULTS[row["result"]]
    return row


def identity(row):
    return (row["tid"], row["root"], row["cell"], row["generation"])


def decode_stream(raw):
    records, subjects, counters, snapshots, obligations = [], {}, {}, {}, {}
    final, late, manifests, workloads = [], [], [], []
    stages = {}
    peaks, storage = {}, []
    for line in raw.decode("utf-8", errors="replace").splitlines():
        # Kernel console may prefix a timestamp; recognize only the explicit tag.
        position = line.find("P01-")
        if position < 0:
            continue
        text = line[position:].strip()
        parts = text.split()
        tag = parts[0]
        if tag == "P01-RECORD":
            need(len(parts) == 2, "record line shape")
            records.append(decode_record(hexbytes(parts[1], 160)))
        elif tag == "P01-SUBJECT":
            need(len(parts) == 3, "subject line shape")
            index = int(parts[1]); need(0 <= index < 9 and index not in subjects, "duplicate/out-of-bounds subject")
            subjects[index] = words(hexbytes(parts[2], 64))
        elif tag == "P01-COMPONENT":
            need(len(parts) == 4, "component line shape")
            key = (int(parts[1]), int(parts[2])); need(0 <= key[0] < 9 and 1 <= key[1] <= 8 and key not in counters, "duplicate/out-of-bounds counter")
            counters[key] = words(hexbytes(parts[3], 64))
        elif tag == "P01-SNAPSHOT":
            need(len(parts) == 4, "snapshot sidecar shape")
            key = (int(parts[1]), int(parts[2])); need(1 <= key[0] <= 6 and 0 <= key[1] < 16 and key not in snapshots, "duplicate/out-of-bounds snapshot sidecar")
            data = hexbytes(parts[3], 24)
            before, after, status, owner, reserved = struct.unpack("<QQIHH", data)
            need(status in (1, 2, 3, 4) and reserved == 0, "snapshot status/reserved")
            need(owner in SOURCE_OWNERS, "unknown snapshot source owner")
            need(owner == METRIC_SOURCE_OWNERS[key[1]] or owner == 0 and status != 1,
                 "snapshot metric/source owner mismatch")
            need(status != 1 or before == after and owner != 0, "invalid coherent source epoch")
            snapshots[key] = (before, after, status, owner)
        elif tag == "P01-OBLIGATION":
            need(len(parts) == 3, "obligation shape")
            index = int(parts[1]); need(0 <= index < 128 and index not in obligations, "duplicate/out-of-bounds obligation")
            data = hexbytes(parts[2], 64)
            obligations[index] = {"words": words(data[:40]), "component": struct.unpack_from("<H", data, 40)[0], "state": struct.unpack_from("<H", data, 42)[0], "flags": struct.unpack_from("<I", data, 44)[0], "created": struct.unpack_from("<Q", data, 48)[0], "terminal": struct.unpack_from("<Q", data, 56)[0]}
            entry = obligations[index]
            need(entry["flags"] == 0 and 1 <= entry["component"] <= 8 and entry["state"] in range(1, 8), "obligation reserved/class/state")
            need(all(entry["words"][:4]) and entry["created"] > 0, "obligation identity/token absent")
        elif tag == "P01-MANIFEST":
            need(len(parts) == 2, "manifest shape")
            manifests.append(hexbytes(parts[1], 96).hex())
        elif tag == "P01-FINAL":
            match = re.fullmatch(r"P01-FINAL reserved=(\d+) ready=(\d+) active=(\d+) flags=(\d+) state=closed", text)
            need(match is not None, "final header malformed")
            final.append(tuple(map(int, match.groups())))
        elif tag == "P01-LATE":
            match = re.fullmatch(r"P01-LATE flags=(\d+)", text)
            need(match is not None, "late status malformed")
            late.append(int(match.group(1)))
        elif tag == "P01-STAGE":
            match = re.fullmatch(r"P01-STAGE sample=([1-6]) tick=(\d+)", text)
            need(match is not None, "sample timing malformed")
            sample, tick = map(int, match.groups())
            need(sample not in stages, "duplicate stage timing")
            stages[sample] = tick
        elif tag == "P01-PEAK":
            need(len(parts) == 3, "peak sidecar shape")
            index = int(parts[1])
            need(index in (0, 1) and index not in peaks, "duplicate/unknown peak metric")
            value, before, after, status, owner, reserved = struct.unpack("<QQQIHH", hexbytes(parts[2], 32))
            need(status in (1, 2, 3, 4) and owner in SOURCE_OWNERS and reserved == 0, "peak status/source/reserved")
            need(status != 1 or before == after and owner == (2 if index == 0 else 1), "incoherent peak source")
            peaks[index] = {"value": value if status == 1 else None, "epoch_before": before, "epoch_after": after, "status": status, "source_owner": owner, "units": "actual_global_heap_used_bytes" if index == 0 else "global_allocator_used_frames"}
        elif tag == "P01-STORAGE":
            match = re.fullmatch(r"P01-STORAGE payload=(\d+) core_static=(\d+) resource_static=(\d+) exporter_buffer=(\d+)", text)
            need(match is not None, "trace overhead header malformed")
            storage.append(tuple(map(int, match.groups())))
        elif tag == "P01-WORKLOAD":
            workload = json.loads(text[len(tag):].strip(), object_pairs_hook=unique_object)
            need(isinstance(workload, dict), "workload outcome is not an object")
            workloads.append(workload)
    return records, subjects, counters, snapshots, obligations, final, late, manifests, workloads, stages, peaks, storage


def validate_export_order(raw):
    payload = {"P01-RECORD", "P01-SUBJECT", "P01-COMPONENT", "P01-SNAPSHOT",
               "P01-OBLIGATION", "P01-MANIFEST", "P01-STAGE", "P01-PEAK", "P01-STORAGE", "P01-WORKLOAD",
               "P01-HEAP-ORIGIN", "P01-HEAP-ENTRY", "P01-HEAP-BINDING", "P01-HEAP-SOURCE",
               "P01-HEAP-FAULT",
               "P01-HEAP-BASELINE-SOURCE", "P01-HEAP-BASELINE-ENTRY",
               "P01-HEAP-INCARNATION", "P01-HEAP-ACCOUNTING",
               "P01-HEAP-BASELINE-CHARGED-SOURCE", "P01-HEAP-BASELINE-CHARGED-ENTRY",
               "P01-HEAP-CHARGED-SOURCE", "P01-HEAP-CHARGED-ENTRY",
               "P01-HEAP-STAGE-SOURCE", "P01-HEAP-STAGE-ENTRY", "P01-HEAP-STAGE-CHARGED-ENTRY",
               "P01-RETAINED-RANGE", "P01-RETAINED-SOURCE",
               "P01-PHYSICAL-ENTRY", "P01-PHYSICAL-SOURCE",
               "P01-TOPOLOGY-SOURCE", "P01-TOPOLOGY-ROOT",
               "P01-TOPOLOGY-NODE", "P01-TOPOLOGY-EDGE",
               "P01-NONCPU-SOURCE", "P01-NONCPU-ROOT", "P01-NONCPU-REFERENCE",
               "P01-XTREE-SOURCE", "P01-XTREE-NODE", "P01-XTREE-EDGE",
               "P01-VTD-SOURCE", "P01-VTD-NODE", "P01-VTD-EDGE"}
    final_seen = False
    late_seen = False
    closing_seen = False
    workload_seen = 0
    for line in raw.decode("utf-8", errors="replace").splitlines():
        if re.search(r"\bP01:closing\s*$", line):
            need(not closing_seen and not final_seen and workload_seen == 1, "closing marker missing/duplicate or precedes workload outcome")
            closing_seen = True
        position = line.find("P01-")
        if position < 0:
            continue
        tag = line[position:].split()[0]
        if tag in payload:
            need(not final_seen, "immutable payload after final status")
            if tag == "P01-WORKLOAD":
                need(not closing_seen, "workload outcome after closing marker")
                workload_seen += 1
        elif tag == "P01-FINAL":
            need(not final_seen, "duplicate final status")
            final_seen = True
        elif tag == "P01-LATE":
            need(final_seen, "late validity status precedes final status")
            late_seen = True
    need(final_seen and late_seen, "missing final/post-final validity status")
    need(closing_seen, "missing workload closing marker")

def ownership_evidence(raw, subjects, records):
    sizes = {"P01-HEAP-ORIGIN": 96, "P01-HEAP-ENTRY": 152,
             "P01-HEAP-BINDING": 48, "P01-HEAP-SOURCE": 48,
             "P01-HEAP-FAULT": 48,
             "P01-HEAP-BASELINE-SOURCE": 96, "P01-HEAP-BASELINE-ENTRY": 160,
             "P01-HEAP-INCARNATION": 16, "P01-HEAP-ACCOUNTING": 64,
             "P01-HEAP-BASELINE-CHARGED-SOURCE": 176, "P01-HEAP-BASELINE-CHARGED-ENTRY": 40,
             "P01-HEAP-CHARGED-SOURCE": 176, "P01-HEAP-CHARGED-ENTRY": 40,
             "P01-HEAP-STAGE-SOURCE": 176, "P01-HEAP-STAGE-ENTRY": 152,
             "P01-HEAP-STAGE-CHARGED-ENTRY": 40,
             "P01-RETAINED-RANGE": 72, "P01-RETAINED-SOURCE": 48}
    payloads = {tag: [] for tag in sizes}
    for line in raw.decode("utf-8", errors="replace").splitlines():
        match = re.search(r"\b(P01-HEAP-\S+|P01-RETAINED-\S+)(.*)$", line)
        if not match:
            continue
        tag, rest = match.groups()
        need(tag in sizes, "unknown ownership payload")
        fields = rest.split()
        if tag.startswith("P01-HEAP-STAGE-"):
            need(len(fields) == 2 and re.fullmatch(r"[2-6]", fields[0]) is not None,
                 "malformed heap stage sample/payload")
            need(re.fullmatch(r"[0-9a-f]+", fields[1]) is not None, "malformed lowercase heap stage hex")
            payloads[tag].append((int(fields[0]), words(hexbytes(fields[1], sizes[tag]))))
        else:
            need(len(fields) == 1, "malformed ownership payload")
            payloads[tag].append(words(hexbytes(fields[0], sizes[tag])))
    for tag in ("P01-HEAP-ORIGIN", "P01-HEAP-SOURCE", "P01-RETAINED-SOURCE"):
        need(len(payloads[tag]) == 1, "missing/duplicate ownership source")
    h = payloads["P01-HEAP-ORIGIN"][0]
    hs = payloads["P01-HEAP-SOURCE"][0]
    s = payloads["P01-RETAINED-SOURCE"][0]
    entries = payloads["P01-HEAP-ENTRY"]
    bindings = payloads["P01-HEAP-BINDING"]
    ranges = payloads["P01-RETAINED-RANGE"]
    need(h[0] > 0 and all(v in (0, 1) for v in h[1:4]) and h[11] == 1024
         and h[4] == len(entries) <= h[11], "heap origin header malformed")
    # This export protocol contains origin/transfer witnesses, not a complete
    # semantic-owner inventory or a proven maximum simultaneous-allocation bound.
    # A producer cannot promote either bit without an accompanying proof schema.
    need(h[2:4] == (0, 0), "unsupported heap fit/coverage proof claim")
    need(hs[0] == hs[1] == h[0] and hs[2] == h[1] and hs[3] == len(entries)
         and hs[4] == len(bindings) <= hs[5] == 10, "heap enumeration count/epoch incoherent")
    faults = payloads["P01-HEAP-FAULT"]
    need(len(faults) <= 1, "duplicate heap first-fault diagnostic")
    if faults:
        fault = faults[0]
        need(fault[0] == 1 and 0 < fault[1] < 128 and fault[2] > 0
             and 0 < fault[3] <= h[0] and fault[4] in (0, 1, 2) and h[1] == 1,
             "malformed/inconsistent heap first-fault diagnostic")
    need(s[2] in (1, 2, 3, 4) and s[3] == 7 and s[4] == len(ranges) <= s[5] == 2048,
         "retained range source malformed")
    need(s[0] > 0 and s[1] >= s[0] and (s[2] != 1 or s[0] == s[1]),
         "retained range epoch incoherent")
    owners = {tuple(row[:4]) for row in subjects.values()}
    selected_tids = {owner[0] for owner in owners}
    results = {row["attempt"]: row for row in records if row["event"] == 4}
    joined = {}
    for binding in bindings:
        attempt, *tail = binding
        owner, closed = tuple(tail[:4]), tail[4]
        need(attempt > 0 and attempt not in joined and closed in (0, 1) and attempt in results,
             "duplicate/unknown heap preparation binding")
        need(owner == (0, 0, 0, 0) or owner in owners and owner[0] == owner[1],
             "heap binding owner is not an exact root subject")
        result = results[attempt]
        need((result["result"] == 1 and owner == identity(result))
             or (result["result"] != 1 and not any(owner)), "heap attempt/publication join mismatch")
        joined[attempt] = (owner, closed)
    need(all(row["result"] != 1 or attempt in joined for attempt, row in results.items()),
         "missing published heap preparation binding")
    pointers, partitions, requested, live_attempts = validate_heap_entries(entries, owners, selected_tids, joined)
    unresolved = sum(not closed or (not any(owner) and attempt in live_attempts)
                     for attempt, (owner, closed) in joined.items())
    need(requested < 1 << 64 and requested == h[5] and tuple(partitions) == h[6:10]
         and unresolved == h[10], "heap raw/summary partitions mismatch")
    baseline = heap_baseline_evidence(payloads, entries, h, owners, selected_tids, joined)
    charged = heap_charged_evidence(payloads, baseline, entries, h)
    heap_stages = heap_stage_evidence(payloads, baseline, charged, entries, h, owners, selected_tids, joined)
    reasons = set(range(1, 8)) | set(range(10, 18))
    tokens = set()
    for r in ranges:
        need(r[0] > 0 and r[0] not in tokens and tuple(r[1:5]) in owners,
             "duplicate/unknown retained range owner")
        tokens.add(r[0])
        need(r[5] % 4096 == r[6] % 4096 == 0 and r[6] > r[5]
             and r[7] in reasons and r[8] in reasons, "retained range boundary/reason")
    total, end, owner = 0, 0, None
    for r in sorted(ranges, key=lambda r: r[5]):
        if r[5] < end:
            need(tuple(r[1:5]) == owner or s[2] != 1, "retained range overlapping owners")
            if r[6] > end:
                total += r[6] - end
                end = r[6]
        else:
            total += r[6] - r[5]
            end, owner = r[6], tuple(r[1:5])
    need(total < 1 << 64, "retained union overflow")
    return {"heap_origin": list(h), "heap_source": list(hs),
            "heap_entries": [list(entry) for entry in entries],
            "heap_bindings": [list(binding) for binding in bindings],
            "heap_first_fault": dict(zip(("version", "bits", "line", "epoch", "phase", "hart"), faults[0])) if faults else None,
            "retained_source": list(s), "retained_ranges": [list(r) for r in ranges],
            "heap_baseline": baseline,
            "heap_charged": charged, "heap_stages": heap_stages,
            "retained_union_bytes": total if s[2] == 1 else None}

def validate_heap_entries(entries, owners, selected_tids, joined):
    # Shared validation for every observed allocation census.
    pointers, partitions = set(), [0, 0, 0, 0]
    requested = 0
    live_attempts = set()
    for entry in entries:
        ptr, size, align, kind, origin_attempt, adopted = entry[:6]
        owner = tuple(entry[6:10])
        semantic_kind, semantic_owner, transferred_from = entry[10], tuple(entry[11:15]), tuple(entry[15:19])
        need(ptr > 0 and ptr not in pointers and size > 0 and align > 0
             and align & (align - 1) == 0 and ptr % align == 0
             and ptr + size <= 1 << 64 and kind in (1, 2, 3, 4, 5),
             "heap allocation boundary/origin malformed")
        pointers.add(ptr)
        need((kind == 3 and owner in owners and owner[0] == owner[1])
             or (kind != 3 and not any(owner)), "heap allocation origin owner mismatch")
        need((kind == 4 and origin_attempt in joined)
             or (kind != 4 and origin_attempt == 0), "heap allocation origin attempt mismatch")
        need(adopted == 0 or adopted in joined, "heap transfer owner attempt mismatch")
        shared_kinds = (6, 7, 8, 9, 10, 12, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 47)
        owner_kinds = (1, 2, 3, 4, 5, 11, 13, 14, 15, 64, 65, 66, 67, 68)
        need(semantic_kind in (0, *shared_kinds, *owner_kinds, *(kind | 16 for kind in owner_kinds)),
             "unknown heap semantic object kind")
        object_kind = semantic_kind & ~16
        outside = bool(semantic_kind & 16)
        if object_kind == 0:
            need(not any(semantic_owner + transferred_from), "origin-only allocation claims semantic owner")
        elif object_kind in shared_kinds:
            need(not outside and adopted == 0 and not any(semantic_owner + transferred_from),
                 "shared kernel backing claims exclusive task owner")
            if object_kind == 12:
                need(align == 1 and size <= 4096, "synthetic wire layout mismatch")
        else:
            if outside:
                need(semantic_owner not in owners and semantic_owner[0] > 0 and semantic_owner[1] > 0
                     and semantic_owner[0] not in selected_tids and adopted == 0,
                     "outside-selected heap owner claims selected incarnation")
            else:
                need(semantic_owner in owners and semantic_owner[0] == semantic_owner[1],
                     "heap semantic owner is not an exact selected subject")
                expected_attempt = [attempt for attempt, (bound, _) in joined.items() if bound == semantic_owner]
                need(len(expected_attempt) <= 1 and adopted == (expected_attempt[0] if expected_attempt else 0),
                     "heap semantic owner/bound attempt mismatch")
            need((object_kind == 4 and transferred_from[0] > 0 and transferred_from[1] > 0
                  and (transferred_from in owners
                       or transferred_from[0] not in selected_tids))
                 or (object_kind != 4 and not any(transferred_from)),
                 "heap wire transfer lacks exact sender witness")
            if object_kind in (2, 4, 5):
                need(align == 1 and size <= 4096 and (object_kind != 2 or size == 4096),
                     "heap wire semantic layout mismatch")
        if origin_attempt:
            live_attempts.add(origin_attempt)
        if adopted:
            live_attempts.add(adopted)
        if kind == 4 or adopted:
            partitions[0] += 1
        elif kind in (1, 2, 5):
            partitions[{1: 1, 2: 2, 5: 3}[kind]] += 1
        requested += size
    # Distinct backing allocations cannot overlap, unlike retained-frame ranges.
    end = 0
    for entry in sorted(entries):
        need(entry[0] >= end, "heap backing allocations overlap")
        end = entry[0] + entry[1]
    return pointers, partitions, requested, live_attempts


def heap_baseline_evidence(payloads, entries, final_origin, owners, selected_tids, joined):
    sources = payloads["P01-HEAP-BASELINE-SOURCE"]
    baseline = payloads["P01-HEAP-BASELINE-ENTRY"]
    incarnations = payloads["P01-HEAP-INCARNATION"]
    accounting = payloads["P01-HEAP-ACCOUNTING"]
    if not (sources or baseline or incarnations or accounting):
        return None  # Historical final-only captures cannot supply a sample1 map.
    need(len(sources) == len(accounting) == 1, "missing/duplicate heap baseline/accounting source")
    source, final = sources[0], accounting[0]
    version, sample, epoch, invalid, count, requested, allocator_epoch, used, free, owner, capacity, static = source
    need(version == sample == 1 and 0 < epoch <= final_origin[0] and invalid == 0
         and count == len(baseline) <= capacity == 1024 and allocator_epoch > 0
         and owner == 2 and static >= capacity * 160, "heap baseline source malformed")
    need(final[:4] == final[4:] and final[0] == final_origin[0]
         and final[1] >= allocator_epoch, "heap final allocator relation incoherent")
    rows = [row[1:] for row in baseline]
    _, _, total, _ = validate_heap_entries(rows, owners, selected_tids, joined)
    need(total == requested and total < 1 << 64, "heap baseline requested/count mismatch")
    before, after, final_by_ptr = {}, {}, {entry[0]: entry for entry in entries}
    for row in baseline:
        incarnation, *entry = row
        need(0 < incarnation <= epoch and incarnation not in before, "heap baseline incarnation malformed")
        before[incarnation] = tuple(entry)
    need(len(incarnations) == len(entries), "heap final incarnation enumeration truncated")
    final_ptrs = set()
    for ptr, incarnation in incarnations:
        need(ptr in final_by_ptr and ptr not in final_ptrs and 0 < incarnation <= final[0]
             and incarnation not in after, "heap final incarnation malformed")
        final_ptrs.add(ptr)
        after[incarnation] = final_by_ptr[ptr]
        if incarnation in before:
            need(before[incarnation][:3] == after[incarnation][:3]
                 and before[incarnation][3:5] == after[incarnation][3:5]
                 and before[incarnation][6:10] == after[incarnation][6:10],
                 "preserved heap incarnation changed Layout/origin")
        else:
            need(incarnation > epoch, "heap new incarnation predates baseline")
    created, released, preserved = set(after) - set(before), set(before) - set(after), set(before) & set(after)
    delta = sum(after[token][1] for token in created) - sum(before[token][1] for token in released)
    need(delta == final_origin[5] - requested, "heap requested delta mismatch")
    semantic = {}
    for sign, inventory in ((-1, before), (1, after)):
        for entry in inventory.values():
            key = ":".join(str(v) for v in (entry[10], *entry[11:15]))
            semantic[key] = semantic.get(key, 0) + sign * entry[1]
    def exported(tokens, inventory):
        return [{"incarnation": token, "entry": list(inventory[token])} for token in sorted(tokens)]
    return {"source": list(source), "final_accounting": list(final),
            "entries": [list(row) for row in baseline],
            "created": exported(created, after), "released": exported(released, before),
            "preserved": [{"incarnation": token, "baseline": list(before[token]), "final": list(after[token])}
                          for token in sorted(preserved)],
            "created_requested_bytes": sum(after[token][1] for token in created),
            "released_requested_bytes": sum(before[token][1] for token in released),
            "preserved_requested_bytes": sum(before[token][1] for token in preserved),
            "semantic_requested_byte_deltas": semantic, "requested_byte_delta": delta,
            "allocator_used_byte_delta": final[2] - used,
            "unmatched_allocator_used_byte_delta": final[2] - used - delta,
            "allocator_used_equals_requested_proven": False,
            "fit_proven": False, "coverage_proven": False, "raw_residual_accepted": False}

def heap_charged_evidence(payloads, baseline, entries, final_origin):
    tags = ("P01-HEAP-BASELINE-CHARGED-SOURCE", "P01-HEAP-BASELINE-CHARGED-ENTRY",
            "P01-HEAP-CHARGED-SOURCE", "P01-HEAP-CHARGED-ENTRY")
    if not any(payloads[tag] for tag in tags):
        return None  # Preserve immutable historical requested-only captures.
    need(baseline is not None and all(len(payloads[tag]) == 1 for tag in (tags[0], tags[2])),
         "partial/duplicate heap charged sources")
    inventories = []
    sources = []
    for is_baseline, source_tag, entry_tag in ((True, tags[0], tags[1]), (False, tags[2], tags[3])):
        source = payloads[source_tag][0]
        rows = payloads[entry_tag]
        legacy_source = baseline["source"] if is_baseline else baseline["final_accounting"][:4]
        raw = {row[0]: row for row in (list(row[1:]) for row in baseline["entries"])} if is_baseline else {row[0]: row for row in entries}
        tokens = {row[1]: row[0] for row in baseline["entries"]} if is_baseline else {row[0]: row[1] for row in payloads["P01-HEAP-INCARNATION"]}
        expected = (legacy_source[2], legacy_source[6], legacy_source[7], legacy_source[8]) if is_baseline else tuple(legacy_source)
        inventory, _ = validate_heap_census(source, rows, raw, tokens, int(is_baseline),
                                           1 if is_baseline else 2, expected)
        inventories.append(inventory)
        sources.append(source)
    before, after = ({row[0]: row[1:] for row in inventory.values()} for inventory in inventories)
    for token in before.keys() & after.keys():
        need(before[token] == after[token], "preserved heap charged birth changed")
    created, released = after.keys() - before.keys(), before.keys() - after.keys()
    delta = sum(after[token][0] for token in created) - sum(before[token][0] for token in released)
    need(delta == sources[1][5] - sources[0][5], "heap charged allocator delta mismatch")
    need(sources[1][14] == sources[0][14] and sources[1][16] >= sources[0][16],
         "heap charged prearm/highwater changed incoherently")
    return {"baseline_source": list(sources[0]), "final_source": list(sources[1]),
            "baseline_entries": [list(row) for row in payloads[tags[1]]],
            "final_entries": [list(row) for row in payloads[tags[3]]],
            "charged_byte_delta": delta, "unmatched_allocator_used_byte_delta": 0,
            "baseline_partitions": dict(zip(("unowned", "shared", "exclusive"), sources[0][11:14])),
            "final_partitions": dict(zip(("unowned", "shared", "exclusive"), sources[1][11:14])),
            "prearm_allocations": sources[1][14], "prearm_live_objects": sources[1][15],
            "measured_live_highwater": sources[1][16], "allocation_coverage_observed": True,
            "semantic_coverage_proven": False, "coverage_proven": False, "fit_proven": False}


def validate_heap_census(source, rows, raw, tokens, sample, phase, expected):
    """One charged-source schema for baseline, live stages, and frozen final."""
    need(source[0] == 1 and source[1] == sample and source[2] == phase
         and source[3] > 0 and source[4] > 0 and source[17] == source[20] == 0 and source[21] == 2
         and source[18] == 1024 and source[19] >= 2 * 1024 * 160,
         "heap charged source malformed/incomplete")
    need(tuple(source[3:7]) == tuple(expected) and source[7] == len(rows) == len(raw)
         and source[8] == sum(row[1] for row in raw.values()), "heap charged source/raw relation mismatch")
    need(source[7] <= source[16] <= source[18] and source[15] <= source[14]
         and source[15] <= source[7] and source[10] == 0, "heap charged counts/unmatched malformed")
    inventory, incarnations, partitions, prearm = {}, set(), [0, 0, 0], 0
    for ptr, incarnation, charged, birth_epoch, boot in rows:
        need(ptr in raw and ptr not in inventory and 0 < incarnation <= source[3]
             and incarnation not in incarnations and (tokens is None or incarnation == tokens.get(ptr))
             and charged >= raw[ptr][1] and 0 < birth_epoch <= source[4] and boot in (0, 1),
             "heap charged entry/incarnation malformed")
        incarnations.add(incarnation)
        semantic_kind = raw[ptr][10] & ~16
        partition = 0 if semantic_kind == 0 else (1 if not any(raw[ptr][11:15]) else 2)
        # An exclusive kernel task may have cell/generation0 but nonzero tid.
        partitions[partition] += charged
        prearm += boot
        inventory[ptr] = (incarnation, charged, birth_epoch, boot)
    total = sum(row[1] for row in inventory.values())
    need(total < 1 << 64 and total == source[9] == source[5]
         and tuple(partitions) == source[11:14] and sum(partitions) == total
         and prearm == source[15], "heap charged totals/partitions mismatch")
    return inventory, partitions


def heap_stage_evidence(payloads, baseline, charged, entries, final_origin, owners, selected_tids, joined):
    tags = ("P01-HEAP-STAGE-SOURCE", "P01-HEAP-STAGE-ENTRY", "P01-HEAP-STAGE-CHARGED-ENTRY")
    if not any(payloads[tag] for tag in tags):
        return None  # Historical captures retain exactly their old qualification.
    sources, raw_rows, charged_rows = {}, {}, {}
    for sample, source in payloads[tags[0]]:
        need(sample not in sources, "duplicate heap stage source")
        sources[sample] = source
    for tag, grouped in ((tags[1], raw_rows), (tags[2], charged_rows)):
        for sample, row in payloads[tag]:
            need(sample in sources, "heap stage entries lack source")
            grouped.setdefault(sample, []).append(row)
    need(bool(sources), "missing heap stage source")
    stages, snapshots = {}, []
    if baseline is not None:
        raw = {row[1]: tuple(row[1:]) for row in baseline["entries"]}
        tokens = {row[1]: row[0] for row in baseline["entries"]}
        charges = {row[0]: tuple(row[2:]) for row in charged["baseline_entries"]} if charged else {}
        snapshots.append((baseline["source"][2], baseline["source"][6], raw, tokens, charges,
                          charged["baseline_source"] if charged else None))
    for sample, source in sorted(sources.items()):
        rows, charges = raw_rows.get(sample, []), charged_rows.get(sample, [])
        _, _, requested, _ = validate_heap_entries(rows, owners, selected_tids, joined)
        need(source[3] <= final_origin[0] and requested < 1 << 64,
             "heap stage ledger epoch/requested malformed")
        raw = {row[0]: row for row in rows}
        inventory, partitions = validate_heap_census(source, charges, raw, None, sample, 1, source[3:7])
        tokens = {ptr: row[0] for ptr, row in inventory.items()}
        charge_by_ptr = {ptr: row[1:] for ptr, row in inventory.items()}
        snapshots.append((source[3], source[4], raw, tokens, charge_by_ptr, source))
        stages[str(sample)] = {
            "source": list(source), "entries": [list(row) for row in rows],
            "charged_entries": [list(row) for row in charges],
            "partitions": dict(zip(("unowned", "shared", "exclusive"), partitions)),
            "source_compatible": {"allocator_epoch": source[4], "allocator_used_bytes": source[5],
                                  "allocator_free_bytes": source[6]},
            "semantic_classification_observed": True, "semantic_coverage_proven": False,
            "coverage_proven": False, "fit_proven": False,
        }
    if baseline is not None:
        raw = {row[0]: row for row in entries}
        tokens = {ptr: token for ptr, token in payloads["P01-HEAP-INCARNATION"]}
        charges = {row[0]: tuple(row[2:]) for row in charged["final_entries"]} if charged else {}
        snapshots.append((baseline["final_accounting"][0], baseline["final_accounting"][1],
                          raw, tokens, charges, charged["final_source"] if charged else None))
    seen, seen_charges, previous, previous_source, previous_tokens, previous_inventory = {}, {}, None, None, None, None
    for epoch, allocator_epoch, raw, tokens, charges, source in snapshots:
        need(previous is None or epoch >= previous[0] and allocator_epoch >= previous[1],
             "heap stage source epochs out of order")
        if previous_tokens is not None:
            need(all(token in previous_tokens or token > previous[0] for token in tokens.values()),
                 "heap stage new incarnation predates previous census")
            need(all(token in previous_tokens or ptr not in charges
                     or charges[ptr][1] > previous[1] and charges[ptr][2] == 0
                     for ptr, token in tokens.items()),
                 "heap stage new allocation birth predates previous census")
        previous_tokens = set(tokens.values())
        previous = (epoch, allocator_epoch)
        if source is not None:
            need(previous_source is None or source[14] == previous_source[14]
                 and source[16] >= previous_source[16] and source[19] == previous_source[19],
                 "heap charged prearm/highwater/static changed incoherently")
            previous_source = source
        for ptr, token in tokens.items():
            row = raw[ptr]
            invariant = tuple(row[:5]) + tuple(row[6:10])
            need(token not in seen or seen[token] == invariant,
                 "preserved heap stage incarnation changed Layout/origin")
            seen[token] = invariant
            if ptr in charges:
                need(token not in seen_charges or seen_charges[token] == tuple(charges[ptr]),
                     "preserved heap stage charged birth changed")
                seen_charges[token] = tuple(charges[ptr])
        inventory = (raw, tokens, charges)
        need(previous_inventory is None or epoch != previous_inventory[0]
             or inventory == previous_inventory[1],
             "heap stage inventory changed without ledger epoch advance")
        previous_inventory = (epoch, inventory)
    return stages


def physical_evidence(raw, records):
    sizes = {"P01-PHYSICAL-ENTRY": 96, "P01-PHYSICAL-SOURCE": 80}
    payloads = {tag: [] for tag in sizes}
    for line in raw.decode("utf-8", errors="replace").splitlines():
        match = re.search(r"\b(P01-PHYSICAL-\S+)(.*)$", line)
        if not match:
            continue
        tag, rest = match.groups()
        fields = rest.split()
        need(tag in sizes and len(fields) == 1, "malformed physical inventory payload")
        payloads[tag].append(words(hexbytes(fields[0], sizes[tag])))
    headers, entries = payloads["P01-PHYSICAL-SOURCE"], payloads["P01-PHYSICAL-ENTRY"]
    if not headers and not entries:
        return None
    need(len(headers) == 1, "missing/duplicate physical inventory source")
    source = headers[0]
    version, before, after, status, owner, count, capacity, supported, gaps, live = source
    need(version == 1 and before > 0 and after >= before and status in (1, 2, 3, 4)
         and (status != 1 or before == after) and owner == 9 and count == len(entries)
         and count <= capacity == 8192 and supported == gaps == 7,
         "physical inventory source/schema incoherent")
    tokens, live_frames, histories = set(), set(), {}
    attempts = {row["attempt"] for row in records if row["event"] == 3}
    for entry in entries:
        token, frame, created, freed, attempt, kind, root, va, state, touches, failed, load_base = entry
        census = kind in (6, 7)
        birth_bound = load_base if census else created
        need(token > 0 and token not in tokens and frame % 4096 == 0
             and frame + 4096 <= 1 << 64 and kind in range(8)
             and ((census and created == attempt == 0 and load_base > 0)
                  or (not census and created > 0 and (attempt == 0 or attempt in attempts)))
             and (freed == 0 or freed > birth_bound),
             "physical incarnation identity/lifetime malformed")
        tokens.add(token)
        if freed == 0:
            need(frame not in live_frames, "duplicate live physical frame incarnation")
            live_frames.add(frame)
        histories.setdefault(frame, []).append((birth_bound, freed))
        if kind in (1, 5, 7):
            need(root > 0 and root % 4096 == va % 4096 == 0 and state in (1, 2)
                 and (census or load_base == 0) and (kind == 1 or va == 0)
                 and (kind == 1 or touches > 0 or failed > 0)
                 and (freed == 0 or state == 2), "shared table witness malformed")
        elif kind in (2, 3, 4):
            need(root == state == touches == failed == 0 and va % 4096 == load_base % 4096 == 0,
                 "prepublication failure-frame witness malformed")
        else:
            need(root == va == state == touches == failed == 0 and (census or load_base == 0),
                 "generic physical frame claims unsupported semantic witness")
    for history in histories.values():
        ordered = sorted(history)
        need(all(previous[1] != 0 and previous[1] < following[0]
                 for previous, following in zip(ordered, ordered[1:])),
             "physical frame incarnation lifetimes overlap")
    need(live == len(live_frames), "physical inventory live count mismatch")
    return {"source": list(source), "entries": [list(entry) for entry in entries],
            "source_owner": "private_physical_inventory",
            "supported_partitions": ["baseline_bitmap_census_unknown_birth", "bitmap_incarnations", "SAS_map_prune", "post_load_failure_edges"],
            "remaining_gaps": ["generic_frame_semantic_owners", "other_mapping_roots_sources", "complete_reference_topology"],
            "coverage_proven": False, "fit_proven": False,
            "metric13_promoted": False}

def cpu_topology_evidence(raw, subjects, records, physical, sidecars, stage_metrics):
    """Current CPU descriptors are observations, never semantic owner/fit proofs.

    Root rows name references to a graph, not exclusive owners of its frames.
    Historical streams with no topology retain an unknown, not a checked zero.
    """
    sizes = {"P01-TOPOLOGY-SOURCE": 128, "P01-TOPOLOGY-ROOT": 64,
             "P01-TOPOLOGY-NODE": 40, "P01-TOPOLOGY-EDGE": 48}
    grouped = {tag: {} for tag in sizes}
    for line in raw.decode("utf-8", errors="replace").splitlines():
        match = re.search(r"\b(P01-TOPOLOGY-\S+)(.*)$", line)
        if not match:
            continue
        tag, rest = match.groups()
        fields = rest.split()
        need(tag in sizes and len(fields) == 2
             and re.fullmatch(r"[1-6]", fields[0]) is not None
             and re.fullmatch(r"[0-9a-f]+", fields[1]) is not None,
             "malformed topology sample/payload")
        size = sizes[tag]
        if tag == "P01-TOPOLOGY-EDGE" and len(fields[1]) == 112:
            size = 56
        grouped[tag].setdefault(int(fields[0]), []).append(words(hexbytes(fields[1], size)))
    if not any(grouped.values()):
        return None
    sources = grouped["P01-TOPOLOGY-SOURCE"]
    samples = {int(sample) for sample in stage_metrics}
    need(set(sources) == samples and all(len(rows) == 1 for rows in sources.values())
         and all(set(rows) <= samples for rows in grouped.values()),
         "missing/duplicate/extra topology stage source")
    known = {tuple(row[:4]) for row in subjects.values()}
    known.update(identity(row) for row in records if row["flags"] & SUBJECT and row["flags"] & OWNER)
    known_by_tid = {owner[0]: owner for owner in known}
    address_mask = 0x000ffffffffff000
    output, complete, previous_epoch = {}, True, None
    for sample in sorted(samples):
        source = sources[sample][0]
        (version, source_sample, epoch, used, online, root_count, node_count, edge_count,
         root_capacity, node_capacity, edge_capacity, status, exclusions, scope,
         required_nodes, required_edges) = source
        roots = grouped["P01-TOPOLOGY-ROOT"].get(sample, [])
        nodes = grouped["P01-TOPOLOGY-NODE"].get(sample, [])
        edges = grouped["P01-TOPOLOGY-EDGE"].get(sample, [])
        need(version in (1, 2) and source_sample == sample and status in (1, 4)
             and (root_capacity, node_capacity, edge_capacity) == (64, 1024, 16384)
             and scope == 7 and exclusions & ~31 == 0
             and (root_count, node_count, edge_count) == (len(roots), len(nodes), len(edges))
             and root_count <= root_capacity and node_count <= node_capacity
             and edge_count <= edge_capacity and required_nodes >= node_count
             and required_edges >= edge_count,
             "topology source schema/counts malformed")
        need(previous_epoch is None or epoch >= previous_epoch, "topology allocator epochs out of order")
        previous_epoch = epoch
        observation = {"source": list(source), "roots": [list(row) for row in roots],
                       "nodes": [list(row) for row in nodes], "edges": [list(row) for row in edges],
                       "cpu_graph_observed_complete": status == 1,
                       "snapshot_storage_fit_proven": status == 1,
                       "physical_semantic_coverage_proven": False,
                       "workload_capacity_fit_proven": False,
                       "shared_tables": [], "leaf_alias_ranges": [],
                       "scope_exclusions": ["non_CPU_roots", "unpublished_retired_roots",
                                            "global_semantic_ownership"]}
        observation["descriptor_accessed_dirty_proven"] = False
        observation["leaf_runs_encoded"] = version == 2
        output[str(sample)] = observation
        if status == 4:
            need(exclusions != 0 and root_count == node_count == edge_count == 0,
                 "invalid topology exports partial graph or lacks exclusion")
            complete = False
            continue
        need(online == 1 and exclusions == 0 and epoch > 0 and root_count > 0
             and required_nodes == node_count and required_edges == edge_count,
             "valid topology source incomplete/excluded")
        need(sidecars[(sample, 1)] == (epoch, epoch, 1, 1)
             and stage_metrics[str(sample)]["used_frames"] == used
             and stage_metrics[str(sample)]["page_size"] == 4096,
             "topology/sample allocator relation mismatch")
        need(physical is not None and physical["source"][3] == 1
             and epoch <= physical["source"][1], "topology physical history source missing/incomplete")
        histories = physical["entries"]
        managed = {row[1] for row in histories}
        active = {row[1]: row for row in histories
                  if (row[11] if row[5] in (6, 7) else row[2]) <= epoch
                  and (row[3] == 0 or epoch < row[3])}
        need(len(active) == used, "topology allocator census cutoff/count mismatch")
        by_pa, by_parent, incoming = {}, {}, {}
        for pa, level, incarnation, classification, refs in nodes:
            need(pa & address_mask == pa and level in (1, 2, 3, 4) and pa not in by_pa
                 and classification in (1, 2), "topology node identity/level malformed")
            if classification == 1:
                need(pa in active and incarnation == active[pa][0] and incarnation > 0,
                     "topology managed node stale/free incarnation")
            else:
                need(incarnation == 0 and pa not in managed, "topology external node invents incarnation")
            by_pa[pa] = (pa, level, incarnation, classification, refs)
            by_parent[pa] = []
            incoming[pa] = 0
        root_owners, root_identities, kernel_roots, owners_by_root = {}, set(), [], {}
        for root in roots:
            pa, tid, task_root, cell, generation, domain, domain_generation, root_class = root
            owner = (tid, task_root, cell, generation)
            need(pa in by_pa and by_pa[pa][1] == 4 and root_class in (1, 2)
                 and root not in root_identities, "topology root missing/duplicate/level conflict")
            root_identities.add(root)
            if not any(owner):
                need(root_class == 1 and domain == domain_generation == 0,
                     "topology kernel root identity malformed")
                kernel_roots.append(pa)
            else:
                need(tid > 0 and task_root > 0 and bool(cell) == bool(generation)
                     and tid not in root_owners
                     and (tid not in known_by_tid or owner == known_by_tid[tid]),
                     "topology root owner identity conflicts")
                root_owners[tid] = owner
                need((root_class == 1 and domain == domain_generation == 0)
                     or (root_class == 2 and domain > 0 and domain_generation > 0),
                     "topology root domain identity malformed")
            owners_by_root.setdefault(pa, []).append(root)
        need(len(kernel_roots) == 1 and all(root[7] != 1 or root[0] == kernel_roots[0] for root in roots),
             "topology explicit kernel/SAS root missing/conflicts")
        domain_roots, root_domains = {}, {}
        for pa, tid, task_root, cell, generation, domain, domain_generation, root_class in roots:
            if tid:
                ancestor = known_by_tid.get(task_root, root_owners.get(task_root))
                need(ancestor is None or ancestor[2:] == (cell, generation),
                     "topology lineage Cell/generation conflicts")
            if root_class == 2:
                key = (domain, domain_generation)
                need(pa != kernel_roots[0]
                     and domain_roots.get(key, pa) == pa and root_domains.get(pa, key) == key,
                     "topology domain/root identity conflicts")
                domain_roots[key], root_domains[pa] = pa, key
        descriptors, leaves = set(), []
        for edge in edges:
            need(len(edge) == (7 if version == 2 else 6), "topology edge version/length mismatch")
            parent, index, descriptor, target, leaf_bytes, classification = edge[:6]
            span = edge[6] if version == 2 else 1
            need(parent in by_pa and 0 <= index < 512 and 1 <= span <= 512 - index
                 and all((parent, index + offset) not in descriptors for offset in range(span))
                 and descriptor & 1 and classification in (1, 2, 3),
                 "topology edge descriptor identity malformed")
            descriptors.update((parent, index + offset) for offset in range(span))
            level = by_pa[parent][1]
            huge = bool(descriptor & 128) and level in (2, 3)
            need(not (level == 4 and descriptor & 128), "topology PML4 illegal huge descriptor")
            expected_bytes = 4096 if level == 1 else (1 << (12 + 9 * (level - 1)) if huge else 0)
            expected_target = descriptor & address_mask
            if huge:
                need(expected_target & (expected_bytes - 1) & ~4096 == 0,
                     "topology huge leaf reserved address bits/alignment")
                expected_target &= ~(expected_bytes - 1)
            need(leaf_bytes == expected_bytes and target == expected_target
                 and target + (leaf_bytes or 4096) <= 1 << 52,
                 "topology descriptor target/leaf geometry mismatch")
            need(span == 1 or leaf_bytes == 4096, "topology run crosses table/huge geometry")
            need(version == 1 or descriptor & 0x60 == 0, "topology run invents hardware accessed/dirty")
            extent = leaf_bytes * span
            need(target + (extent or 4096) <= 1 << 52, "topology run physical extent overflow")
            need(classification != 3 or leaf_bytes == 4096 and target not in active,
                 "topology managed-free table or allocated leaf conflicts")
            by_parent[parent].append(edge)
            if not leaf_bytes:
                need(target in by_pa and by_pa[target][1] == level - 1
                     and classification == by_pa[target][3],
                     "topology missing table target/cycle/level conflict")
                incoming[target] += 1
            else:
                if classification == 1:
                    need(extent // 4096 <= len(active)
                         and all(frame in active for frame in range(target, target + extent, 4096)),
                         "topology managed leaf extent missing/free incarnation")
                elif classification == 2:
                    need(extent // 4096 > len(active)
                         or not all(frame in active for frame in range(target, target + extent, 4096)),
                         "topology external/mixed leaf is entirely allocator-owned")
                if classification == 3:
                    need(all(frame not in active for frame in range(target, target + extent, 4096)),
                         "topology free leaf run overlaps allocator-owned frame")
                leaves.append(edge)
        need(all(node[4] == incoming[pa] for pa, node in by_pa.items()),
             "topology current incoming descriptor count mismatch")
        # Level descent proves acyclicity and bounds path multiplicity without
        # enumerating the potentially exponential virtual paths of a shared DAG.
        paths = {pa: {} for pa in by_pa}
        for pa in owners_by_root:
            paths[pa][pa] = 1
        for pa in sorted(by_pa, key=lambda pa: (-by_pa[pa][1], pa)):
            for edge in by_parent[pa]:
                if edge[4] == 0:
                    for root_pa, count in paths[pa].items():
                        paths[edge[3]][root_pa] = paths[edge[3]].get(root_pa, 0) + count
        need(all(paths.values()), "topology unreachable node/graph incomplete")
        def references(root_pas):
            return [list(row) for root_pa in sorted(root_pas)
                    for row in sorted(owners_by_root[root_pa])]
        for pa in sorted(by_pa):
            refs = references(paths[pa])
            if incoming[pa] > 1 or sum(paths[pa].values()) > 1 or len(refs) > 1:
                observation["shared_tables"].append({
                    "table_pa": pa, "level": by_pa[pa][1], "physical_incarnation": by_pa[pa][2],
                    "incoming_descriptor_count": incoming[pa],
                    "mapping_path_count": sum(paths[pa].values()),
                    "root_pas": sorted(paths[pa]), "root_references": refs,
                    "semantic_owner_proven": False})
        # Sweep physical leaf extents. A shared table's single descriptor can
        # represent multiple CPU mappings; multiple task references to one SAS
        # root alone do not manufacture an additional mapping or semantic owner.
        events = {}
        leaf_by_key = {}
        for edge in leaves:
            key = (edge[0], edge[1])
            leaf_by_key[key] = edge
            events.setdefault(edge[3], []).append((True, key))
            span = edge[6] if version == 2 else 1
            events.setdefault(edge[3] + edge[4] * span, []).append((False, key))
        live, previous = set(), None
        for boundary in sorted(events):
            count = sum(sum(paths[parent].values()) for parent, index in live)
            if previous is not None and boundary > previous and count > 1:
                root_pas = {root_pa for parent, index in live for root_pa in paths[parent]}
                observation["leaf_alias_ranges"].append({
                    "start_pa": previous, "end_pa": boundary, "mapping_alias_count": count,
                    "descriptors": [list(key) for key in sorted(live)],
                    "root_pas": sorted(root_pas), "root_references": references(root_pas),
                    "semantic_owner_proven": False})
            for add, key in events[boundary]:
                if add:
                    live.add(key)
                else:
                    live.remove(key)
            previous = boundary
    return {"stages": output, "cpu_graph_observed_complete": complete,
            "snapshot_storage_fit_proven": complete,
            "physical_semantic_coverage_proven": False, "workload_capacity_fit_proven": False}




def noncpu_root_evidence(raw, physical, sidecars, stage_metrics, ownership):
    """Allocation-lifetime witnesses plus current references; no tree/authority claim."""
    sizes = {"P01-NONCPU-SOURCE": 128, "P01-NONCPU-ROOT": 64, "P01-NONCPU-REFERENCE": 40}
    grouped = {tag: {} for tag in sizes}
    for line in raw.decode("utf-8", errors="replace").splitlines():
        match = re.search(r"\b(P01-NONCPU-\S+)(.*)$", line)
        if match is None:
            continue
        tag, rest = match.groups()
        fields = rest.split()
        need(tag in sizes and len(fields) == 2 and re.fullmatch(r"[1-6]", fields[0]) is not None
             and re.fullmatch(r"[0-9a-f]+", fields[1]) is not None, "malformed non-CPU root payload")
        grouped[tag].setdefault(int(fields[0]), []).append(words(hexbytes(fields[1], sizes[tag])))
    if not any(grouped.values()):
        return None
    sources = grouped["P01-NONCPU-SOURCE"]
    samples = {int(sample) for sample in stage_metrics}
    need(set(sources) == samples and all(len(rows) == 1 for rows in sources.values())
         and all(set(rows) <= samples for rows in grouped.values()),
         "missing/duplicate/extra non-CPU root census")
    output, previous, previous_source = {}, None, None
    for sample in sorted(samples):
        source = sources[sample][0]
        (version, source_sample, epoch, frame_epoch, used, online, root_count, ref_count,
         root_capacity, ref_capacity, status, exclusions, activities, births, scope, hhdm) = source
        roots = grouped["P01-NONCPU-ROOT"].get(sample, [])
        refs = grouped["P01-NONCPU-REFERENCE"].get(sample, [])
        need(version in (1, 2) and source_sample == sample and (root_capacity, ref_capacity) == (128, 256)
             and status in (1, 4) and exclusions & ~63 == 0 and scope == 7 and hhdm & 4095 == 0
             and (root_count, ref_count) == (len(roots), len(refs))
             and root_count <= root_capacity and ref_count <= ref_capacity,
             "non-CPU root source schema/counts malformed")
        if previous_source is not None:
            need(epoch >= previous_source[2] and frame_epoch >= previous_source[3]
                 and births >= previous_source[13] and hhdm == previous_source[15],
                 "non-CPU root source epochs/births regressed")
        previous_source = source
        observation = {"source": list(source), "roots": [list(row) for row in roots],
                       "references": [list(row) for row in refs],
                       "allocation_lifetime_census_complete": status == 1,
                       "published_registry_references_complete": status == 1,
                       "nested_root_count": sum(row[2] <= 2 for row in roots),
                       "iommu_root_allocation_count": sum(row[2] >= 3 for row in roots),
                       "unreferenced_allocated_root_count": sum(row[7] == 0 for row in roots),
                       "physical_tree_topology_proven": False, "physical_semantic_coverage_proven": False,
                       "workload_capacity_fit_proven": False,
                       "scope_exclusions": ["descriptor_trees_and_hardware_publication",
                                            "DMA_VM_authority_and_creation_owner",
                                            "other_physical_allocations_and_workload_capacity"]}
        output[str(sample)] = observation
        if status == 4:
            need(exclusions != 0 and root_count == ref_count == 0,
                 "refused non-CPU root census published partial rows")
            continue
        need(online == 1 and exclusions == activities == 0 and births >= root_count
             and epoch == 2 * births - root_count,
             "valid non-CPU lifetime source incomplete/in-flight")
        need(sidecars[(sample, 1)] == (frame_epoch, frame_epoch, 1, 1)
             and stage_metrics[str(sample)]["used_frames"] == used
             and stage_metrics[str(sample)]["page_size"] == 4096
             and physical is not None and physical["source"][3] == 1
             and frame_epoch <= physical["source"][1], "non-CPU root physical cutoff mismatch")
        active = {row[1]: row for row in physical["entries"]
                  if (row[11] if row[5] in (6, 7) else row[2]) <= frame_epoch
                  and (row[3] == 0 or frame_epoch < row[3])}
        need(len(active) == used, "non-CPU root physical census count mismatch")
        by_token, by_pa = {}, {}
        for row in roots:
            token, pa, kind, incarnation, backing, ptr, heap_incarnation, count = row
            need(0 < token <= epoch and token not in by_token and 0 < pa < 1 << 52
                 and pa & 4095 == 0 and pa not in by_pa and kind in range(1, 6 if version == 1 else 7)
                 and backing == (1 if kind <= 2 else 2) and count <= ref_count,
                 "non-CPU root birth identity/backing malformed")
            need(pa in active and incarnation == active[pa][0] and incarnation > 0,
                 "non-CPU root physical incarnation stale/free")
            if backing == 1:
                need(ptr == heap_incarnation == 0, "frame root invents heap allocation identity")
            else:
                need(ptr == hhdm + pa < 1 << 64 and ptr & 4095 == 0 and heap_incarnation > 0,
                     "heap root pointer/physical relation malformed")
                if sample == 1:
                    baseline = ownership["heap_baseline"]
                    need(baseline is not None, "heap root missing baseline allocation census")
                    heap_rows = {entry[1]: entry for entry in baseline["entries"]}
                    need(ptr in heap_rows and heap_rows[ptr][0] == heap_incarnation
                         and heap_rows[ptr][2:4] == [4096, 4096],
                         "heap root stale incarnation/Layout at baseline")
                else:
                    census = (ownership["heap_stages"] or {}).get(str(sample))
                    need(census is not None, "heap root missing stage allocation census")
                    heap_rows = {entry[0]: entry for entry in census["entries"]}
                    heap_tokens = {entry[0]: entry[1] for entry in census["charged_entries"]}
                    need(ptr in heap_rows and heap_tokens.get(ptr) == heap_incarnation
                         and heap_rows[ptr][1:3] == [4096, 4096],
                         "heap root stale incarnation/Layout at cutoff")
            by_token[token], by_pa[pa] = row, row
        identities, counts = set(), {token: 0 for token in by_token}
        for reference in refs:
            token, role, holder, object_key, parent = reference
            need(token in by_token and role in range(1, 5) and tuple(reference) not in identities,
                 "non-CPU registry reference identity malformed")
            identities.add(tuple(reference))
            kind = by_token[token][2]
            if role == 1:
                need(kind in (1, 2) and object_key > 0 and parent == 0,
                     "VM registry reference uses wrong root family")
                key = (role, holder, object_key)
            elif role == 2:
                need(kind == 5 and holder == object_key == parent == 0,
                     "VT-d hardware root reference malformed")
                key = (role,)
            else:
                need(parent in by_pa and by_pa[parent][2] == 5
                     and any(ref[0] == by_pa[parent][0] and ref[1] == 2 for ref in refs),
                     "VT-d reference missing current hardware root")
                need((role == 3 and kind == (5 if version == 1 else 6) and holder < 256 and object_key == 0)
                     or (role == 4 and kind == 4 and 0 < object_key < 1 << 16),
                     "VT-d registry reference uses wrong root family/key")
                key = (role, holder)
            need(key not in identities, "duplicate non-CPU registry object")
            identities.add(key)
            counts[token] += 1
        need(all(row[7] == counts[token] for token, row in by_token.items()),
             "non-CPU root current-reference count mismatch")
        if previous is not None:
            old_epoch, old_roots, old_frame_epoch = previous
            for token, row in by_token.items():
                if token in old_roots:
                    need(tuple(row[:7]) == tuple(old_roots[token][:7]),
                         "surviving non-CPU root birth/backing changed")
                else:
                    need(token > old_epoch, "new non-CPU root predates previous closed census")
                    need(row[4] != 1 or active[row[1]][2] > old_frame_epoch,
                         "new frame root predates previous physical cutoff")
            if epoch == old_epoch:
                need(set(by_token) == set(old_roots), "non-CPU lifetimes changed without epoch")
            need(all(row[2] <= 2 or token in by_token for token, row in old_roots.items()),
                 "non-freeing IOMMU allocation disappeared")
        previous = epoch, by_token, frame_epoch
    complete = all(stage["allocation_lifetime_census_complete"] for stage in output.values())
    return {"stages": output, "allocation_lifetime_census_complete": complete,
            "published_registry_references_complete": complete,
            "physical_tree_topology_proven": False, "physical_semantic_coverage_proven": False,
            "workload_capacity_fit_proven": False}


def xtree_descriptor(kind, level, raw):
    need(kind in (1, 2, 3, 4) and 1 <= level <= (4 if kind <= 2 else 3), "xtree descriptor family/level")
    if kind == 3:
        need(raw & 1 and raw >> 54 == 0 and not (raw & 4 and not raw & 2), "xtree Sv39 invalid descriptor")
        leaf = bool(raw & 14)
        need(leaf or level > 1, "xtree Sv39 invalid terminal table")
        size = 1 << (12 + 9 * (level - 1)) if leaf else 0
        target = (raw >> 10) << 12
        normalized = raw & ~0xc0
    else:
        need(raw & (1 if kind == 1 else 7 if kind == 2 else 3), "xtree absent descriptor")
        need(kind != 2 or not raw & 2 or raw & 1, "xtree EPT write without read")
        huge = bool(raw & 128) and level > 1
        need(not huge or level < 4, "xtree invalid PML4 huge leaf")
        size = 4096 if level == 1 else 1 << (12 + 9 * (level - 1)) if huge else 0
        address = raw & 0x000ffffffffff000
        if size > 4096:
            need(address & (size - 1) & ~(4096 if kind == 1 else 0) == 0, "xtree huge address/PAT mismatch")
            target = address & ~(size - 1)
        else:
            target = address
        normalized = raw & ~(0x60 if kind == 1 else 0x300)
    need(target < 1 << 52 and (not size or target & (size - 1) == 0) and (size or target),
         "xtree physical geometry")
    return normalized, target, size


def xtree_evidence(raw, physical, ownership):
    sizes = {"P01-XTREE-SOURCE": 128, "P01-XTREE-NODE": 40, "P01-XTREE-EDGE": 56}
    grouped = {tag: {} for tag in sizes}
    for line in raw.decode("utf-8", errors="replace").splitlines():
        match = re.search(r"\b(P01-XTREE-\S+)(.*)$", line)
        if match is None:
            continue
        tag, rest = match.groups()
        fields = rest.split()
        need(tag in sizes and len(fields) == 2 and re.fullmatch(r"[1-6]", fields[0])
             and re.fullmatch(r"[0-9a-f]+", fields[1]), "malformed xtree payload")
        grouped[tag].setdefault(fields[0], []).append(words(hexbytes(fields[1], sizes[tag])))
    if not any(grouped.values()):
        return None
    lifetimes = ownership["noncpu_roots"]
    need(lifetimes is not None and set(grouped["P01-XTREE-SOURCE"]) == set(lifetimes["stages"])
         and all(len(rows) == 1 for rows in grouped["P01-XTREE-SOURCE"].values())
         and all(set(rows) <= set(lifetimes["stages"]) for rows in grouped.values()), "xtree cutoff groups")
    output = {}
    for sample, source_rows in grouped["P01-XTREE-SOURCE"].items():
        source = source_rows[0]
        (version, source_sample, epoch, used, roots_count, node_count, edge_count,
         root_cap, node_cap, edge_cap, status, exclusions, scope, required_nodes, required_edges, root_epoch) = source
        root_stage = lifetimes["stages"][sample]
        nodes = grouped["P01-XTREE-NODE"].get(sample, [])
        edges = grouped["P01-XTREE-EDGE"].get(sample, [])
        supported_roots = [root for root in root_stage["roots"] if version == 1 or root[2] in (1, 2, 3, 4)]
        need(version in (1, 2) and source_sample == int(sample) and (root_cap, node_cap, edge_cap) == (128, 1024, 16384)
             and status in (1, 4) and scope == 15 and exclusions & ~31 == 0
             and (epoch, used, root_epoch, roots_count) == (root_stage["source"][3], root_stage["source"][4],
                                                           root_stage["source"][2], len(supported_roots))
             and (node_count, edge_count) == (len(nodes), len(edges))
             and node_count <= node_cap and edge_count <= edge_cap
             and required_nodes >= node_count and required_edges >= edge_count, "xtree source coherence/counts")
        observation = {"source": list(source), "nodes": [list(row) for row in nodes],
                       "edges": [list(row) for row in edges], "software_tree_observed_complete": status == 1,
                       "snapshot_storage_fit_proven": status == 1, "table_heap_identities": [],
                       "shared_tables": [], "software_alias_ranges": [],
                       "hardware_publication_proven": False, "physical_semantic_coverage_proven": False,
                       "workload_capacity_fit_proven": False,
                       "scope_exclusions": ["VTd_root_context_128bit_structures", "hardware_publication_and_authority",
                                            "global_physical_creation_owners_and_all_path_capacity"]}
        output[sample] = observation
        if status == 4:
            need(exclusions and node_count == edge_count == 0, "xtree refused partial graph")
            continue
        need(exclusions == 0 and root_stage["allocation_lifetime_census_complete"]
             and required_nodes == node_count and required_edges == edge_count,
             "xtree complete preflight missing")
        active = {row[1]: row for row in physical["entries"]
                  if (row[11] if row[5] in (6, 7) else row[2]) <= epoch
                  and (row[3] == 0 or epoch < row[3])}
        by_pa, incoming, children, leaves = {}, {}, {}, []
        for pa, kind, level, incarnation, refs in nodes:
            need(pa not in by_pa and pa in active and incarnation == active[pa][0]
                 and kind in (1, 2, 3, 4) and 1 <= level <= (4 if kind <= 2 else 3),
                 "xtree table identity/incarnation")
            if kind >= 3:
                ptr = root_stage["source"][15] + pa
                if sample == "1":
                    baseline = ownership["heap_baseline"]
                    need(baseline is not None, "xtree missing heap baseline")
                    allocations = {row[1]: row for row in baseline["entries"]}
                    need(ptr in allocations and allocations[ptr][2:4] == [4096, 4096], "xtree heap table Layout/missing")
                    token = allocations[ptr][0]
                else:
                    census = (ownership["heap_stages"] or {}).get(sample)
                    need(census is not None, "xtree missing heap stage")
                    allocations = {row[0]: row for row in census["entries"]}
                    tokens = {row[0]: row[1] for row in census["charged_entries"]}
                    need(ptr in allocations and allocations[ptr][1:3] == [4096, 4096] and ptr in tokens,
                         "xtree heap table Layout/missing")
                    token = tokens[ptr]
                observation["table_heap_identities"].append({"pa": pa, "ptr": ptr, "incarnation": token})
            by_pa[pa], incoming[pa], children[pa] = (pa, kind, level, incarnation, refs), 0, []
        covered = set()
        for edge in edges:
            pa, index, descriptor, target, size, span, kind = edge
            need(pa in by_pa and kind == by_pa[pa][1] and 0 <= index < 512 and 1 <= span <= 512 - index
                 and all((pa, index + offset) not in covered for offset in range(span)), "xtree overlapping/index span")
            covered.update((pa, index + offset) for offset in range(span))
            need(xtree_descriptor(kind, by_pa[pa][2], descriptor) == (descriptor, target, size)
                 and (span == 1 or size == 4096) and target + (size * span or 4096) <= 1 << 52,
                 "xtree descriptor/run geometry")
            if size:
                leaves.append(edge)
            else:
                need(target in by_pa and by_pa[target][1:3] == (kind, by_pa[pa][2] - 1),
                     "xtree child family/level")
                incoming[target] += 1
                children[pa].append(target)
        need(all(row[4] == incoming[pa] for pa, row in by_pa.items()), "xtree incoming count")
        paths = {pa: {} for pa in by_pa}
        for root in supported_roots:
            token, pa, kind = root[:3]
            need(pa in by_pa and by_pa[pa][1:3] == (kind, 4 if kind <= 2 else 3)
                 and by_pa[pa][3] == root[3], "xtree root seed mismatch")
            paths[pa][("noncpu", token)] = 1
        for pa in sorted(by_pa, key=lambda pa: by_pa[pa][2], reverse=True):
            for child in children[pa]:
                for key, count in paths[pa].items():
                    paths[child][key] = paths[child].get(key, 0) + count
        need(all(paths.values()), "xtree unreachable table")
        observation["shared_tables"] = [{"pa": pa, "incoming": incoming[pa],
                                         "mapping_paths": sum(paths[pa].values())}
                                        for pa in by_pa if incoming[pa] > 1 or sum(paths[pa].values()) > 1]
        extents = [(edge[3], edge[3] + edge[4] * edge[5], ("noncpu", edge[0], edge[1]),
                    paths[edge[0]]) for edge in leaves]
        cpu = (ownership.get("cpu_topology") or {}).get("stages", {}).get(sample)
        if cpu is not None and cpu["cpu_graph_observed_complete"]:
            cpu_paths = {row[0]: {} for row in cpu["nodes"]}
            for pa in {row[0] for row in cpu["roots"]}:
                cpu_paths[pa][("cpu", pa)] = 1
            for node in sorted(cpu["nodes"], key=lambda row: row[1], reverse=True):
                for edge in cpu["edges"]:
                    if edge[0] == node[0] and edge[4] == 0:
                        for key, count in cpu_paths[node[0]].items():
                            cpu_paths[edge[3]][key] = cpu_paths[edge[3]].get(key, 0) + count
            for edge in cpu["edges"]:
                if edge[4]:
                    extents.append((edge[3], edge[3] + edge[4] * (edge[6] if len(edge) == 7 else 1),
                                    ("cpu", edge[0], edge[1]), cpu_paths[edge[0]]))
        events, descriptors = {}, {}
        for start, end, key, refs in extents:
            descriptors[key] = refs
            events.setdefault(start, []).append((True, key))
            events.setdefault(end, []).append((False, key))
        live, previous = set(), None
        for boundary in sorted(events):
            if previous is not None and previous < boundary and any(key[0] == "noncpu" for key in live):
                count = sum(sum(descriptors[key].values()) for key in live)
                if count > 1:
                    observation["software_alias_ranges"].append({
                        "start": previous, "end": boundary, "mapping_path_count": count,
                        "planes": sorted({key[0] for key in live}), "hardware_authority_proven": False})
            for begin, key in events[boundary]:
                if begin: live.add(key)
                else: live.remove(key)
            previous = boundary
    complete = all(stage["software_tree_observed_complete"] for stage in output.values())
    return {"stages": output, "software_tree_observed_complete": complete,
            "snapshot_storage_fit_proven": complete, "hardware_publication_proven": False,
            "physical_semantic_coverage_proven": False, "workload_capacity_fit_proven": False}


def vtd_evidence(raw, physical, ownership):
    """Legacy 128-bit software routing structures, not DMA/hardware authority."""
    sizes = {"P01-VTD-SOURCE": 128, "P01-VTD-NODE": 64, "P01-VTD-EDGE": 64}
    grouped = {tag: {} for tag in sizes}
    for line in raw.decode("utf-8", errors="replace").splitlines():
        match = re.search(r"\b(P01-VTD-\S+)(.*)$", line)
        if match is None:
            continue
        tag, rest = match.groups()
        fields = rest.split()
        need(tag in sizes and len(fields) == 2 and re.fullmatch(r"[1-6]", fields[0])
             and re.fullmatch(r"[0-9a-f]+", fields[1]), "malformed VT-d payload")
        grouped[tag].setdefault(fields[0], []).append(words(hexbytes(fields[1], sizes[tag])))
    if not any(grouped.values()):
        return None
    lifetimes = ownership["noncpu_roots"]
    need(lifetimes is not None and set(grouped["P01-VTD-SOURCE"]) == set(lifetimes["stages"])
         and all(len(rows) == 1 for rows in grouped["P01-VTD-SOURCE"].values())
         and all(set(rows) <= set(lifetimes["stages"]) for rows in grouped.values()), "VT-d cutoff groups")
    output = {}
    for sample, source_rows in grouped["P01-VTD-SOURCE"].items():
        source = source_rows[0]
        (version, source_sample, epoch, used, root_epoch, root_count, context_count, edge_count,
         node_cap, edge_cap, status, exclusions, scope, required_edges, census_count, registry_count) = source
        root_stage = lifetimes["stages"][sample]
        nodes = grouped["P01-VTD-NODE"].get(sample, [])
        edges = grouped["P01-VTD-EDGE"].get(sample, [])
        need(version == 1 and source_sample == int(sample) and (node_cap, edge_cap) == (128, 4096)
             and status in (1, 4) and exclusions & ~31 == 0 and scope == 1
             and root_stage["source"][0] == 2
             and (epoch, used, root_epoch, census_count, registry_count) == (
                 root_stage["source"][3], root_stage["source"][4], root_stage["source"][2],
                 root_stage["source"][6], root_stage["source"][7])
             and root_count + context_count == len(nodes) and edge_count == len(edges)
             and len(nodes) <= node_cap and edge_count <= edge_cap and required_edges >= edge_count,
             "VT-d source coherence/counts")
        observation = {
            "source": list(source), "nodes": [list(row) for row in nodes],
            "edges": [list(row) for row in edges],
            "software_structure_observed_complete": status == 1, "snapshot_storage_fit_proven": status == 1,
            "incoming_routing_references": [], "hardware_publication_proven": False,
            "physical_semantic_coverage_proven": False, "workload_capacity_fit_proven": False,
            "scope_exclusions": ["hardware_publication_invalidation_and_DMA_VM_authority",
                                 "physical_creation_owners_and_all_path_workload_capacity"]}
        output[sample] = observation
        if status == 4:
            need(exclusions != 0 and not nodes and not edges, "VT-d refused partial graph")
            continue
        need(exclusions == 0 and root_stage["allocation_lifetime_census_complete"]
             and required_edges == edge_count, "VT-d complete preflight missing")
        roots = {row[1]: row for row in root_stage["roots"]}
        structures = {pa: row for pa, row in roots.items() if row[2] in (5, 6)}
        need((root_count, context_count) == (
                 sum(row[2] == 5 for row in structures.values()), sum(row[2] == 6 for row in structures.values()))
             and len(nodes) == len(structures), "VT-d structure census omitted")
        need(physical is not None and physical["source"][3] == 1 and epoch <= physical["source"][1],
             "VT-d physical history missing")
        active = {row[1]: row for row in physical["entries"]
                  if (row[11] if row[5] in (6, 7) else row[2]) <= epoch
                  and (row[3] == 0 or epoch < row[3])}
        need(len(active) == used, "VT-d physical census count mismatch")
        by_pa = {}
        for row in nodes:
            pa = row[1]
            need(pa not in by_pa and pa in structures and tuple(row) == tuple(structures[pa])
                 and pa in active and row[3] == active[pa][0], "VT-d node lifetime identity mismatch")
            by_pa[pa] = row
        slots, incoming = {}, {}
        for edge in edges:
            parent, index, role, lo, hi, target, birth, did = edge
            need(parent in by_pa and 0 <= index < 256 and (parent, index) not in slots
                 and role == (1 if by_pa[parent][2] == 5 else 2), "VT-d parent/slot/role mismatch")
            need(target in roots and roots[target][0] == birth
                 and roots[target][2] == (6 if role == 1 else 4)
                 and target in active and roots[target][3] == active[target][0],
                 "VT-d target birth/family/incarnation mismatch")
            need(0 < target < 1 << 52 and target & 4095 == 0
                 and lo == target | 1 and lo & ~0x000ffffffffff001 == 0,
                 "VT-d low-word geometry unsupported")
            need((role == 1 and hi == did == 0)
                 or (role == 2 and 0 < did < 1 << 16 and hi == (did << 8) | 1),
                 "VT-d high-word/DID/AW geometry unsupported")
            slots[parent, index] = edge
            incoming[target] = incoming.get(target, 0) + 1
        for token, role, holder, object_key, parent in root_stage["references"]:
            if role == 3:
                edge = slots.get((parent, holder))
                need(edge is not None and edge[2] == 1 and edge[6] == token,
                     "VT-d registry context/root slot mismatch")
        observation["incoming_routing_references"] = [
            {"pa": pa, "birth_token": roots[pa][0], "incoming": count}
            for pa, count in sorted(incoming.items())]
    complete = all(stage["software_structure_observed_complete"] for stage in output.values())
    return {"stages": output, "software_structure_observed_complete": complete,
            "snapshot_storage_fit_proven": complete, "hardware_publication_proven": False,
            "physical_semantic_coverage_proven": False, "workload_capacity_fit_proven": False}


def validate_stream(raw, metadata):
    validate_export_order(raw)
    records, subjects, counters, sidecars, obligations, finals, late, manifests, workloads, stages, peaks, storage = decode_stream(raw)
    ownership = ownership_evidence(raw, subjects, records)
    physical = physical_evidence(raw, records)
    need(ownership["heap_origin"][1] == 0, "heap ownership ledger invalid")
    need(len(finals) == 1 and len(manifests) == 1, "missing/duplicate final header or manifest")
    need(bool(late), "missing independent late status")
    need(manifests[0] == metadata["manifest_hex"], "signed image manifest mismatch")
    need(set(peaks) == {0, 1} and len(storage) == 1, "missing/duplicate peak or overhead sidecars")
    expected_buffer = 352 if ownership["heap_charged"] is not None or ownership["heap_stages"] is not None else 320
    need(storage[0][0] == 56896 and storage[0][1] >= 56896 and storage[0][2] > 0
         and storage[0][3] == expected_buffer, "private storage accounting mismatch")
    reserved, ready, active, invalid = finals[0]
    need(0 < reserved <= 256 and reserved == ready == len(records) and active == 0, "overflow/unready/faulted writer")
    need(invalid == 0 and not any(late), "sticky final/late invalid status")
    need([row["sequence"] for row in records] == list(range(1, len(records) + 1)), "missing/duplicate record sequence")
    for event in (1, 2, 11, 12):
        need(sum(row["event"] == event for row in records) == 1, "missing/duplicate capture control")
    need(records[0]["event"] == 1 and records[-1]["event"] == 12, "ambiguous arm/final boundary")
    need(records[-1]["values"][0] == invalid, "record/header final validity mismatch")
    need(all(row["cpu"] < metadata["cpus"] for row in records), "logicalCPU out of configured range")
    intents, results, published = {}, {}, {}
    for row in records:
        attempt = row["attempt"]
        if row["event"] == 3:
            need(attempt not in intents, "duplicate launch intent")
            intents[attempt] = row
        elif row["event"] == 4:
            need(attempt in intents and attempt not in results, "unmatched/duplicate attempt result")
            intent = intents[attempt]
            need(all(row[key] == intent[key] for key in ("route", "source", "parent_tid", "parent_cell", "parent_generation")), "attempt provenance changed")
            results[attempt] = row
            if row["result"] == 1:
                owner = identity(row)
                need(owner not in published and all(other[0] != owner[0] for other in published), "duplicate subject/incarnation")
                published[owner] = row
    need(set(intents) == set(results), "unfinished logical launch attempt")
    need(len(published) == len(subjects) <= 9, "subject sidecar inventory mismatch")
    subject_index = {}
    for index, entry in subjects.items():
        owner = entry[:4]
        need(owner in published and owner not in subject_index, "unknown/duplicate subject sidecar")
        row = published[owner]
        need(entry[4:] == (row["parent_tid"], row["parent_cell"], row["parent_generation"], row["attempt"]), "sidecar publication mismatch")
        subject_index[owner] = index
    init_digest, driver_digest, probe_digest = [metadata["manifest_hex"][i:i + 64] for i in (0, 64, 128)]
    drivers = [row for row in published.values() if row["digest"] == driver_digest and row["route"] in (1, 2)]
    # A missing driver is NOT_RUN, never a count-zero control.
    need(len(drivers) == 1, "NOT_RUN: driver publication missing/ambiguous")
    driver = drivers[0]
    arm = records[0]
    need(arm["digest"] == init_digest and (driver["parent_tid"], driver["parent_cell"], driver["parent_generation"]) == (arm["tid"], arm["cell"], arm["generation"]), "bootstrap owner/digest binding mismatch")
    need(all(r["parent_tid"] in (arm["tid"], driver["tid"]) for r in intents.values()), "unselected launch parent")
    need(all(r["key"] == subject_index[owner] for owner, r in published.items()), "publication subject index mismatch")
    for event in (2, 11, 12):
        control = next(r for r in records if r["event"] == event)
        need(identity(control) == identity(driver), "control not bound driver incarnation")
    children = [row for row in published.values() if row is not driver]
    for row in children:
        need(row["digest"] == probe_digest and row["route"] == 3 and (row["parent_tid"], row["parent_cell"], row["parent_generation"]) == (driver["tid"], driver["cell"], driver["generation"]), "child digest/route/exact parent mismatch")
    need(sum(row["parent_tid"] == driver["parent_tid"] for row in intents.values()) <= 2, "driver fallback attempt bound")
    need(sum(row["parent_tid"] == driver["tid"] for row in intents.values()) <= 8, "child attempt bound")
    reason = []
    if physical is not None:
        reason.append("physical inventory semantic/topology coverage unproven")
        if physical["source"][3] != 1:
            reason.append("physical inventory source incomplete")
    if not ownership["heap_origin"][2] or not ownership["heap_origin"][3]:
        reason.append("heap ownership capacity/coverage unproven")
    if ownership["heap_origin"][9] or ownership["heap_origin"][10]:
        reason.append("unknown heap origin or unresolved preparation owner")
    charged = ownership["heap_charged"]
    if charged is not None and charged["final_partitions"]["unowned"]:
        reason.append("unclassified semantic owner charged bytes remain")
    if ownership["retained_union_bytes"] is None:
        reason.append("retained range source incomplete")
    elif ownership["retained_union_bytes"] != 0:
        reason.append("final retained physical range union nonzero")
    for owner, row in published.items():
        for event in (5, 6, 7):
            rows = [r for r in records if r["event"] == event and identity(r) == owner]
            need(len(rows) == 1, "missing/duplicate lifecycle outcome")
            if rows[0]["result"] != 1:
                reason.append(EVENTS[event] + " not success")
        for component in range(1, 9):
            for event in (8, 9):
                rows = [r for r in records if r["event"] == event and identity(r) == owner and r["group"] == component]
                need(len(rows) == 1, "missing/duplicate component event")
                if event == 9:
                    key = (subject_index[owner], component)
                    need(key in counters, "missing component fixed counter")
                    counter = counters[key]
                    need(tuple(rows[0]["values"]) == counter[:4], "component record/counter mismatch")
                    need(rows[0]["key"] == key[0], "component subject index mismatch")
                    if counter[0] or counter[2] or counter[3] or counter[4] or counter[5] or rows[0]["result"] != 1:
                        reason.append("pending/retained/unknown/live component")
    need(len(counters) == len(subjects) * 8, "extra counter sidecar")
    for row in records:
        if row["event"] in (5, 6, 7, 8, 9):
            need(identity(row) in published, "unpublished lifecycle subject")
    requested = 1 if metadata["count"] == "denied" else int(metadata["count"])
    need(len(records) <= 51 + 21 * requested, "conditional finite event bound exceeded")
    need(len(workloads) == 1, "missing/duplicate workload outcome")
    workload = workloads[0]
    need(workload.get("requested") == requested and workload.get("admitted") == len(children), "workload/trace requested-count mismatch")
    pre = next(r for r in records if r["event"] == 2)
    need(pre["values"] == [requested, 0, 0, 0], "pre/requested count mismatch")
    denied = metadata["count"] == "denied"
    failures = [r for r in results.values() if r["parent_tid"] == driver["tid"] and r["result"] != 1]
    denied_public_result_mapping = None
    if denied:
        need(len(children) == 0 and len(failures) == 1 and failures[0]["result"] == 2 and workload.get("status") == "DENIED_CONTROL", "intentional refusal absent/wrong result")
        refusal = failures[0]
        need((refusal["route"], refusal["source"], refusal["group"], refusal["values"][0]) == (3, 2, 1, 4)
             and refusal["source_error"] == "PermissionDenied"
             and (refusal["parent_tid"], refusal["parent_cell"], refusal["parent_generation"]) == (driver["tid"], driver["cell"], driver["generation"]),
             "denied control lacks exact Pinned kernel PermissionDenied refusal")
        # task::syscall::encode_syscall_result preserves PermissionDenied as
        # -1; ostd::decode_spawn_result maps -1 to Unknown, not PermissionDenied.
        # Unknown is accepted only with the exact source refusal above.
        need(workload.get("public_result") == "Err(Unknown)", "denied control public/source result mismatch")
        denied_public_result_mapping = {
            "attempt": refusal["attempt"], "route": refusal["route"], "source": refusal["source"],
            "error_namespace": refusal["group"], "raw_source_error": refusal["values"][0],
            "source_error": refusal["source_error"], "encoded_return": -1,
            "public_result": workload["public_result"], "target_path_recorded": False,
        }
        reason.append("intentional denied control; not a normal resource workload")
    else:
        need(len(children) == requested and not failures and workload.get("status") == "LIFECYCLE_COMPLETE" and workload.get("exited") == requested, "count refusal/handshake failure")
    groups = {}
    for row in records:
        if row["event"] == 10:
            key = (row["key"], row["group"])
            need(key not in groups, "duplicate snapshot group")
            groups[key] = row
    samples = [1, 3] if denied else list(range(1, 7))
    need(set(groups) == {(sample, group) for sample in samples for group in range(4)}, "missing/extra snapshot group")
    need(set(sidecars) == {(sample, metric) for sample in samples for metric in range(16)}, "missing/extra snapshot source sidecar")
    need(set(stages) == set(samples), "missing/extra stage timing")
    ordered_ticks = [stages[sample] for sample in samples]
    need(ordered_ticks == sorted(ordered_ticks) and ordered_ticks[-1] - ordered_ticks[0] <= 500, "guest observation bound/timing order")
    if not denied and (stages[5] != stages[4] + 1 or stages[6] != stages[5] + 1):
        reason.append("settled samples not consecutive scheduler ticks")
    stage_metrics = {}
    for sample in samples:
        values = []
        for metric in range(16):
            row = groups[(sample, metric // 4)]
            present = bool(row["detail"] & (1 << (metric % 4)))
            before, after, status, source_owner = sidecars[(sample, metric)]
            need(present == (status == 1), "metric presence/source status mismatch")
            if not present:
                reason.append("absent/incoherent/unknown metric: " + METRICS[metric])
            values.append(row["values"][metric % 4] if present else None)
        stage_metrics[str(sample)] = dict(zip(METRICS, values))
    baseline = ownership["heap_baseline"]
    if baseline is not None:
        source = baseline["source"]
        need(all(sidecars[(1, metric)] == (source[6], source[6], 1, 2) for metric in (4, 5))
             and stage_metrics["1"]["actual_heap_used_bytes"] == source[7]
             and stage_metrics["1"]["actual_heap_free_bytes"] == source[8],
             "heap baseline/sample1 allocator relation mismatch")
    heap_stages = ownership["heap_stages"]
    if heap_stages is not None:
        need(set(heap_stages) == set(stage_metrics) - {"1"}, "missing/extra heap stage census")
        for sample, census in heap_stages.items():
            source = census["source"]
            need(all(sidecars[(int(sample), metric)] == (source[4], source[4], 1, 2) for metric in (4, 5))
                 and stage_metrics[sample]["actual_heap_used_bytes"] == source[5]
                 and stage_metrics[sample]["actual_heap_free_bytes"] == source[6],
                 "heap stage/sample allocator relation mismatch")
    if physical is not None:
        population = {}
        for sample in samples:
            before, after, status, owner = sidecars[(sample, 1)]
            if status != 1 or before != after or owner != 1:
                population[str(sample)] = None
                continue
            frames = {entry[1] for entry in physical["entries"]
                      if (entry[11] if entry[5] in (6, 7) else entry[2]) <= before
                      and (entry[3] == 0 or before < entry[3])}
            population[str(sample)] = {"allocator_epoch": before, "inventory_frames": len(frames),
                "allocator_used_frames": stage_metrics[str(sample)]["used_frames"],
                "unmatched_allocator_used_frames": stage_metrics[str(sample)]["used_frames"] - len(frames)}
        physical["sample_populations"] = population
    ownership["cpu_topology"] = cpu_topology_evidence(
        raw, subjects, records, physical, sidecars, stage_metrics)
    if ownership["cpu_topology"] is not None and not ownership["cpu_topology"]["cpu_graph_observed_complete"]:
        reason.append("CPU topology graph incomplete/excluded")
    ownership["noncpu_roots"] = noncpu_root_evidence(raw, physical, sidecars, stage_metrics, ownership)
    if ownership["noncpu_roots"] is not None and not ownership["noncpu_roots"]["allocation_lifetime_census_complete"]:
        reason.append("non-CPU root lifetime census incomplete/excluded")
    ownership["noncpu_topology"] = xtree_evidence(raw, physical, ownership)
    if ownership["noncpu_topology"] is not None and not ownership["noncpu_topology"]["software_tree_observed_complete"]:
        reason.append("non-CPU software descriptor graph incomplete/excluded")
    ownership["vtd_structures"] = vtd_evidence(raw, physical, ownership)
    if ownership["vtd_structures"] is not None and not ownership["vtd_structures"]["software_structure_observed_complete"]:
        reason.append("VT-d software routing structures incomplete/excluded")
    for sample in samples:
        values = stage_metrics[str(sample)]
        if all(values[name] is not None for name in ("total_frames", "used_frames", "free_frames")):
            need(values["used_frames"] + values["free_frames"] == values["total_frames"], "frame snapshot reconciliation")
        page_size = values["page_size"]
        if page_size is not None:
            need(page_size > 0 and page_size & (page_size - 1) == 0, "invalid page-size units")
    if not denied:
        if stage_metrics["1"]["admitted_live_subjects"] != 1 or stage_metrics["2"]["admitted_live_subjects"] != requested + 1:
            reason.append("pre/live subject inventory mismatch")
    if not denied:
        settled = [stage_metrics[str(s)] for s in (4, 5, 6)]
        if not all(sample == settled[0] for sample in settled): reason.append("three settled samples unstable")
        for sample in settled:
            for metric in ("outstanding_cleanup_obligations", "retained_physical_frame_union", "unknown_retained_obligations", "capture_invalid_flags", "active_operations", "reserved_completions"):
                if sample[metric] != 0: reason.append("settled nonzero/absent " + metric)
            if sample["admitted_live_subjects"] != 1: reason.append("settled driver inventory")
        for metric in ("used_frames", "actual_heap_used_bytes", "execution_context_quota_charge", "allocated_stack_bytes", "segment_bytes"):
            if settled[-1][metric] != stage_metrics["1"][metric]: reason.append("unreclaimed raw residual: " + metric)
    tokens = set()
    for obligation in obligations.values():
        cell, generation, tid, token, quantity = obligation["words"]
        need(token not in tokens and any(owner[0] == tid and owner[2:] == (cell, generation) for owner in published), "duplicate/unknown obligation owner")
        tokens.add(token)
        if obligation["state"] != 1 or obligation["terminal"] == 0:
            reason.append("pending/retained/unknown obligation")
        need(obligation["terminal"] == 0 or obligation["terminal"] >= obligation["created"], "obligation terminal precedes creation")
        need(obligation["created"] <= reserved and obligation["terminal"] <= reserved, "obligation sequence out of capture")
    if records[-1]["result"] != 1: reason.append("CaptureFinal not accounting success")
    deltas = {}
    for sample in samples[1:]:
        deltas[str(sample)] = {
            name: None if stage_metrics["1"][name] is None or stage_metrics[str(sample)][name] is None
            else stage_metrics[str(sample)][name] - stage_metrics["1"][name]
            for name in METRICS
        }
    return {
        "structurally_complete": True, "fully_settled": not reason,
        "qualification_blockers": sorted(set(reason)), "records": records,
        "subjects": {str(k): list(v) for k, v in subjects.items()},
        "component_counters": {f"{k[0]}:{k[1]}": list(v) for k, v in counters.items()},
        "snapshot_sidecars": {f"{k[0]}:{k[1]}": list(v) for k, v in sidecars.items()},
        "source_owner_mapping": SOURCE_OWNERS, "obligations": obligations,
        "stage_metrics": stage_metrics, "raw_stage_minus_pre": deltas,
        "stage_ticks": stages, "control_drift_subtracted": False,
        "requested": requested, "admitted": len(children), "denied_control": denied,
        "denied_control_accepted": denied, "denied_public_result_mapping": denied_public_result_mapping,
        "workload_outcome": workload,
        "ownership_evidence": ownership,
        "physical_inventory": physical,
        "global_independent_peaks": peaks,
        "private_trace_storage": {"serialized_payload_bytes": storage[0][0], "core_static_bytes": storage[0][1], "resource_static_bytes": storage[0][2], "exporter_buffer_bytes": storage[0][3], "linked_image_overhead_not_inferred": True},
        "final_status": {"reserved": reserved, "ready": ready, "active": active, "flags": invalid},
        "late_status_flags": late, "throughput_claim": False,
    }


def parse_capture(directory):
    directory = Path(directory)
    evidence = {"structurally_complete": False, "fully_settled": False, "denied_control_accepted": False, "qualification_blockers": [], "throughput_claim": False}
    try:
        metadata = json.loads((directory / "capture.json").read_text())
        termination = json.loads((directory / "termination.json").read_text())
        raw = (directory / "serial.raw.log").read_bytes()
        need(str(uuid.UUID(metadata["capture_uuid"])) == directory.name, "captureUUID/path identity mismatch")
        need(metadata["count"] in ("0", "1", "4", "8", "denied") and metadata["cpus"] in (1, 2), "capture configuration")
        need(metadata["host_bound_seconds"] == 150 and termination["retained_through_vm_termination"] is True, "missing termination validity window")
        need(termination["elapsed_seconds"] <= 157 and termination["termination"] in ("natural", "host-bound-terminate", "host-bound-kill"), "termination bound exceeded")
        need(termination["termination"] != "natural" or termination["returncode"] == 0, "VM failed before bound")
        need(hashlib.sha256(raw).hexdigest() == termination["raw_sha256"], "raw provenance mismatch")
        need(hashlib.sha256((directory / "capture.json").read_bytes()).hexdigest() == termination["capture_sha256"], "command provenance mismatch")
        build_path = directory / "build.json"
        need(hashlib.sha256(build_path.read_bytes()).hexdigest() == metadata["build_sha256"], "build provenance mismatch")
        build = json.loads(build_path.read_text())
        need(build["scenarios"][metadata["count"]] == metadata["artifact_sha256"], "artifact provenance mismatch")
        need("".join(build["signed"][name] for name in ("app-init", "bench", "bench-probe")) == metadata["manifest_hex"], "manifest provenance mismatch")
        for name in ("autoload.toml", "services.toml"):
            expected = metadata["artifact_sha256"][metadata["count"] + "/override/" + name]
            need(hashlib.sha256((directory / name).read_bytes()).hexdigest() == expected, "config provenance mismatch")
        need('args = ["parked-footprint:' + metadata["count"] + '"]' in (directory / "autoload.toml").read_text(), "configured argv mismatch")
        command = metadata["command"]
        need(command.count("-cdrom") == command.count("-smp") == 1 and command[command.index("-smp") + 1] == str(metadata["cpus"]), "VM command/config mismatch")
        need(command[0] == metadata["qemu_binary_path"] and Path(command[0]).is_absolute(), "emulator operand/provenance mismatch")
        need(bool(re.fullmatch(r"[0-9a-f]{64}", metadata["qemu_binary_sha256"])), "emulator digest absent")
        need(command[command.index("-cdrom") + 1] == metadata["image_path"] and Path(metadata["image_path"]).is_absolute(), "CD-ROM operand/provenance mismatch")
        need(metadata["image_sha256"] == metadata["artifact_sha256"][metadata["count"] + "/image.iso"], "CD-ROM digest/build mismatch")
        need(not re.search(rb"(?:panicked at|KERNEL PANIC|triple fault|P01-FAULT)", raw, re.IGNORECASE), "kernel/observer fault")
        evidence = validate_stream(raw, metadata)
        evidence["capture_uuid"] = metadata["capture_uuid"]
        evidence["termination"] = termination
    except (InvalidTrace, ValueError, KeyError, IndexError, OSError, TypeError, struct.error) as error:
        evidence["qualification_blockers"].append(str(error))
        if "raw" in locals():
            try:
                decoded = decode_stream(raw)
                evidence["partial_records"] = decoded[0]
                evidence["partial_final_status"] = decoded[5]
                evidence["partial_late_status"] = decoded[6]
                evidence["partial_workload_outcomes"] = decoded[8]
                fault = ownership_evidence(raw, decoded[1], decoded[0])["heap_first_fault"]
                if fault is not None:
                    evidence["partial_heap_first_fault"] = fault
            except (InvalidTrace, ValueError, KeyError, IndexError, TypeError, struct.error):
                pass
    return evidence


# G1 is deliberately a separate consumer. No G1 result relaxes legacy P01 gates.
G1_EXPERIMENT = {
    "lane": "Intel-x86_64-q35-TCG-development", "period_ns": 100_000_000,
    "entitlement_ns": 20_000_000, "burst_cap_ns": 40_000_000,
    "service_gap_limit_ns": 20_000_000, "switch_delay_limit_ns": 20_000_000,
    "exhaustion_to_departure_limit_ns": 20_000_000,
    "periods": 16, "max_events": 256, "workers": 2,
    "actions": {"admit": 2, "release": 2, "worker_exit": 2, "join": 2,
                "explicit_yield_max": 8, "delay": 1, "delay_source": "hpet-period16", "root_exit": 1},
    "max_worker_chunks": 1_000_000, "initial_bank_ns": 0, "initial_debt_ns": 0,
    "pacing_clock_is_charge_source": False, "hardware_D_W_qualified": False,
    "max_capture_periods": 18, "max_workload_stages": 64,
    "service_summary_kind": 17, "max_schedule_events": 206,
}
G1_HEADER_FIELDS = (
    "state error boot_session tick_period_fs epoch_ns last_ns period_ns period_index "
    "next_boundary_ns control_charge_ns next_switch_serial event_count admitted_hart_mask "
    "service_gap_limit_ns switch_delay_limit_ns exhaustion_to_departure_limit_ns "
    "last_raw_counter timebase_kind").split()
G1_GROUP_FIELDS = (
    "cell_id generation root_tid state error policy_revision entitlement_ns burst_cap_ns "
    "burst_bank_ns debt_ns period_charge_ns lifetime_charge_ns last_period_charge_ns "
    "last_period_allowance_ns max_period_overshoot_ns pending_revision pending_entitlement_ns "
    "pending_burst_cap_ns pending_apply_period terminal_serial charged_period_count "
    "refusal_count throttle_count birth_ns").split()
G1_HART_FIELDS = (
    "active_cell active_generation active_root_tid active_tid pending_cell pending_generation "
    "pending_root_tid pending_tid pending_serial last_completed_serial pending_since_ns last_service_ns").split()
G1_EVENT_FIELDS = "sequence now_ns kind cell_id generation hart value0 value1".split()
G1_MAX = (1 << 64) - 1
G1_FAILURES = dict(enumerate((
    "BadConfig", "ClockUnavailable", "ClockRegression", "ArithmeticOverflow",
    "ServiceGapExceeded", "SwitchDelayExceeded", "OvershootExceeded", "UnknownOwner",
    "StaleGeneration", "PolicyConflict", "CounterExhausted", "WitnessFull",
    "SwitchMismatch", "Busy", "RetirementPending", "HistoryFull", "ExperimentBoundExceeded"), 2))


def validate_g1_review(review, build_sha256, inputs_sha256):
    """Accept a parent-issued source-review receipt, never manufacture approval."""
    need(review.get("actual_diff_reviewed") is True and review.get("finite_event_capacity_approved") is True
         and review.get("max_events") == 256 and review.get("build_sha256") == build_sha256
         and review.get("inputs_sha256") == inputs_sha256
         and review.get("experiment") == G1_EXPERIMENT, "G1 exact-build pre-capture review required")
    certificate = review.get("finite_schedule", {})
    need(set(certificate) == {"g1-overlap", "g1-yield", "g1-retention"}, "G1 missing finite schedule certificate")
    for row in certificate.values():
        need(isinstance(row, dict) and bool(row.get("source_reasoning"))
             and re.fullmatch("[0-9a-f]{64}", row.get("review_sha256", "")), "G1 missing source-bound schedule review")
        fields = ("scheduler_transition_bound", "service_event_bound", "fixed_event_bound", "total_event_bound")
        need(all(type(row.get(field)) is int and row[field] >= 0 for field in fields), "G1 malformed finite event bound")
        need(row["total_event_bound"] == 2 * row["scheduler_transition_bound"] + row["service_event_bound"] + row["fixed_event_bound"]
             and 0 < row["total_event_bound"] <= 206, "G1 finite event capacity exceeded")


def g1_kv(text):
    fields = {}
    for token in text.split()[1:]:
        key, separator, value = token.partition("=")
        need(separator and key not in fields and re.fullmatch(r"\d+", value), "G1 malformed/duplicate numeric field")
        number = int(value)
        need(number <= G1_MAX, "G1 numeric overflow")
        fields[key] = number
    return fields


def decode_g1_stream(raw):
    """Fixed private bytes only; unknown tags/duplicate final rows fail closed."""
    result = {"events": [], "groups": {}, "retired": {}, "harts": {}, "actions": [],
              "workloads": [], "clock_boot": [], "clock": [], "cutoff": [], "headers": [], "manifests": []}
    for line in raw.decode("utf-8", errors="strict").splitlines():
        position = line.find("G1-")
        if position < 0:
            continue
        text = line[position:].strip()
        parts = text.split()
        tag = parts[0]
        if tag in ("G1-HEADER", "G1-EVENT", "G1-GROUP", "G1-RETIRED", "G1-HART"):
            names = {"G1-HEADER": G1_HEADER_FIELDS, "G1-EVENT": G1_EVENT_FIELDS,
                     "G1-GROUP": G1_GROUP_FIELDS, "G1-RETIRED": G1_GROUP_FIELDS,
                     "G1-HART": G1_HART_FIELDS}[tag]
            need(len(parts) == 3 and re.fullmatch(r"index=\d+", parts[1]) and parts[2].startswith("hex="), "G1 fixed indexed row shape")
            index = int(parts[1][6:])
            row = dict(zip(names, words(hexbytes(parts[2][4:], len(names) * 8))))
            if tag in ("G1-HEADER", "G1-EVENT"):
                target = result["headers" if tag == "G1-HEADER" else "events"]
                need(index == len(target) and (tag != "G1-HEADER" or index == 0), "G1 row index/order")
                target.append(row)
            else:
                target = result[{"G1-GROUP": "groups", "G1-RETIRED": "retired", "G1-HART": "harts"}[tag]]
                need(index < (2 if tag == "G1-HART" else 64) and index not in target, "G1 duplicate/out-of-bound row")
                target[index] = row
        elif tag == "G1-MANIFEST":
            need(len(parts) == 2 and parts[1].startswith("hex="), "G1 manifest shape")
            result["manifests"].append(hexbytes(parts[1][4:], 96).hex())
        elif tag in ("G1-CLOCK-BOOT", "G1-CLOCK", "G1-CUTOFF"):
            result[{"G1-CLOCK-BOOT": "clock_boot", "G1-CLOCK": "clock", "G1-CUTOFF": "cutoff"}[tag]].append(g1_kv(text))
        elif tag in ("G1-WORKLOAD", "G1-ACTION"):
            row = {}
            for token in parts[1:]:
                key, separator, value = token.partition("=")
                need(separator and key not in row, "G1 malformed action")
                row[key] = value
            result["workloads" if tag == "G1-WORKLOAD" else "actions"].append(row)
        else:
            raise InvalidTrace("G1 unknown record tag")
    return result


def validate_g1_stream(raw, metadata):
    data = decode_g1_stream(raw)
    need(metadata.get("g1") is True and metadata["g1_experiment"] == G1_EXPERIMENT, "G1 experiment/config mismatch")
    need(metadata["count"] in ("g1-overlap", "g1-yield", "g1-retention") and metadata["cpus"] in (1, 2), "G1 mode/hart selection")
    need(data["manifests"] == [metadata["manifest_hex"]], "G1 measured scope manifest mismatch")
    need(all(len(data[key]) == 1 for key in ("headers", "clock_boot", "clock", "cutoff")), "G1 missing/duplicate cutoff/clock/header")
    header, boot, clock, cutoff = (data[key][0] for key in ("headers", "clock_boot", "clock", "cutoff"))
    events = data["events"]
    need(set(cutoff) == {"events", "retired", "pending", "state", "error", "payload"}, "G1 cutoff fields")
    need(cutoff["payload"] == 41304 and cutoff["pending"] == cutoff["error"] == header["error"] == 0,
         "G1 failure/unknown/pending cutoff")
    need(cutoff["state"] == header["state"] == 3 and cutoff["events"] == header["event_count"] == len(events) <= 256,
         "G1 cutoff count/state mismatch or overflow")
    need(cutoff["retired"] == len(data["retired"]) == 1 and not data["groups"], "G1 complete retained generation required")
    need(set(boot) == {"raw", "period_fs", "base", "caps", "config", "ticks_per_ms", "initial", "lvt"}
         and set(clock) == {"raw", "period_fs", "base", "capabilities", "configuration"}, "G1 clock sidecar fields")
    need(boot["base"] == clock["base"] != 0
         and 0 < boot["period_fs"] == clock["period_fs"] == header["tick_period_fs"] <= 100_000_000
         and boot["caps"] == clock["capabilities"] and boot["caps"] >> 32 == boot["period_fs"]
         and boot["config"] == clock["configuration"] and boot["caps"] & (1 << 13) and boot["config"] & 1
         and 0 < boot["ticks_per_ms"] <= ((1 << 32) - 1) // 10
         and 10_000 <= boot["initial"] == boot["ticks_per_ms"] * 10 <= (1 << 32) - 1
         and not boot["lvt"] & (1 << 16) and (boot["lvt"] >> 17) & 3 == 1,
         "G1 checked HPET/calibration identity")
    need(boot["raw"] <= header["last_raw_counter"] <= clock["raw"]
         and header["last_ns"] == header["last_raw_counter"] * header["tick_period_fs"] // 1_000_000,
         "G1 raw clock regression/conversion")
    need(header["timebase_kind"] == 1 and header["boot_session"] > 0
         and header["admitted_hart_mask"] == (1 << metadata["cpus"]) - 1, "G1 admitted clock/hart identity")
    for field in ("period_ns", "service_gap_limit_ns", "switch_delay_limit_ns", "exhaustion_to_departure_limit_ns"):
        need(header[field] == G1_EXPERIMENT[field], "G1 immutable policy limit mismatch")
    period, epoch, harts = header["period_ns"], header["epoch_ns"], metadata["cpus"]
    need(16 <= header["period_index"] < 18 and header["next_boundary_ns"] == epoch + (header["period_index"] + 1) * period,
         "G1 sixteen-period finite schedule not observed")
    need(epoch + header["period_index"] * period <= header["last_ns"] < header["next_boundary_ns"] <= G1_MAX,
         "G1 cutoff period/timestamp overflow mismatch")
    need(set(data["harts"]) == {0, 1}, "G1 hart storage enumeration")
    for index, row in data["harts"].items():
        need(all(row[key] == 0 for key in ("active_cell", "active_generation", "active_root_tid",
             "pending_cell", "pending_generation", "pending_root_tid", "pending_tid", "pending_serial", "pending_since_ns")),
             "G1 cutoff active/pending ownership")
        if index >= harts:
            need(all(value == 0 for value in row.values()), "G1 offline hart fabricated occupancy")
    need(len(events) >= 2 and events[0]["kind"] == 1 and events[0]["now_ns"] == epoch
         and events[1]["kind"] == 16 and events[1]["now_ns"] == epoch and events[-1]["kind"] == 12,
         "G1 immutable clock enable/cutoff boundaries missing")
    owners = [None] * harts
    active_tids = [None] * harts
    serials = set()
    pending = {}
    intervals = []
    summaries, required_summaries = {}, set()
    stage_count = 0
    groups, tids, periods, transfers = {}, {}, {}, []
    last, control, overlap, index = epoch, 0, 0, 0
    terminal_seen = set()

    def checked(value):
        need(0 <= value <= G1_MAX, "G1 accounting arithmetic overflow")
        return value

    def integrate(now):
        nonlocal last, control, overlap, index
        need(last <= now <= header["last_ns"], "G1 timestamp regression/outside cutoff")
        if any(owner is not None for owner in owners) or any(item[0] is not None for item in pending.values()):
            need(now - last <= period, "G1 active global service gap exceeds period")
        for cpu in range(harts):
            if cpu in pending:
                need(now - pending[cpu][3] <= header["switch_delay_limit_ns"], "G1 pending switch delay")
        while last < now:
            end = min(now, epoch + (index + 1) * period)
            delta = end - last
            active = [owner for owner in owners if owner is not None]
            for cpu, owner in enumerate(owners):
                if owner is not None and delta:
                    intervals.append({"hart": cpu, "cell": owner[0], "generation": owner[1],
                                      "tid": active_tids[cpu], "start_ns": last, "end_ns": end,
                                      "charge_ns": delta, "period": index})
            for key in groups:
                row = groups[key]
                count = active.count(key)
                row["charge"] = checked(row["charge"] + count * delta)
                row["lifetime"] = checked(row["lifetime"] + count * delta)
                row["overshoot"] = max(row["overshoot"], max(0, row["charge"] - row["allowance"]))
                need(row["charge"] <= row["allowance"] + harts * header["exhaustion_to_departure_limit_ns"],
                     "G1 overshoot bound exceeded")
            control = checked(control + (harts - len(active)) * delta)
            if len(active) == 2 and active[0] == active[1]:
                overlap = checked(overlap + delta)
            last = end
            if last == epoch + (index + 1) * period:
                for key, row in groups.items():
                    if key in terminal_seen:
                        continue
                    charge, allowance = row["charge"], row["allowance"]
                    periods[(key, index)] = (charge, allowance)
                    net = row["entitlement"] + row["bank"] - row["debt"] - charge
                    # First period uses birth proration, never a full-period gift.
                    if row["birth_period"] == index:
                        net = row["initial_entitlement"] + row["bank"] - row["debt"] - charge
                    row["bank"], row["debt"] = min(row["cap"], max(0, net)), max(0, -net)
                    row["last_charge"], row["last_allowance"] = charge, allowance
                    row["overshoot"] = max(row["overshoot"], max(0, charge - allowance))
                    row["period_count"] += int(charge != 0)
                    row["charge"] = 0
                    row["allowance"] = max(0, row["entitlement"] + row["bank"] - row["debt"])
                index += 1

    matched_periods = set()
    throttles = []
    for sequence, event in enumerate(events):
        need(event["sequence"] == sequence and event["kind"] in set(range(1, 18)) - {5, 13}, "G1 missing/unknown/obsolete event sequence")
        need(event["kind"] != 11, "G1 producer failure retained")
        if event["kind"] == 17:
            summary_key = (event["hart"], event["generation"])
            need(event["cell_id"] == 0 and event["hart"] < harts and event["generation"] < 18
                 and summary_key not in summaries
                 and epoch + event["generation"] * period <= event["now_ns"] < epoch + (event["generation"] + 1) * period
                 and event["now_ns"] <= header["last_ns"]
                 and event["value0"] <= header["service_gap_limit_ns"], "G1 per-hart service summary gap/identity/period")
            summaries[summary_key] = event
            continue
        integrate(event["now_ns"])
        kind, key, hart, v0, v1 = event["kind"], (event["cell_id"], event["generation"]), event["hart"], event["value0"], event["value1"]
        if kind == 1:
            need(sequence == 0 and key == (0, 0) and hart == G1_MAX and v0 == clock["base"] and v1 == clock["capabilities"], "G1 enable identity")
        elif kind == 16:
            need(sequence == 1 and key == (0, 0) and hart == G1_MAX
                 and v0 == boot["config"] == clock["configuration"]
                 and boot["raw"] <= v1 <= header["last_raw_counter"]
                 and v1 * header["tick_period_fs"] // 1_000_000 == epoch,
                 "G1 immutable initial configuration/raw clock mismatch")
        elif kind == 2:
            need(0 < key[0] < 64 and key[1] > 0 and key not in groups and v0 > 0 and v1 == 20_000_000, "G1 bind identity/policy")
            need(v0 == header["boot_session"], "G1 private single-root experiment binding mismatch")
            need(not any(old[0] == key[0] and old not in terminal_seen for old in groups), "G1 stale-generation slot reuse")
            entitlement = v1 * (epoch + (index + 1) * period - last) // period
            groups[key] = {"root": v0, "birth": last, "birth_period": index, "initial_entitlement": entitlement,
                           "entitlement": v1, "cap": 40_000_000, "bank": 0, "debt": 0, "charge": 0,
                           "lifetime": 0, "allowance": entitlement, "last_charge": 0, "last_allowance": 0,
                           "period_count": 0, "overshoot": 0}
        elif kind in (3, 4):
            need(hart < harts, "G1 event on unadmitted hart")
            owner = None if key == (0, 0) else key
            need(owner is None or owner in groups and owner not in terminal_seen, "G1 unknown/stale owner")
            if owner is not None or owners[hart] is not None:
                required_summaries.add((hart, (last - epoch) // period))
            if kind == 3:
                need(hart not in pending and v1 > 0 and v1 not in serials and (owner is None or v0 > 0), "G1 stage duplicate/identity")
                stage_count += 1
                need(stage_count <= 64, "G1 private session transition action bound exceeded")
                serials.add(v1)
                if owner is not None:
                    need(groups[owner]["charge"] < groups[owner]["allowance"], "G1 selection after exhaustion")
                    need(v0 not in tids or tids[v0] == owner, "G1 TID generation mismatch")
                    tids[v0] = owner
                need(owner is None or all(other == hart or active_tids[other] != v0 for other in range(harts)), "G1 duplicate executing TID")
                pending[hart] = (owner, v0, v1, last)
            elif kind == 4:
                need(hart in pending and owners[hart] == owner, "G1 unstaged/wrong outgoing transfer")
                incoming, tid, serial, staged = pending.pop(hart)
                need(serial == v1 and last - staged <= header["switch_delay_limit_ns"], "G1 switch delay/serial mismatch")
                need(owner is None or active_tids[hart] == v0, "G1 outgoing TID mismatch")
                need(incoming is None or all(other == hart or active_tids[other] != tid for other in range(harts)),
                     "G1 incoming TID already executing on another hart")
                owners[hart] = incoming
                active_tids[hart] = tid
                transfers.append({"hart": hart, "now_ns": last, "outgoing": owner, "incoming": incoming, "tid": tid, "serial": serial})
        elif kind == 6:
            need(key in groups and hart == G1_MAX and last > epoch and (last - epoch) % period == 0, "G1 period boundary shape")
            entry = (key, (last - epoch) // period - 1)
            need(entry in periods and entry not in matched_periods and periods[entry] == (v0, v1), "G1 exact charge/allowance recurrence mismatch")
            matched_periods.add(entry)
        elif kind == 7:
            need(key in groups and hart == G1_MAX and groups[key]["charge"] >= groups[key]["allowance"]
                 and v0 == groups[key]["allowance"] and 0 <= v1 < (1 << harts), "G1 throttle charge/active mask")
            throttles.append((key, last, v1))
            actual_mask = sum(1 << cpu for cpu in range(harts) if owners[cpu] == key or cpu in pending and pending[cpu][0] == key)
            need(v1 == actual_mask, "G1 throttle occupied/pending mask mismatch")
        elif kind == 8:
            raise InvalidTrace("G1 unexpected policy action outside fixed workload")
        elif kind in (9, 10):
            need(key in groups and v0 == groups[key]["root"], "G1 retirement identity")
            if kind == 10:
                need(key not in owners and all(item[0] != key for item in pending.values()) and key not in terminal_seen,
                     "G1 retirement before transfer/duplicate")
                need(v1 in data["retired"], "G1 terminal history missing")
                terminal_seen.add(key)
        elif kind == 12:
            need(sequence == len(events) - 1 and not pending and all(owner is None for owner in owners), "G1 unsettled cutoff")
        elif kind == 14:
            need(key in groups and hart == G1_MAX and v0 > 0 and last > epoch
                 and (last - epoch) % period == 0 and key not in owners
                 and all(item[0] != key for item in pending.values()), "G1 idle skip occupied/boundary")
            end_period = (last - epoch) // period
            need(v0 <= end_period <= 18, "G1 idle skip count overflow/outside bounded schedule")
            skipped = [(key, skipped_index) for skipped_index in range(end_period - v0, end_period)]
            need(all(entry in periods and entry not in matched_periods and periods[entry][0] == 0 for entry in skipped)
                 and periods[skipped[-1]][1] == v1, "G1 idle skip recurrence mismatch")
            matched_periods.update(skipped)
        elif kind == 15:
            need(hart in pending and pending[hart][:3] == (None if key == (0, 0) else key, v0, v1),
                 "G1 abort unstaged/identity mismatch")
            pending.pop(hart)
    integrate(header["last_ns"])
    need(index == header["period_index"] and control == header["control_charge_ns"], "G1 global/control interval reconciliation")
    need(matched_periods == set(periods), "G1 missing period recurrence witness")
    need(len(groups) == len(terminal_seen) == 1 and len(tids) == 3, "G1 root and two same-generation workers required")
    required_summaries.update((entry["hart"], entry["period"]) for entry in intervals)
    need(required_summaries <= set(summaries), "G1 missing occupied-hart service summary")
    need(len(summaries) <= harts * 18 and len(events) <= 206, "G1 source-bound finite event schedule exceeded")
    for row in data["retired"].values():
        key = (row["cell_id"], row["generation"])
        need(key in terminal_seen and row["error"] == 0 and row["root_tid"] == groups[key]["root"], "G1 terminal stale/unknown identity")
        expected = groups[key]
        for field, replay in (("birth_ns", "birth"), ("entitlement_ns", "entitlement"), ("burst_cap_ns", "cap"),
                              ("burst_bank_ns", "bank"), ("debt_ns", "debt"), ("period_charge_ns", "charge"),
                              ("lifetime_charge_ns", "lifetime"), ("last_period_charge_ns", "last_charge"),
                              ("last_period_allowance_ns", "last_allowance"), ("charged_period_count", "period_count"),
                              ("max_period_overshoot_ns", "overshoot")):
            need(row[field] == expected[replay], "G1 terminal accounting mismatch: " + field)
        terminal_transfers = [item["serial"] for item in transfers if item["outgoing"] == key]
        need(terminal_transfers and row["terminal_serial"] == terminal_transfers[-1]
             and row["terminal_serial"] < header["next_switch_serial"]
             and row["throttle_count"] == sum(item[0] == key for item in throttles), "G1 retained terminal serial/throttle count")
        need(row["state"] == 5 and row["policy_revision"] == 1 and all(row[field] == 0 for field in
             ("pending_revision", "pending_entitlement_ns", "pending_burst_cap_ns", "pending_apply_period")), "G1 terminal policy/state")
    for key, exhausted, mask in throttles:
        for hart in range(harts):
            if mask & (1 << hart):
                departure = next((item["now_ns"] for item in transfers if item["hart"] == hart and item["now_ns"] >= exhausted
                                  and item["outgoing"] == key and item["incoming"] != key), None)
                need(departure is not None and departure - exhausted <= header["exhaustion_to_departure_limit_ns"], "G1 exhaustion-to-final-departure bound")
    mode = metadata["count"][3:]
    need(not any("invalid" in row for row in data["workloads"]), "G1 workload refusal")
    need(len(data["workloads"]) == 2 and data["workloads"][0] ==
         {"mode": mode, "workers": "2", "explicit_yields": "8" if mode == "yield" else "0",
          "delay_source": "hpet-period16", "max_chunks": "1000000", "status": "start"}
         and data["workloads"][1]["status"] == "terminal" and data["workloads"][1]["mode"] == mode, "G1 finite workload markers")
    for worker in (0, 1):
        rows = [row for row in data["actions"] if row.get("worker") == str(worker)]
        need([row["action"] for row in rows] == ["admit", "release", "start"] + (["yield"] * 4 if mode == "yield" else []) + ["exit", "joined"],
             "G1 finite worker action ordering")
        tid = int(rows[0]["tid"])
        need(tid == int(rows[-1]["tid"]) and tids.get(tid) in terminal_seen, "G1 worker admission/producer generation mismatch")
        need(int(rows[1]["tid"]) == tid and int(rows[2]["sender_tid"]) == next(iter(groups.values()))["root"],
             "G1 release sender/root/worker binding mismatch")
        measured_owner = next(iter(groups))
        need((int(rows[2]["sender_cell"]), int(rows[2]["sender_generation"])) == measured_owner,
             "G1 release attested sender generation mismatch")
        need(tid != next(iter(groups.values()))["root"], "G1 worker substituted with root")
        need([int(row["ms"]) for row in rows] == sorted(int(row["ms"]) for row in rows), "G1 pacing marker regression")
        if mode == "yield":
            need([row["ordinal"] for row in rows if row["action"] == "yield"] == ["0", "1", "2", "3"], "G1 finite yield ordinals")
    need([row["action"] for row in data["actions"] if row.get("worker") == "2"] == ["delay-start", "delay-end"], "G1 delay markers")
    need(len(data["actions"]) == (20 if mode == "yield" else 12), "G1 unknown/extra finite action")
    worker_tids = {int(row["tid"]) for row in data["actions"] if row["action"] == "admit"}
    need(len(worker_tids) == 2, "G1 duplicate worker TID")
    worker_overlap = 0
    for left in intervals:
        if left["tid"] not in worker_tids:
            continue
        for right in intervals:
            if right["hart"] > left["hart"] and right["tid"] in worker_tids and right["tid"] != left["tid"]:
                worker_overlap += max(0, min(left["end_ns"], right["end_ns"]) - max(left["start_ns"], right["start_ns"]))
    if metadata["cpus"] == 2 and mode == "overlap":
        need(worker_overlap > 0, "G1 no observed same-generation distinct-hart worker overlap")
    if mode == "retention":
        need(throttles, "G1 no observed throttle; PASS text cannot substitute")
    return {"structurally_complete": True, "g1_accounting_valid": True, "fully_settled": False,
            "qualification_blockers": [], "header": header, "events": events, "retired": data["retired"],
            "clock": clock, "clock_boot": boot, "same_generation_overlap_ns": overlap,
            "same_generation_worker_overlap_ns": worker_overlap, "per_hart_intervals": intervals,
            "per_hart_transfers": transfers, "throttle_observations": len(throttles),
            "per_hart_period_service_summaries": [row for _, row in sorted(summaries.items())],
            "production_qualified": False, "hardware_D_W_qualified": False, "throughput_claim": False}


def parse_g1_capture(directory):
    directory = Path(directory)
    evidence = {"structurally_complete": False, "g1_accounting_valid": False, "fully_settled": False,
                "qualification_blockers": [], "production_qualified": False, "hardware_D_W_qualified": False}
    try:
        metadata = json.loads((directory / "capture.json").read_text(), object_pairs_hook=unique_object)
        termination = json.loads((directory / "termination.json").read_text(), object_pairs_hook=unique_object)
        evidence["termination"] = termination
        code = termination["returncode"]
        if type(code) is int and (code - 65) % 2 == 0 and (code - 65) // 2 in G1_FAILURES:
            error = (code - 65) // 2
            evidence["private_exit_failure"] = {"code": error, "name": G1_FAILURES[error],
                                                "source": "private-debug-exit protocol; not hardware attestation"}
        build_bytes = (directory / "build.json").read_bytes()
        build = json.loads(build_bytes, object_pairs_hook=unique_object)
        review_bytes = (directory / "g1-review.json").read_bytes()
        review = json.loads(review_bytes, object_pairs_hook=unique_object)
        inputs_bytes = (directory / "inputs.json").read_bytes()
        inputs = json.loads(inputs_bytes, object_pairs_hook=unique_object)
        raw = (directory / "serial.raw.log").read_bytes()
        need(str(uuid.UUID(metadata["capture_uuid"])) == directory.name, "G1 capture UUID/path")
        need(metadata["host_bound_seconds"] == 150 and termination["retained_through_vm_termination"] is True
             and termination["termination"] == "natural" and termination["returncode"] == 33
             and 0 <= termination["elapsed_seconds"] <= 150, "G1 actual private boot termination required")
        need(hashlib.sha256(raw).hexdigest() == termination["raw_sha256"]
             and hashlib.sha256((directory / "capture.json").read_bytes()).hexdigest() == termination["capture_sha256"]
             and hashlib.sha256(build_bytes).hexdigest() == metadata["build_sha256"]
             and hashlib.sha256(review_bytes).hexdigest() == metadata["review_sha256"], "G1 capture provenance")
        validate_g1_review(review, metadata["build_sha256"], build["inputs_sha256"])
        need(hashlib.sha256(inputs_bytes).hexdigest() == build["inputs_sha256"]
             and inputs["scripts/x86/p01_capture.py"] == metadata["runner_sha256"]
             and inputs["scripts/x86/p01_trace.py"] == metadata["parser_sha256"], "G1 producer source provenance")
        need(build["g1"] is True and build["g1_experiment"] == metadata["g1_experiment"] == G1_EXPERIMENT
             and build["fault"] is None and build["scenarios"][metadata["count"]] == metadata["artifact_sha256"], "G1 source-bound build/config")
        need("".join(build["signed"][name] for name in ("app-init", "bench", "bench-probe")) == metadata["manifest_hex"], "G1 signed scope digests")
        for name in ("autoload.toml", "services.toml"):
            need(hashlib.sha256((directory / name).read_bytes()).hexdigest() ==
                 metadata["artifact_sha256"][metadata["count"] + "/override/" + name], "G1 packaged config provenance")
        need('args = ["cpu-accounting:' + metadata["count"][3:] + '"]' in (directory / "autoload.toml").read_text(), "G1 workload argv")
        command = metadata["command"]
        for flag, operand in (("-machine", "q35"), ("-accel", "tcg"), ("-cpu", "qemu64,vendor=GenuineIntel,+pdpe1gb"),
                              ("-smp", str(metadata["cpus"])), ("-m", "256M"), ("-cdrom", metadata["image_path"])):
            need(command.count(flag) == 1 and command[command.index(flag) + 1] == operand, "G1 VM configuration")
        need(command[0] == metadata["qemu_binary_path"] and Path(command[0]).is_absolute()
             and re.fullmatch("[0-9a-f]{64}", metadata["qemu_binary_sha256"])
             and Path(metadata["image_path"]).is_absolute()
             and metadata["image_sha256"] == metadata["artifact_sha256"][metadata["count"] + "/image.iso"]
             and "isa-debug-exit,iobase=0xf4,iosize=0x04" in command, "G1 image/emulator termination provenance")
        need(not re.search(rb"(?:panicked at|KERNEL PANIC|triple fault|P01-FAULT)", raw, re.IGNORECASE), "G1 kernel/observer fault")
        evidence = validate_g1_stream(raw, metadata)
        need(len(evidence["events"]) <= review["finite_schedule"][metadata["count"]]["total_event_bound"],
             "G1 observed schedule exceeds reviewed source bound")
        evidence.update(capture_uuid=metadata["capture_uuid"], termination=termination)
    except (InvalidTrace, ValueError, KeyError, IndexError, OSError, TypeError, struct.error) as error:
        evidence["structurally_complete"] = evidence["g1_accounting_valid"] = evidence["fully_settled"] = False
        evidence["qualification_blockers"].append(str(error))
        if "raw" in locals():
            try:
                evidence["partial_g1"] = decode_g1_stream(raw)
            except (InvalidTrace, ValueError, KeyError, IndexError, TypeError, struct.error):
                pass
    return evidence
