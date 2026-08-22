"""Shared constants and integrity primitives for the frozen predesign validator."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ARTIFACTS = ROOT / ".agents/260822-phase08-manifest-predesign/artifacts"
PARENT_PLAN = ROOT / ".agents/260821-0642-app-tiers-completion/plan.md"
PLAN = ROOT / ".agents/260822-phase08-manifest-predesign/plan.md"

ROOTS = ("cells", "kernel", "libs", "scripts", "tools", "tests")
SUFFIXES = (".S", ".c", ".h", ".in", ".ld", ".py", ".rs", ".sh", ".toml", ".zig")
TOKENS = ("CellManifest", "MANIFEST_FLAGS_MASK", "MANIFEST_FLAG_", "MANIFEST_MAGIC", "MANIFEST_SECTION", "MANIFEST_VERSION", "ManifestSection", "PROTECTION_CLASS_", "TIER_", "TIER_LEGACY", "VICELL_MANIFEST", "__ViCell_manifest", "__ViCell_sig", "app_entry!", "declare_manifest", "from_manifest", "granted_protection_class", "legacy_path_caps", "manifest.declare", "manifest_bytes", "manifest_section", "service_entry!")
PHASE08_ARTIFACT_PREFIX = ".agents/260822-phase08-manifest-predesign/artifacts/"
EXCLUDED_SCAN_PATHS = frozenset({
    "scripts/validate-manifest-abi-predesign.py",
    "scripts/manifest_abi_predesign/__init__.py",
    "scripts/manifest_abi_predesign/common.py",
    "scripts/manifest_abi_predesign/schema.py",
    "scripts/manifest_abi_predesign/elf.py",
    "scripts/manifest_abi_predesign/corpus.py",
    "scripts/manifest_abi_predesign/inventory.py",
    "scripts/manifest_abi_predesign/matrix.py",
    "scripts/manifest_abi_predesign/report.py",
    "scripts/manifest_abi_predesign/cli.py",
    "scripts/manifest_abi_predesign/state.py",
    "tests/manifest-abi-predesign/test_validator.py",
    "tests/manifest-abi-predesign/test_validator_corpus.py",
    "tests/manifest-abi-predesign/test_validator_inventory.py",
    "tests/manifest-abi-predesign/validator_test_support.py",
})

CORPUS_KEYS = {"schema_version", "corpus_id", "base_revision", "derived_source_state", "frozen_phase", "hash_algorithm", "canonical_json", "abi_invariants", "tri_state_contract", "fixtures", "source_tests", "corpus_sha256"}
FIXTURE_KEYS = {"id", "origin", "artifact_kind", "bytes_hex", "size_bytes", "sha256", "record_version_class", "expected_tri_state", "expected_canonical", "expected_policy_effect", "mutation_family", "mutation_index", "evidence_refs"}
INVENTORY_KEYS = {"schema_version", "inventory_id", "base_revision", "derived_source_state", "discovery_contract", "role_vocabulary", "entries", "unclassified_matches", "inventory_sha256"}
ENTRY_KEYS = {"consumer_id", "path", "symbol", "language", "roles", "operation", "accepted_version_classes", "emitted_version_class", "absent_behavior", "malformed_behavior", "signature_relation", "route_effect", "owner_phase", "phase08_disposition", "evidence_refs", "notes", "source_sha256", "classified_occurrences"}
OCCURRENCE_KEYS = {"line", "symbol", "token", "classification"}
CLASSIFICATION_KEYS = {"consumer_id", "operation", "roles"}
DERIVED_SOURCE_STATE_KEYS = {"hash_algorithm", "inputs", "derived_source_state_sha256"}
REPORT_KEYS = {"schema_version", "report_id", "base_revision", "terminal_state", "phase08_readiness", "approval_claims", "required_dependencies", "validation_command", "validation_execution_required", "counts", "content_digests", "derived_source_state_digests", "artifact_sha256"}
SOURCE_INPUT_KEYS = {"path", "sha256"}
MATRIX_KEYS = {"schema_version", "matrix_id", "base_revision", "derived_source_state", "dimensions", "baseline_tuple", "rows", "mandatory_hostile_tuples", "unresolved_decision_ids", "matrix_sha256"}
ROW_KEYS = {"threat_id", "publisher_epoch_state", "owner_generation_state", "manifest_version_class", "route_state", "publisher_identity_binding", "artifact_binding", "signature_state", "expected_result", "invariant_ids", "rationale", "owner_phase", "evidence_refs"}

IMMUTABLE_BASE_REVISION = "2d61a4728834a0dd23ce63b6e09e5b735246c41a"
SCHEMA_FILES = {"corpus": "manifest-v1-v2-corpus.schema.json", "inventory": "manifest-consumer-inventory.schema.json", "matrix": "manifest-downgrade-matrix.schema.json"}
CORPUS_AUTHORITATIVE_SOURCES = (
    "kernel/src/loader/manifest_section_tests.rs", "kernel/src/task/manifest_v2_selftest.rs",
    "libs/api/src/abi/manifest.rs", "libs/api/src/abi/manifest_compat_tests.rs",
    "libs/api/src/abi/manifest_flags.rs", "libs/api/src/abi/manifest_macro.rs",
    "libs/api/src/abi/manifest_parse.rs", "libs/zig-syscall/src/manifest.zig",
    "tools/elf_manifest.py", "tools/test_check_elf.py",
)


def canonical(value: object) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def digest(value: object) -> str:
    return hashlib.sha256(canonical(value)).hexdigest()


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def exact_keys(value: dict, expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise ValueError(f"{label} keys differ: {sorted(set(value) ^ expected)}")


def reject_promotional_claims(*values: object) -> None:
    text = " ".join(json.dumps(value, sort_keys=True).lower() for value in values)
    for forbidden in ("phase08_ready", "phase08_complete", '"approved"', "tier2", "owner consent", "sas fallback"):
        if forbidden in text:
            raise ValueError(f"forbidden predesign claim: {forbidden}")
