#!/usr/bin/env python3
"""Replay captured heap Layouts with the checksum-bound allocator; never qualify P01."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import tomllib

from p01_trace import parse_capture

PACKAGE = "linked_list_allocator-0.10.6"
CHECKSUM = "2b23ac50abb8261cb38c6e2a7192d3302e0836dac1628f6a93b82b4fad185897"
HEAP_SHA256 = "df8533091e15cfd9d6d68168c35f42f51a5851edb35fdd91c9c4718a1ce10a6a"
REPO = Path(__file__).resolve().parents[2]

# Exercise the real Heap, not a second implementation of its alignment rule.
REPLAY = r'''
use std::{alloc::Layout, io::{self, Read}, ptr::NonNull};
use linked_list_allocator::Heap;
fn main() {
    assert_eq!(std::mem::size_of::<usize>(), 8);
    let backing = Layout::from_size_align(4 * 1024 * 1024, 4096).unwrap();
    let ptr = NonNull::new(unsafe { std::alloc::alloc(backing) }).unwrap();
    let mut heap = Heap::empty();
    unsafe { heap.init(ptr.as_ptr(), backing.size()); }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap();
    for line in input.lines() {
        let mut fields = line.split_whitespace();
        let size: usize = fields.next().unwrap().parse().unwrap();
        let align: usize = fields.next().unwrap().parse().unwrap();
        assert!(fields.next().is_none());
        let layout = Layout::from_size_align(size, align).unwrap();
        let before = heap.used();
        let allocation = heap.allocate_first_fit(layout).unwrap();
        let charge = heap.used().checked_sub(before).unwrap();
        unsafe { heap.deallocate(allocation, layout); }
        assert_eq!(heap.used(), before);
        println!("{}", charge);
    }
    drop(heap);
    unsafe { std::alloc::dealloc(ptr.as_ptr(), backing); }
}
'''


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def load_json(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result
    return json.loads(path.read_bytes(), object_pairs_hook=unique)


def checked_inputs(build_path, source, archive):
    build = load_json(build_path)
    inputs_path = build_path.parent / "inputs.json"
    inputs_data = inputs_path.read_bytes()
    require(digest(inputs_data) == build["inputs_sha256"], "source inventory digest mismatch")
    inputs = load_json(inputs_path)
    lock_data = (REPO / "Cargo.lock").read_bytes()
    require(digest(lock_data) == inputs["Cargo.lock"], "current lock differs from retained build")
    for name in ("Cargo.toml", "kernel/Cargo.toml"):
        require(digest((REPO / name).read_bytes()) == inputs[name],
                "current package manifest differs from retained build: " + name)
    require(inputs["kernel/src/memory/heap.rs"] == HEAP_SHA256
            and digest((REPO / "kernel/src/memory/heap.rs").read_bytes()) == HEAP_SHA256,
            "unsupported kernel allocator policy")
    packages = [p for p in tomllib.loads(lock_data.decode())["package"]
                if p["name"] == "linked_list_allocator"]
    require(len(packages) == 1 and packages[0]["version"] == "0.10.6"
            and packages[0]["checksum"] == CHECKSUM, "unsupported locked allocator")
    require(digest(archive.read_bytes()) == CHECKSUM, "allocator archive checksum mismatch")
    hashes = {}
    with tarfile.open(archive, "r:gz") as bundle:
        for member in bundle.getmembers():
            prefix = PACKAGE + "/"
            if member.isfile() and member.name.startswith(prefix):
                relative = member.name[len(prefix):]
                if relative.startswith("src/") and relative.endswith(".rs"):
                    data = bundle.extractfile(member).read()
                    require((source / relative).read_bytes() == data,
                            "allocator source differs from archive: " + relative)
                    hashes[relative] = digest(data)
    require("src/lib.rs" in hashes and "src/hole.rs" in hashes, "missing allocator sources")
    require(any(c.get("returncode") == 0 and c.get("argv", [])[:2] == ["cargo", "build"]
                and "cellos-kernel" in c["argv"] and "--locked" in c["argv"]
                and "x86_64-unknown-none" in c["argv"] for c in build["commands"]),
            "no successful locked x86_64 kernel build")
    return {"build_sha256": digest(build_path.read_bytes()),
            "inputs_sha256": digest(inputs_data), "cargo_lock_sha256": digest(lock_data),
            "allocator_archive_sha256": CHECKSUM, "allocator_sources_sha256": hashes,
            "kernel_heap_sha256": HEAP_SHA256}


def replay(source, layouts):
    with tempfile.TemporaryDirectory(prefix="p01-allocator-replay-") as temporary:
        directory = Path(temporary)
        library = directory / "liblinked_list_allocator.rlib"
        subprocess.run(["rustc", "--edition=2018", "--crate-name", "linked_list_allocator",
                        "--crate-type", "rlib", str(source / "src/lib.rs"), "-o", str(library)],
                       check=True, capture_output=True)
        helper = directory / "replay.rs"
        helper.write_text(REPLAY)
        binary = directory / "replay"
        subprocess.run(["rustc", "--edition=2021", str(helper), "--extern",
                        "linked_list_allocator=" + str(library), "-o", str(binary)],
                       check=True, capture_output=True)
        result = subprocess.run([str(binary)], input="".join(f"{s} {a}\n" for s, a in layouts),
                                text=True, check=True, capture_output=True)
        charges = [int(line) for line in result.stdout.splitlines()]
        require(len(charges) == len(layouts), "allocator replay output count mismatch")
        return dict(zip(layouts, charges)), digest(REPLAY.encode())


def certificate(build_path, captures, source, archive):
    require(bool(captures) and len(set(p.resolve() for p in captures)) == len(captures),
            "empty/duplicate captures")
    provenance = checked_inputs(build_path, source, archive)
    parsed = []
    layouts = set()
    for directory in captures:
        require(digest((directory / "build.json").read_bytes()) == provenance["build_sha256"],
                "capture belongs to a different build")
        evidence = parse_capture(directory)
        require(evidence["structurally_complete"], "capture is not structurally complete")
        baseline = evidence["ownership_evidence"]["heap_baseline"]
        require(baseline is not None, "capture has no baseline heap inventory")
        entries = [r["entry"] for r in baseline["created"] + baseline["released"]]
        entries.extend(r["baseline"] for r in baseline["preserved"])
        for entry in entries:
            layouts.add((entry[1], entry[2]))
        parsed.append((directory, evidence, baseline))
    charges, helper_digest = replay(source, sorted(layouts))
    rows = []
    for directory, evidence, baseline in parsed:
        total = lambda items: sum(charges[(r["entry"][1], r["entry"][2])] for r in items)
        created = total(baseline["created"])
        released = total(baseline["released"])
        preserved = sum(charges[(r["baseline"][1], r["baseline"][2])]
                        for r in baseline["preserved"])
        delta = created - released
        observed = baseline["allocator_used_byte_delta"]
        baseline_known = preserved + released
        final_known = preserved + created
        rows.append({"capture_uuid": evidence["capture_uuid"],
                     "raw_sha256": digest((directory / "serial.raw.log").read_bytes()),
                     "capture_sha256": digest((directory / "capture.json").read_bytes()),
                     "created_charged_bytes": created, "released_charged_bytes": released,
                     "requested_byte_delta": baseline["requested_byte_delta"],
                     "replayed_charged_byte_delta": delta, "observed_allocator_used_byte_delta": observed,
                     "unmatched_charged_byte_delta": observed - delta,
                     "rounding_byte_delta": delta - baseline["requested_byte_delta"],
                     "baseline_uninventoried_used_bytes": baseline["source"][7] - baseline_known,
                     "final_uninventoried_used_bytes": baseline["final_accounting"][2] - final_known,
                     "changed_layout_rounding": [{"incarnation": r["incarnation"],
                         "transition": kind, "requested": r["entry"][1],
                         "charged": charges[(r["entry"][1], r["entry"][2])]}
                         for kind in ("created", "released") for r in baseline[kind]
                         if charges[(r["entry"][1], r["entry"][2])] != r["entry"][1]],
                     "qualification_blockers_unchanged": evidence["qualification_blockers"]})
    return {"version": 1, "scope": "private-P01-captured-Layout-accounting-replay",
            "provenance": provenance, "replay_source_sha256": helper_digest,
            "rustc": subprocess.check_output(["rustc", "-Vv"], text=True).strip(),
            "replay": "Host 64-bit real Heap allocate/deallocate for each distinct captured Layout; not kernel execution or original heap topology replay.",
            "captures": rows, "all_observed_deltas_reconciled": all(r["unmatched_charged_byte_delta"] == 0 for r in rows),
            "coverage_proven": False, "fit_proven": False, "fully_settled": False,
            "evidence_ceiling": "Per-Layout charged-byte delta only. Equal uninventoried remainders do not prove no churn, complete ownership, table fit, physical union or acceptance of unequal raw residuals."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", type=Path, required=True)
    parser.add_argument("--captures", type=Path, nargs="+", required=True)
    parser.add_argument("--allocator-source", type=Path, required=True)
    parser.add_argument("--allocator-archive", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = certificate(args.build, args.captures, args.allocator_source, args.allocator_archive)
    with args.out.open("x") as output:
        json.dump(result, output, indent=2, sort_keys=True)
        output.write("\n")
    print(args.out)


if __name__ == "__main__":
    main()
