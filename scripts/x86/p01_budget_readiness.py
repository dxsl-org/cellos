#!/usr/bin/env python3
"""Analyze pinned P01 captures without choosing or approving resource budgets.

Raw stage deltas, independent peaks and frozen owner maps are distinct evidence.
Control intervals are sensitivity calculations, never leak tolerances or bounds.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import sys
import uuid

# The analysis can run outside the source tree while a pinned capture is active.
sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path.cwd() / "scripts/x86"))
import p01_trace
from p01_trace import InvalidTrace, parse_capture, unique_object

COUNTS = ("0", "1", "4", "8", "denied")
METRICS = ("actual_heap_used_bytes", "used_frames", "execution_context_quota_charge",
           "allocated_stack_bytes", "segment_bytes", "active_operations",
           "reserved_completions", "outstanding_cleanup_obligations",
           "unknown_retained_obligations")
UNSUPPORTED = {
    "aggregate_cpu": "No admitted aggregate execution unit/period/burst/overshoot measurement; scheduler ticks and latency are not CPU entitlement.",
    "threads_and_shared_cspace": "Fixture is root-only, not same-incarnation thread/SMP or lineage/handle saturation measurement.",
    "operations_and_completion_retention": "Parked footprint is not accepted/retained/unknown operation saturation and authoritative consume measurement.",
    "authority_controller_router": "Actual genesis, persistence, reserve/live-claim, queues and resize producer costs are not supplied by this fixture.",
    "broker_and_lifecycle": "Session/reconnect/durable-key/unknown-generation retention costs are outside this fixture.",
    "production_capacity": "QEMU-TCG one-CPU finite samples do not qualify physical Intel/VMX, 200 overlapping clients or production capacity.",
}


def load_json(path):
    return json.loads(Path(path).read_text(), object_pairs_hook=unique_object)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def interval(values):
    values = list(values)
    if not values or any(value is None for value in values):
        return None
    return {"min": min(values), "max": max(values)}


def difference_interval(raw, control):
    if raw is None or control is None:
        return None
    return {"min": raw["min"] - control["max"],
            "max": raw["max"] - control["min"]}


def owner_partition(entries, charges):
    charged = {row[0]: row[2] for row in charges}
    groups = defaultdict(lambda: {"objects": 0, "requested_bytes": 0, "charged_bytes": 0})
    for entry in entries:
        owner = tuple(entry[11:15])
        kind = entry[10]
        partition = "unclassified" if kind == 0 else ("exclusive" if any(owner) else "shared")
        key = (partition, kind, owner)
        group = groups[key]
        group["objects"] += 1
        group["requested_bytes"] += entry[1]
        group["charged_bytes"] += charged[entry[0]]
    return [{"partition": part, "semantic_kind": kind,
             "owner_tid_root_cell_generation": list(owner), **totals}
            for (part, kind, owner), totals in sorted(groups.items())]

def charged_inventory(entries, charges):
    raw = {row[0]: row for row in entries}
    return {row[1]: {"incarnation": row[1], "pointer": row[0],
                     "requested_bytes": raw[row[0]][1], "alignment": raw[row[0]][2],
                     "charged_bytes": row[2], "birth_epoch": row[3], "prearm": row[4],
                     "semantic_kind": raw[row[0]][10],
                     "semantic_owner": list(raw[row[0]][11:15]),
                     "origin_kind": raw[row[0]][3], "origin_attempt": raw[row[0]][4],
                     "adopted": raw[row[0]][5], "origin_owner": list(raw[row[0]][6:10]),
                     "transferred_from": list(raw[row[0]][15:19])}
            for row in charges}


def inventory_changes(before, after, subjects):
    created = [after[token] for token in sorted(after.keys() - before.keys())]
    released = [before[token] for token in sorted(before.keys() - after.keys())]
    preserved = sorted(before.keys() & after.keys())
    selected = []
    for index, subject in sorted(subjects.items(), key=lambda row: int(row[0])):
        owner = list(subject[:4])
        baseline_owned = [row for row in before.values() if row["semantic_owner"] == owner]
        final_owned = [row for row in after.values() if row["semantic_owner"] == owner]
        selected.append({"role": "driver" if int(index) == 0 else "child",
                         "owner_tid_root_cell_generation": owner,
                         "baseline_charged_bytes": sum(row["charged_bytes"] for row in baseline_owned),
                         "after_charged_bytes": sum(row["charged_bytes"] for row in final_owned),
                         "baseline_owned_incarnations": sorted(row["incarnation"] for row in baseline_owned),
                         "after_owned_incarnations": sorted(row["incarnation"] for row in final_owned)})
    result = {"created": created, "released": released, "preserved_incarnations": preserved,
            "preserved_state_changes": [
                {"incarnation": token, "before": before[token], "after": after[token]}
                for token in preserved if before[token] != after[token]],
            "selected_semantic_owner_snapshots": selected,
            "released_charged_bytes": sum(row["charged_bytes"] for row in released),
            "created_charged_bytes": sum(row["charged_bytes"] for row in created),
            "transient_lifetime_complete": False}
    # Surviving allocations can transfer semantic ownership without rebirth.
    delta = defaultdict(int)
    for sign, inventory in ((-1, before), (1, after)):
        for row in inventory.values():
            delta[(row["semantic_kind"], tuple(row["semantic_owner"]))] += sign * row["charged_bytes"]
    result["semantic_charged_byte_deltas"] = [
        {"semantic_kind": kind, "owner_tid_root_cell_generation": list(owner), "charged_byte_delta": size}
        for (kind, owner), size in sorted(delta.items()) if size]
    return result


def incarnation_changes(heap, subjects):
    charged = heap["heap_charged"]
    before = charged_inventory([row[1:] for row in heap["heap_baseline"]["entries"]], charged["baseline_entries"])
    after = charged_inventory(heap["heap_entries"], charged["final_entries"])
    return inventory_changes(before, after, subjects)



def physical_snapshot_deltas(evidence):
    physical = evidence["physical_inventory"]
    if physical is None:
        return None
    if evidence["stage_metrics"]["1"]["used_frames"] is None or evidence["snapshot_sidecars"]["1:1"][2] != 1:
        return None
    entries = {row[0]: row for row in physical["entries"]}
    def population(sample):
        epoch = evidence["snapshot_sidecars"][f"{sample}:1"][0]
        return {token for token, row in entries.items()
                if (row[11] if row[5] in (6, 7) else row[2]) <= epoch
                and (row[3] == 0 or epoch < row[3])}
    before = population("1")
    result = {}
    for sample, metrics in evidence["stage_metrics"].items():
        if sample == "1":
            continue
        if metrics["used_frames"] is None or evidence["snapshot_sidecars"][f"{sample}:1"][2] != 1:
            result[sample] = None
            continue
        after = population(sample)
        created, released = after - before, before - after
        observed = metrics["used_frames"] - evidence["stage_metrics"]["1"]["used_frames"]
        result[sample] = {
            "created_incarnations": sorted(created), "released_incarnations": sorted(released),
            "preserved_count": len(before & after), "frame_count_delta": len(created) - len(released),
            "actual_used_frame_delta": observed,
            "unmatched_frame_count_delta": observed - (len(created) - len(released)),
            "created_by_final_history_source_kind": dict(Counter(entries[token][5] for token in created)),
            "released_by_final_history_source_kind": dict(Counter(entries[token][5] for token in released)),
            "semantic_attribution_proven": False,
            "reference_topology_proven": False,
        }
    return result


def capacity_basis(runs, build_sha256, build_path=None):
    """Conditional partition bounds never replace a missing all-path bound."""
    provenance = None
    if build_path is not None:
        path = Path(build_path).resolve()
        manifest = load_json(path)
        if digest(path) != build_sha256:
            raise InvalidTrace("capacity source manifest differs from capture build")
        inputs_path = path.parent / "inputs.json"
        if digest(inputs_path) != manifest["inputs_sha256"]:
            raise InvalidTrace("capacity source input inventory hash mismatch")
        inputs = load_json(inputs_path)
        names = ("Cargo.lock", "rust-toolchain.toml", "kernel/src/memory/heap.rs",
                 "kernel/src/memory/heap/p01_owner.rs", "kernel/src/memory/frame.rs",
                 "kernel/src/p01_retained.rs", "kernel/src/loader/elf.rs",
                 "kernel/src/task/elf_prepare.rs", "kernel/src/task/tcb.rs",
                 "kernel/src/memory/deferred_release.rs",
                 "cells/tests/bench/src/scenarios/parked_footprint.rs")
        if any(name not in inputs for name in names):
            raise InvalidTrace("capacity source prerequisites absent from pinned inventory")
        provenance = {"build_sha256": build_sha256, "inputs_sha256": digest(inputs_path),
                      "rule_source_pins": {name: inputs[name] for name in names},
                      "external_dependency_sources_exhaustively_pinned": False}
    scenarios = []
    for count in ("0", "1", "4", "8"):
        n = int(count)
        scenarios.append({
            "children": n, "retained_replicates": sum(run["count"] == count for run in runs),
            "conditional_trace_record_bound": 51 + 21 * n, "trace_capacity": 256,
            "conditional_trace_schema_fits": 51 + 21 * n <= 256,
            "conditional_historical_attempt_bound": n + 2, "attempt_capacity": 10,
            "conditional_exclusive_preparation_object_bound": 6 * (n + 1),
            "fixture_submission_call_bound": 2 * n,
            "conditions": ["fixed root-only event schema; at most two driver route attempts",
                           "one child attempt each; no retries/replacements/threads",
                           "successful preparation partition excludes runtime/scratch/failures"],
            "all_path_heap_live_upper_bound": None,
            "all_path_physical_history_upper_bound": None,
            "all_path_retained_interval_upper_bound": None,
            "workload_capacity_fit_proven": False,
        })
    cutoffs = []
    for run in runs:
        for sample, graph in (run.get("cpu_topology") or {}).get("stages", {}).items():
            source = graph["source"]
            cutoffs.append({
                "capture_uuid": run["capture_uuid"], "sample": int(sample),
                "required_nodes": source[14], "node_capacity": source[9],
                "required_edges": source[15], "edge_capacity": source[10],
                "required_counts_exact": source[11] == 1,
                "snapshot_storage_fit_proven": graph["snapshot_storage_fit_proven"],
                "workload_capacity_fit_proven": False,
            })
    return {
        "format": "p01-private-capacity-basis-v1", "outcome": "REFUSED",
        "provenance": provenance, "scenario_partition_bounds": scenarios,
        "topology_cutoff_storage_proofs": cutoffs,
        "required_all_path_formulas": {
            "heap": "max_cut(B + P + D + sum(R_i + S_i + X_i) + F) <= 1024",
            "physical_history": "prearm_used_bits + all_later_births_including_reuse_and_failures <= 8192",
            "retained_ranges": "max_cut(live selected retained intervals) <= 2048; bytes use union",
        },
        "unresolved_terms": {
            "B/P": "prearm/background producer and survivor maximum",
            "S_i/X_i": "all loader scratch and realloc old/new overlap",
            "F": "all refusal/publication/retirement and retained failure interleavings",
            "physical history": "finite successful-birth/reuse bound for every producer",
            "global references": "non-CPU, unpublished/retired roots and semantic allocation owners",
        },
        "denied_is_separate_failure_branch": True,
        "workload_capacity_fit_proven": False, "global_physical_topology_proven": False,
        "interpretation": "Exact cutoff graph storage fit is not future/all-path workload fit. Conditional trace/attempt/preparation bounds do not cover global heap/history/retention.",
    }


def summarize_run(metadata, evidence):
    stages = evidence["stage_metrics"]
    heap = evidence["ownership_evidence"]
    charged = heap["heap_charged"]
    physical = evidence["physical_inventory"]
    gaps = list(evidence["qualification_blockers"])
    if charged is None:
        gaps.append("actual charged-byte inventory absent")
    if heap.get("heap_stages") is None:
        gaps.append("live/settled heap semantic census absent")
    if physical is None:
        gaps.append("physical bitmap inventory absent")
    elif any(row is None or row["unmatched_allocator_used_frames"] != 0
             for row in physical.get("sample_populations", {}).values()):
        gaps.append("physical bitmap samples not completely reconciled")
    result = {
        "capture_uuid": metadata["capture_uuid"], "count": metadata["count"],
        "replica": metadata["replica"], "build_sha256": metadata["build_sha256"],
        "cpus": metadata["cpus"], "image_sha256": metadata["image_sha256"],
        "structurally_complete": evidence["structurally_complete"],
        "fully_settled": evidence["fully_settled"],
        "denied_control_accepted": evidence["denied_control_accepted"],
        "requested": evidence["requested"], "admitted": evidence["admitted"],
        "workload_outcome": evidence["workload_outcome"],
        "stages": stages, "raw_stage_minus_pre": evidence["raw_stage_minus_pre"],
        "stage_ticks": evidence["stage_ticks"],
        "independent_peaks": evidence["global_independent_peaks"],
        "private_trace_storage": evidence["private_trace_storage"],
        "final_status": evidence["final_status"],
        "late_status_flags": evidence["late_status_flags"],
        "qualification_blockers": sorted(set(gaps)),
        "physical_sample_populations": None if physical is None else physical.get("sample_populations"),
    }
    result["cpu_topology"] = heap.get("cpu_topology")
    result["noncpu_roots"] = heap.get("noncpu_roots")
    result["noncpu_topology"] = heap.get("noncpu_topology")
    result["vtd_structures"] = heap.get("vtd_structures")
    result["physical_snapshot_deltas"] = physical_snapshot_deltas(evidence)
    if metadata["count"] != "denied":
        result["settled_samples_stable"] = all(stages[str(s)] == stages["4"] for s in (5, 6))
    if charged is not None:
        result["heap_charge"] = {key: charged[key] for key in (
            "baseline_partitions", "final_partitions", "charged_byte_delta",
            "unmatched_allocator_used_byte_delta", "measured_live_highwater",
            "prearm_allocations", "prearm_live_objects", "fit_proven", "coverage_proven")}
        result["heap_charge"]["baseline_objects"] = charged["baseline_source"][7]
        result["heap_charge"]["final_objects"] = charged["final_source"][7]
        result["baseline_semantic_owners"] = owner_partition(
            [row[1:] for row in heap["heap_baseline"]["entries"]], charged["baseline_entries"])
        result["final_semantic_owners"] = owner_partition(heap["heap_entries"], charged["final_entries"])
        result["heap_incarnation_changes"] = incarnation_changes(heap, evidence["subjects"])
        heap_stages = heap.get("heap_stages")
        result["heap_stage_ownership"] = None
        if heap_stages is not None:
            before = charged_inventory(
                [row[1:] for row in heap["heap_baseline"]["entries"]], charged["baseline_entries"])
            result["heap_stage_ownership"] = {}
            for sample, inventory in heap_stages.items():
                after = charged_inventory(inventory["entries"], inventory["charged_entries"])
                changes = inventory_changes(before, after, evidence["subjects"])
                delta = inventory["source"][5] - charged["baseline_source"][5]
                charged_delta = changes["created_charged_bytes"] - changes["released_charged_bytes"]
                semantic_delta = sum(row["charged_byte_delta"] for row in changes["semantic_charged_byte_deltas"])
                result["heap_stage_ownership"][sample] = {
                    "source": inventory["source"], "partitions": inventory["partitions"],
                    "semantic_owners": owner_partition(inventory["entries"], inventory["charged_entries"]),
                    "changes_from_pre": changes, "allocator_used_byte_delta": delta,
                    "unmatched_charged_byte_delta": delta - charged_delta,
                    "heap_residual_semantically_decomposed": delta == charged_delta == semantic_delta
                        and inventory["partitions"]["unowned"] == charged["baseline_partitions"]["unowned"] == 0,
                }
    return result


def analyze(matrix_paths, build_path=None):
    if isinstance(matrix_paths, (str, Path)):
        matrix_paths = [matrix_paths]
    paths = [Path(path).resolve() for path in matrix_paths]
    if len(set(paths)) != len(paths) or not paths:
        raise InvalidTrace("duplicate or absent matrix input")
    rows, matrix_pins, build_hashes = [], [], set()
    for path in paths:
        matrix = load_json(path)
        if type(matrix.get("cpus")) is not int or matrix["cpus"] != 1:
            raise InvalidTrace("budget basis requires the declared one-CPU matrix")
        captures = matrix.get("captures")
        if not isinstance(captures, list) or not captures:
            raise InvalidTrace("matrix captures absent")
        build_hashes.add(matrix["build_sha256"])
        rows.extend((path.parent, matrix, row) for row in captures)
        matrix_pins.append({"path": str(path), "sha256": digest(path)})
    if len(build_hashes) != 1:
        raise InvalidTrace("mixed pinned build revisions")
    build_sha256 = next(iter(build_hashes))
    seen, frequencies, runs, capture_pins = set(), Counter(), [], []
    producer_fingerprint = None
    for directory_root, matrix, row in rows:
        count, replica, identity = row["count"], row["replica"], row["capture_uuid"]
        if count not in COUNTS or type(replica) is not int or replica not in range(3):
            raise InvalidTrace("matrix scenario/replica outside declared workload")
        if str(uuid.UUID(identity)) != identity or (count, replica) in seen:
            raise InvalidTrace("duplicate or noncanonical capture identity/scenario")
        if identity in {run["capture_uuid"] for run in runs}:
            raise InvalidTrace("capture reused as an independent replicate")
        seen.add((count, replica))
        directory = directory_root / identity
        metadata = load_json(directory / "capture.json")
        if any(metadata[key] != value for key, value in (
                ("count", count), ("replica", replica), ("capture_uuid", identity),
                ("cpus", 1), ("build_sha256", matrix["build_sha256"]))):
            raise InvalidTrace("matrix/capture scenario or pinned build mismatch")
        command = list(metadata["command"])
        command[command.index("-cdrom") + 1] = "<scenario-image>"
        fingerprint = tuple(metadata[key] for key in (
            "qemu_binary_sha256", "runner_sha256", "parser_sha256", "manifest_hex", "qemu_version")) + (tuple(command),)
        if producer_fingerprint is not None and fingerprint != producer_fingerprint:
            raise InvalidTrace("mixed emulator, producer, manifest or VM configuration")
        producer_fingerprint = fingerprint
        evidence = parse_capture(directory)
        if not evidence["structurally_complete"]:
            raise InvalidTrace(identity + ": " + "; ".join(evidence["qualification_blockers"]))
        if count == "denied" and not evidence["denied_control_accepted"]:
            raise InvalidTrace("denied control did not prove its exact source refusal")
        # Never trust cached evidence.json or matrix qualification booleans.
        runs.append(summarize_run(metadata, evidence))
        frequencies[count] += 1
        capture_pins.append({"capture_uuid": identity,
                             "capture_sha256": digest(directory / "capture.json"),
                             "raw_sha256": digest(directory / "serial.raw.log"),
                             "build_sha256": digest(directory / "build.json")})
    missing = [f"{count}:{replica}" for count in COUNTS for replica in range(3)
               if (count, replica) not in seen]
    normal = [run for run in runs if run["count"] != "denied"]
    controls = [run for run in normal if run["count"] == "0"]
    cohorts = {}
    for count in COUNTS[:-1]:
        cohort = [run for run in normal if run["count"] == count]
        if not cohort:
            continue
        stage_ranges, deltas, sensitivity = {}, {}, {}
        for sample in range(1, 7):
            key = str(sample)
            stage_ranges[key] = {metric: interval(run["stages"][key][metric] for run in cohort)
                                 for metric in METRICS}
            if sample > 1:
                deltas[key] = {metric: interval(run["raw_stage_minus_pre"][key][metric] for run in cohort)
                               for metric in METRICS}
                sensitivity[key] = {metric: difference_interval(deltas[key][metric], interval(
                    run["raw_stage_minus_pre"][key][metric] for run in controls)) for metric in METRICS}
        cohorts[count] = {"replicates": len(cohort), "stage_ranges": stage_ranges,
                          "raw_delta_ranges": deltas,
                          "control_difference_sensitivity_intervals": sensitivity,
                          "heap_peak_bytes": interval(run["independent_peaks"][0]["value"]
                              if run["independent_peaks"][0]["status"] == 1 else None for run in cohort),
                          "frame_peak_count": interval(run["independent_peaks"][1]["value"]
                              if run["independent_peaks"][1]["status"] == 1 else None for run in cohort),
                          "census_live_highwater": interval(run.get("heap_charge", {}).get("measured_live_highwater") for run in cohort)}
    blockers = sorted({gap for run in normal for gap in run["qualification_blockers"]})
    if missing:
        blockers.append("missing scenario/replicates: " + ", ".join(missing))
    for dimension, reason in UNSUPPORTED.items():
        blockers.append(dimension + ": " + reason)
    observed_heap_partition_complete = not missing and bool(normal) and all(
        run.get("heap_stage_ownership") is not None
        and run["heap_charge"]["baseline_partitions"]["unowned"] == run["heap_charge"]["final_partitions"]["unowned"] == 0
        and all(stage["heap_residual_semantically_decomposed"] and stage["unmatched_charged_byte_delta"] == 0
                for stage in run["heap_stage_ownership"].values())
        for run in normal)
    return {
        "format": "p01-budget-measurement-v1", "scope": "pinned root-only QEMU-TCG one-CPU fixture",
        "matrix_pins": matrix_pins, "build_sha256": build_sha256,
        "capture_pins": capture_pins, "scenario_replicates": dict(frequencies),
        "complete_requested_matrix": not missing,
        "structural_and_provenance_valid": True,
        "observed_heap_stage_partition_complete": observed_heap_partition_complete,
        "capacity_basis": capacity_basis(runs, build_sha256, build_path),
        "observed_cpu_graph_complete": bool(runs) and all(
            run.get("cpu_topology") is not None
            and run["cpu_topology"]["cpu_graph_observed_complete"]
            for run in runs),
        "observed_noncpu_root_lifetimes_complete": bool(runs) and all(
            run.get("noncpu_roots") is not None
            and run["noncpu_roots"]["allocation_lifetime_census_complete"]
            for run in runs),
        "observed_vtd_software_structures_complete": bool(runs) and all(
            run.get("vtd_structures") is not None
            and run["vtd_structures"]["software_structure_observed_complete"]
            for run in runs),
        "observed_noncpu_software_trees_complete": bool(runs) and all(
            run.get("noncpu_topology") is not None
            and run["noncpu_topology"]["software_tree_observed_complete"]
            and run.get("vtd_structures") is not None
            and run["vtd_structures"]["software_structure_observed_complete"]
            for run in runs),
        "per_owner_highwater_complete": False,
        "budget_selection_ready": False, "candidate_budgets": None,
        "architecture_approval": False, "throughput_claim": False,
        "control_drift_subtracted": False,
        "interpretation": [
            "Ranges include every retained fresh boot; no percentile or extrapolation beyond counts0/1/4/8.",
            "Control sensitivity is raw-range minus independent control-range, not paired causality or a residual acceptance tolerance.",
            "Heap and frame peaks are independent windows; never add heap use to committed frame bytes, which include heap reservation.",
            "Sample6 settled metrics and frozen final heap accounting have distinct cutoffs; neither substitutes for the other.",
            "Live/settled semantic maps are used only when their source-coherent stage census exists; no per-owner high-water is inferred between samples.",
            "Observed high-water is a lower bound on a necessary budget, not a simultaneous fit certificate or safe upper bound.",
            "No arbitrary safety margin, rounding, profile activation, quota enforcement or approval is supplied.",
        ],
        "cohorts": cohorts, "runs": runs,
        "budget_selection_blockers": sorted(set(blockers)),
        "unsupported_dimensions": UNSUPPORTED,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", required=True, nargs="+")
    parser.add_argument("--out", required=True)
    parser.add_argument("--build", help="Pinned build.json plus inputs.json for capacity source provenance")
    parser.add_argument("--require-budget-ready", action="store_true")
    args = parser.parse_args()
    report = analyze(args.matrix, args.build)
    report["analyzer_sha256"] = digest(__file__)
    report["decoder_sha256"] = digest(p01_trace.__file__)
    with Path(args.out).open("x") as stream:
        json.dump(report, stream, indent=2, sort_keys=True)
        stream.write("\n")
    print(json.dumps({"report": str(Path(args.out).resolve()),
                      "complete_requested_matrix": report["complete_requested_matrix"],
                      "budget_selection_ready": report["budget_selection_ready"],
                      "blocker_count": len(report["budget_selection_blockers"])}, sort_keys=True))
    return 2 if args.require_budget_ready and not report["budget_selection_ready"] else 0


if __name__ == "__main__":
    sys.exit(main())
