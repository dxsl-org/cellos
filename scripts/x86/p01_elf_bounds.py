#!/usr/bin/env python3
"""Deterministic, offline P01 ELF partition bounds, not a fit/coverage verdict.

Usage: python3 scripts/x86/p01_elf_bounds.py --build BUILD/build.json --out bounds.json
Reads signed artifacts from successful cellos-sign command arguments in the build
manifest. Never executes a manifest command, rebuilds, or overwrites output.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

PAGE_SIZE = 4096
U64_MAX = (1 << 64) - 1
USER_VADDR_MAX = 0x40_0000_0000
CELL_VA_START = 0x1_0000_0000
CELL_VA_STRIDE = 0x200_0000
CELL_VA_SLOTS = 512
ELF_HEADER = struct.Struct("<16sHHIQQQIHHHHHH")
PROGRAM_HEADER = struct.Struct("<IIQQQQQQ")
SELECTED_COUNTS = (0, 1, 4, 8)

# Reviewed rules of the retained build, not hashes of a possibly edited tree.
# A different implementation requires a superseding rule review, not blind reuse.
RULE_SOURCE_SHA256 = {
    "kernel/src/loader/elf.rs": "a3370d6ac9fd21d56e36b09c06777ec47ecbbc5d8c527e6f30a53b646aa7b758",
    "kernel/src/loader/va_alloc.rs": "585fbb66677c68a6ac735436efa738d8cbf3dac66a2d9bc3189ca2ed7da77298",
    "kernel/src/task.rs": "8bbfdf152be2cf29b3f985f79a6b5536c8cf1dfaf63ed7ff220f2559657181c7",
    "kernel/src/task/elf_prepare.rs": "8f2fbebb65be232e6b3977787b40c228bfb2b1ac5b9eaf765c845d58286ffa1e",
    "kernel/src/task/stack.rs": "e9c1d152d6532fdc3bb737186633a32562fa5eb5cf26c4d10d35f145cd1f03cb",
    "kernel/src/task/tcb.rs": "67a2cf7b97614fee1262d88bcb136c777c04f703f826a459426579e3836c5d13",
}


class InvalidCertificate(ValueError):
    """Malformed input or unsupported source-policy binding."""


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def checked_add(left, right, label):
    if not 0 <= left <= U64_MAX or not 0 <= right <= U64_MAX or left > U64_MAX - right:
        raise InvalidCertificate(f"{label}: unsigned 64-bit addition overflow")
    return left + right


def checked_mul(left, right, label):
    if not 0 <= left <= U64_MAX or not 0 <= right <= U64_MAX or (right and left > U64_MAX // right):
        raise InvalidCertificate(f"{label}: unsigned 64-bit multiplication overflow")
    return left * right


def page_interval(vaddr, mem_size, load_base=0):
    """Mirror load_segments, including its unaligned zero-size interval rule."""
    start = checked_add(vaddr, load_base, "segment VA + base")
    end = checked_add(start, mem_size, "segment VA + memory size")
    if start >= USER_VADDR_MAX or end > USER_VADDR_MAX:
        raise InvalidCertificate("segment outside loader user VA window")
    rounded_end = checked_add(end, PAGE_SIZE - 1, "segment end rounding")
    return start & ~(PAGE_SIZE - 1), rounded_end & ~(PAGE_SIZE - 1)


def union_intervals(intervals):
    """Merge half-open page intervals without allocating one item per page."""
    merged = []
    for start, end in sorted(intervals):
        if start >= end:
            continue
        if merged and start <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], end)
        else:
            merged.append([start, end])
    return merged


def interval_pages(intervals):
    total = 0
    for start, end in intervals:
        total = checked_add(total, (end - start) // PAGE_SIZE, "page count sum")
    return total


def parse_elf(data):
    """Check ELF64 LE x86-64 headers and calculate successful-loader bounds.

    This is not signature verification, relocation validation, or proof of
    collision-free/runtime admission. p_align is reported, not used: the loader
    rounds actual virtual intervals regardless of that ELF field.
    """
    if len(data) < ELF_HEADER.size:
        raise InvalidCertificate("truncated ELF64 header")
    (ident, elf_type, machine, version, entry, phoff, _shoff, _flags,
     ehsize, phentsize, phnum, _shentsize, _shnum, _shstrndx) = ELF_HEADER.unpack_from(data)
    if ident[:4] != b"\x7fELF" or ident[4:7] != b"\x02\x01\x01":
        raise InvalidCertificate("expected ELF64 little-endian version 1")
    if elf_type not in (2, 3) or machine != 62 or version != 1:
        raise InvalidCertificate("expected ET_EXEC/ET_DYN, EM_X86_64, ELF version 1")
    if ehsize != ELF_HEADER.size or phentsize != PROGRAM_HEADER.size:
        raise InvalidCertificate("unexpected ELF64 header/program-header size")
    if not phnum or phnum == 0xFFFF:
        raise InvalidCertificate("missing or unsupported extended program-header count")
    if phoff < ehsize or phoff % 8:
        raise InvalidCertificate("program-header table overlaps ELF header or is unaligned")
    phend = checked_add(phoff, checked_mul(phnum, phentsize, "program-header table size"),
                        "program-header table end")
    if phend > len(data):
        raise InvalidCertificate("program-header table outside ELF bytes")

    # Every normal PIE slot has a page-aligned base. Counts are invariant under
    # translation; validate the highest possible base as well as the lowest.
    bases = (CELL_VA_START, CELL_VA_START + (CELL_VA_SLOTS - 1) * CELL_VA_STRIDE) if elf_type == 3 else (0,)
    segments = []
    reserve_pages = 0
    for index in range(phnum):
        kind, flags, offset, vaddr, _paddr, filesz, memsz, alignment = PROGRAM_HEADER.unpack_from(
            data, phoff + index * phentsize)
        if kind != 1:  # Only PT_LOAD reaches the actual page mapping loop.
            continue
        if filesz > memsz:
            raise InvalidCertificate(f"PT_LOAD {index}: file size exceeds memory size")
        file_end = checked_add(offset, filesz, f"PT_LOAD {index} file range")
        if file_end > len(data):
            raise InvalidCertificate(f"PT_LOAD {index}: file range outside ELF bytes")
        if flags & 3 == 3:
            raise InvalidCertificate(f"PT_LOAD {index}: loader rejects W+X")
        relative = page_interval(vaddr, memsz)
        for base in bases:
            page_interval(vaddr, memsz, base)
        pages = (relative[1] - relative[0]) // PAGE_SIZE
        # div_ceil in the source reservation is NOT the interval page count.
        requested = memsz // PAGE_SIZE + int(memsz % PAGE_SIZE != 0)
        reserve_pages = checked_add(reserve_pages, requested, "loader reservation sum")
        segments.append({
            "program_header_index": index, "flags": flags, "file_offset": offset,
            "file_size": filesz, "virtual_address": vaddr, "memory_size": memsz,
            "p_align": alignment, "page_interval_relative_to_base": list(relative),
            "interval_pages": pages, "reservation_expression_pages": requested,
        })
    if not segments:
        raise InvalidCertificate("ELF has no PT_LOAD program headers")
    intervals = [item["page_interval_relative_to_base"] for item in segments]
    merged = union_intervals(intervals)
    writable = union_intervals([item["page_interval_relative_to_base"] for item in segments if item["flags"] & 2])
    executable = union_intervals([item["page_interval_relative_to_base"] for item in segments if item["flags"] & 1])
    # wx::enforce also refuses W+X created by merging different segment flags.
    for wstart, wend in writable:
        for xstart, xend in executable:
            if max(wstart, xstart) < min(wend, xend):
                raise InvalidCertificate("loader W^X enforcement rejects merged writable/executable page")
    union_pages = interval_pages(merged)
    summed_pages = interval_pages(intervals)
    return {
        "elf_type": "ET_DYN" if elf_type == 3 else "ET_EXEC", "machine": "EM_X86_64",
        "entry": entry, "program_header_offset": phoff, "program_header_count": phnum,
        "load_base": {"kind": "normal-page-aligned-PIE-slot" if elf_type == 3 else "fixed-zero",
                      "minimum": bases[0], "maximum": bases[-1],
                      "actual_runtime_base_observed": False},
        "load_segments": segments, "union_page_intervals_relative_to_base": merged,
        "interval_sum_pages": summed_pages, "union_pages": union_pages,
        "overlap_pages_in_interval_sum": summed_pages - union_pages,
        "exclusive_segment_frames": union_pages, "mapped_page_entries": union_pages,
        "writable_page_entries": interval_pages(writable),
        "writable_array_reservation_entries": union_pages,
        "loader_reservation_expression_pages": reserve_pages,
        "reservation_request_covers_union_count": reserve_pages >= union_pages,
        "reservation_request_shortfall_entries": max(0, union_pages - reserve_pages),
        "no_reallocation_proven": False,
        "reservation_ceiling": "Request/count comparison only; actual Vec capacity, compiled library/allocator behavior and realloc overlap are not certified.",
    }


def load_json(data, label):
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise InvalidCertificate(f"{label}: duplicate JSON key {key!r}")
            result[key] = value
        return result
    try:
        value = json.loads(data, object_pairs_hook=unique_object)
    except (ValueError, UnicodeDecodeError) as error:
        raise InvalidCertificate(f"{label}: {error}") from error
    if not isinstance(value, dict):
        raise InvalidCertificate(f"{label}: expected JSON object")
    return value


def signed_paths(manifest):
    """Resolve the actual signing command operands, never guessed target paths."""
    commands = manifest.get("commands")
    if not isinstance(commands, list):
        raise InvalidCertificate("manifest has no command list")
    found = {name: set() for name in ("bench", "bench-probe")}
    for command in commands:
        if not isinstance(command, dict):
            raise InvalidCertificate("malformed manifest command")
        argv = command.get("argv")
        if not isinstance(argv, list) or not all(isinstance(item, str) for item in argv):
            raise InvalidCertificate("malformed manifest command argv")
        if not any(Path(item).name == "cellos-sign" for item in argv) or "--sign" not in argv:
            continue
        if command.get("returncode") != 0:
            raise InvalidCertificate("manifest signing command did not succeed")
        for item in argv[argv.index("--sign") + 1:]:
            name = Path(item).name
            if name not in found:
                continue
            path = Path(item)
            if not path.is_absolute():
                cwd = command.get("cwd")
                if not isinstance(cwd, str) or not Path(cwd).is_absolute():
                    raise InvalidCertificate("relative signed path lacks absolute command cwd")
                path = Path(cwd) / path
            found[name].add(str(path.resolve()))
    if any(len(paths) != 1 for paths in found.values()):
        raise InvalidCertificate("missing or ambiguous signed bench/probe paths")
    return {name: Path(next(iter(paths))) for name, paths in found.items()}


def selected_bounds(artifacts):
    driver = artifacts["bench"]
    probe = artifacts["bench-probe"]
    rows = []
    for count in SELECTED_COUNTS:
        roots = count + 1
        segments = checked_add(driver["union_pages"], checked_mul(count, probe["union_pages"], "child segment total"), "selected segment total")
        stacks = 36 * roots
        rows.append({
            "children": count, "driver_tasks": 1, "total_root_tasks": roots,
            "exclusive_segment_frames": segments, "mapped_page_entries": segments,
            "segment_interval_sum_pages": driver["interval_sum_pages"] + count * probe["interval_sum_pages"],
            "exclusive_stack_frames": stacks, "child_stack_frames": 36 * count,
            "exclusive_stack_allocated_bytes": stacks * PAGE_SIZE,
            "exclusive_stack_usable_bytes": 32 * roots * PAGE_SIZE,
            "exclusive_stack_guard_bytes": 4 * roots * PAGE_SIZE,
            "exclusive_segment_and_stack_frames": checked_add(segments, stacks, "selected exclusive frame total"),
            "exclusive_preparation_retained_allocations_upper_bound": 6 * roots,
            "child_exclusive_preparation_retained_allocations_upper_bound": 6 * count,
            "shared_sas_new_table_frames_coarse_upper_bound": 3 * (segments + 32 * roots),
        })
    return rows


def certificate(build_path):
    build_path = Path(build_path).resolve()
    manifest_bytes = build_path.read_bytes()
    manifest = load_json(manifest_bytes, "build manifest")
    if manifest.get("version") != 1 or manifest.get("scope") != "private-P01-no-architecture-approval":
        raise InvalidCertificate("unsupported build manifest version/scope")
    inputs_path = build_path.parent / "inputs.json"
    inputs_bytes = inputs_path.read_bytes()
    if sha256(inputs_bytes) != manifest.get("inputs_sha256"):
        raise InvalidCertificate("build source inventory SHA-256 mismatch")
    inputs = load_json(inputs_bytes, "source inventory")
    for name, expected in RULE_SOURCE_SHA256.items():
        if inputs.get(name) != expected:
            raise InvalidCertificate(f"unsupported source-policy SHA-256: {name}")
    signed = manifest.get("signed")
    if not isinstance(signed, dict):
        raise InvalidCertificate("manifest has no signed artifact hashes")
    artifacts = {}
    for name, path in signed_paths(manifest).items():
        data = path.read_bytes()
        digest = sha256(data)
        if digest != signed.get(name):
            raise InvalidCertificate(f"signed artifact SHA-256 mismatch: {name}")
        artifacts[name] = {"path": str(path), "size_bytes": len(data), "sha256": digest,
                           **parse_elf(data)}
    return {
        "version": 1, "scope": "private-P01-artifact-bound-exclusive-preparation-partitions",
        "build_manifest": {"path": str(build_path), "sha256": sha256(manifest_bytes)},
        "source_inventory": {"path": str(inputs_path), "sha256": sha256(inputs_bytes),
                             "reviewed_rule_sources_sha256": RULE_SOURCE_SHA256},
        "page_size_bytes": PAGE_SIZE, "artifacts": artifacts,
        "selected_bounds": selected_bounds(artifacts),
        "rules": {
            "interval": "[floor((p_vaddr+base)/4096)*4096, ceil((p_vaddr+base+p_memsz)/4096)*4096)",
            "interval_sum": "Sum before overlap removal; conservative, not exclusive physical frames.",
            "union": "Unique mapped pages; same-task segment overlap reuses frames and merges flags.",
            "zero_size": "Unaligned zero-size PT_LOAD still touches one page in this loader; aligned zero-size touches none.",
            "loader_reservation": "sum(ceil(p_memsz/4096)) omits the start offset and is not a general bound on mapped pages.",
            "stacks_per_task": {"count": 2, "usable_frames_each": 16, "guard_frames_each": 2,
                                "allocated_frames": 36, "descriptors": "inline in Task, no separate heap allocation"},
            "retained_exclusive_allocations_per_task": {"upper_bound": 6,
                "objects": ["Task Box", "Task name", "cwd", "argv", "mapped-page array", "writable-page array"],
                "empty_buffers": "May reduce allocation count; selected allowed-driver Vec is empty.",
                "bytes": "Actual Layout/capacity required; length, count and allocator-used bytes are not interchangeable."},
            "shared_sas_tables": "At most three new non-root tables per new leaf path. Coarse 3*(segment pages + 32 usable stack pages/task) assumes every mapping is a new path; reused/pre-existing root/tables are excluded and are never exclusively child-owned.",
        },
        "assumptions": [
            "Conditional successful preparation/publication: one bench driver and N bench-probe root tasks, no retries/replacements, empty allowed-driver buffers, selected bench/bench-probe stack names, normal 512-slot PIE policy.",
            "Per-task page counts are translation-invariant at page-aligned PIE bases; no actual runtime base, collision-free VA assignment or concurrent admission is inferred.",
            "Signed byte hashes are compared with the retained manifest; no cryptographic signature verification or fresh build execution is performed.",
            "Reviewed source-policy hashes bind the retained build inventory, not the current working tree, compiler implementation proof or runtime evidence.",
        ],
        "excluded_partitions": [
            "Fallible aligned-ELF copy, LoadedPage Vec, final-flags Vec and prepared-name scratch; transient realloc old/new overlap.",
            "Vec/String spare capacity, allocator rounding/metadata, Box conversion, BTreeMap nodes, shared scheduler/ready/death/log/quota containers and unrelated post-arm allocations.",
            "Runtime Operations/mailbox/request/reply/death/completion backing and pre-arm transfers with immutable Unknown origin.",
            "Shared SAS roots/intermediate tables, boot mappings, bitmap, reserved heap backing and shared physical union/release joins.",
            "Failure handoff after load_segments and before CellSegments frame RAII; failed preparation retention/release is not proved by successful-path bounds.",
        ],
        "ownership": "Immutable allocation origin and exclusive semantic adoption are separate facts; Preparation or Execution provenance alone is not per-Cell ownership.",
        "fit_proven": False, "coverage_proven": False, "no_reallocation_proven": False,
        "runtime_evidence": False,
        "evidence_ceiling": "Digest-bound static partition arithmetic only; no overall 1024-entry fit, complete semantic heap/physical coverage, reclamation, quota, authority or production qualification.",
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", required=True, type=Path, help="retained build.json manifest")
    parser.add_argument("--out", required=True, type=Path, help="new JSON output path; must not exist")
    args = parser.parse_args(argv)
    try:
        result = certificate(args.build)
        encoded = json.dumps(result, sort_keys=True, indent=2, allow_nan=False) + "\n"
        with args.out.open("x", encoding="utf-8") as stream:
            stream.write(encoded)
    except (OSError, InvalidCertificate) as error:
        parser.exit(1, f"p01_elf_bounds: {error}\n")
    print(args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
