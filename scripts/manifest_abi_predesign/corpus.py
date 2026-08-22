"""Manifest ABI derivation and frozen corpus validation."""
from __future__ import annotations

import hashlib
import re
from pathlib import Path

from .common import CORPUS_AUTHORITATIVE_SOURCES, CORPUS_KEYS, FIXTURE_KEYS, canonical, digest, exact_keys
from .elf import extract_elf_manifest
from .state import validate_derived_source_state


def parse_rust_constant(source: str, name: str) -> int:
    match = re.search(rf"pub const {re.escape(name)}: u(?:8|16|32) = (0x[0-9A-Fa-f_]+|[0-9_]+);", source)
    if match is None:
        raise ValueError(f"authoritative ABI source omits {name}")
    return int(match.group(1).replace("_", ""), 0)


def derive_abi_invariants(root: Path) -> dict:
    manifest = (root / "libs/api/src/abi/manifest.rs").read_text(encoding="utf-8")
    flags = (root / "libs/api/src/abi/manifest_flags.rs").read_text(encoding="utf-8")
    parser = (root / "libs/api/src/abi/manifest_parse.rs").read_text(encoding="utf-8")
    if "#[repr(C)]" not in manifest or "pub struct CellManifest" not in manifest:
        raise ValueError("manifest record layout source drift")
    fields = ("pub magic: u32", "pub version: u8", "pub tier: u8", "pub flags: u16", "pub cap_args_off: u32", "pub reserved: u32")
    if not all(field in manifest for field in fields):
        raise ValueError("manifest record field layout source drift")
    boundaries = ("bytes.len() != 8 && bytes.len() != 16", "bytes.len() != 8", "bytes.len() != 16", "bytes[6] != 0 || bytes[7] != 0", "cap_args_off != 0 || reserved != 0")
    if not all(text in parser for text in boundaries):
        raise ValueError("manifest parser boundary source drift")
    magic, version, version_v1 = (parse_rust_constant(flags, name) for name in ("MANIFEST_MAGIC", "MANIFEST_VERSION", "MANIFEST_VERSION_V1"))
    tiers = {name: parse_rust_constant(flags, name) for name in ("TIER_TRUSTED_CORE", "TIER_STANDARD", "TIER_TIER1B_FFI", "TIER_UNTRUSTED", "TIER_LEGACY")}
    aliases = {}
    for name in ("PROTECTION_CLASS_TRUSTED_CORE", "PROTECTION_CLASS_STANDARD", "PROTECTION_CLASS_FFI", "PROTECTION_CLASS_UNTRUSTED", "PROTECTION_CLASS_LEGACY"):
        match = re.search(rf"pub const {name}: u8 = (TIER_[A-Z0-9_]+);", flags)
        if match is None or match.group(1) not in tiers:
            raise ValueError(f"protection-class alias source drift: {name}")
        aliases[name] = tiers[match.group(1)]
    bits = [int(match.group(1)) for match in re.finditer(r"pub const MANIFEST_FLAG_[A-Z0-9_]+: u16 = 1 << ([0-9]+);", flags)]
    if sorted(bits) != list(range(12)):
        raise ValueError("manifest flag source drift")
    if "pub const MANIFEST_FLAGS_MASK: u16" not in flags:
        raise ValueError("manifest flag mask source drift")
    if magic != 0x56494345 or version != 2 or version_v1 != 1:
        raise ValueError("manifest version or magic source drift")
    return {
        "magic_little_endian_hex": magic.to_bytes(4, "little").hex(), "magic_u32": magic,
        "exact_record_lengths": {"v1": 8, "v2": 16},
        "legacy_aliases": {key: tiers[key] for key in ("TIER_LEGACY", "TIER_STANDARD", "TIER_TIER1B_FFI", "TIER_TRUSTED_CORE", "TIER_UNTRUSTED")},
        "protection_classes": {"ffi": aliases["PROTECTION_CLASS_FFI"], "legacy": aliases["PROTECTION_CLASS_LEGACY"], "standard": aliases["PROTECTION_CLASS_STANDARD"], "trusted-core": aliases["PROTECTION_CLASS_TRUSTED_CORE"], "untrusted": aliases["PROTECTION_CLASS_UNTRUSTED"]},
        "capability_mask_u16": sum(1 << bit for bit in bits),
        "v1_upcast": {"flags_width_bits": 8, "in_memory_version": version, "input_version": version_v1, "padding_must_be_zero": True, "protection_class": aliases["PROTECTION_CLASS_LEGACY"]},
        "v2_offsets": {"cap_args_off": 8, "flags": 6, "magic": 0, "protection_class": 5, "reserved": 12, "version": 4},
        "v2_reserved_zero_ranges": [[8, 12], [12, 16]],
    }


def classify_record(raw: bytes, abi: dict) -> tuple[str, dict | None]:
    if len(raw) not in (8, 16) or int.from_bytes(raw[:4], "little") != abi["magic_u32"]:
        return "Malformed", None
    if raw[4] == abi["v1_upcast"]["input_version"]:
        flags = raw[5]
        if len(raw) != abi["exact_record_lengths"]["v1"] or raw[6:] != b"\0\0" or flags & ~0xff:
            return "Malformed", None
        return "ValidV1", {"flags": flags, "in_memory_version": 2, "protection_class": abi["protection_classes"]["legacy"]}
    if raw[4] == 2:
        flags, allowed = int.from_bytes(raw[6:8], "little"), set(abi["protection_classes"].values())
        if len(raw) != abi["exact_record_lengths"]["v2"] or raw[5] not in allowed or flags & ~abi["capability_mask_u16"] or raw[8:] != b"\0" * 8:
            return "Malformed", None
        return "ValidV2", {"flags": flags, "in_memory_version": 2, "protection_class": raw[5]}
    return "Malformed", None


def classify_fixture(raw: bytes, kind: str, abi: dict) -> tuple[str, dict | None]:
    try:
        if kind == "record":
            return classify_record(raw, abi)
        manifest = extract_elf_manifest(raw)
        return ("Absent", None) if manifest is None else classify_record(manifest, abi)
    except ValueError:
        return "Malformed", None


def validate_authoritative_corpus_sources(doc: dict, root: Path) -> dict:
    paths = tuple(item["path"] for item in doc["derived_source_state"]["inputs"])
    if paths != CORPUS_AUTHORITATIVE_SOURCES:
        raise ValueError("corpus authoritative source set substitution")
    source_tests = doc["source_tests"]
    test_paths = {item["path"] for item in source_tests}
    if len(test_paths) != len(source_tests) or not set(CORPUS_AUTHORITATIVE_SOURCES) <= test_paths:
        raise ValueError("corpus source-test authority drift")
    if any(not item.get("anchors") for item in source_tests):
        raise ValueError("corpus source-test anchor drift")
    return derive_abi_invariants(root)


def validate_corpus(doc: dict, root: Path) -> None:
    exact_keys(doc, CORPUS_KEYS, "corpus")
    validate_derived_source_state(doc, root, "corpus")
    if doc["schema_version"] != 1 or doc["frozen_phase"] != "05":
        raise ValueError("corpus identity drift")
    abi = validate_authoritative_corpus_sources(doc, root)
    for key, value in abi.items():
        if doc["abi_invariants"].get(key) != value:
            raise ValueError(f"corpus ABI invariant drift: {key}")
    items = doc["fixtures"]
    if [item["id"] for item in items] != sorted(item["id"] for item in items):
        raise ValueError("fixtures are not stable-ID sorted")
    if len({item["id"] for item in items}) != len(items):
        raise ValueError("duplicate fixture id")
    expected_policy = {"Absent": "existing-absent-policy", "ValidV1": "continue-validation", "ValidV2": "continue-validation", "Malformed": "deny-before-task-publication"}
    for item in items:
        exact_keys(item, FIXTURE_KEYS, f"fixture {item.get('id')}")
        try:
            raw = bytes.fromhex(item["bytes_hex"])
        except ValueError as error:
            raise ValueError("malformed fixture hex") from error
        if raw.hex() != item["bytes_hex"] or len(raw) != item["size_bytes"]:
            raise ValueError("fixture byte/size drift")
        if hashlib.sha256(raw).hexdigest() != item["sha256"]:
            raise ValueError("fixture hash drift")
        tri_state, canonical_record = classify_fixture(raw, item["artifact_kind"], abi)
        if item["expected_tri_state"] != tri_state or item["expected_canonical"] != canonical_record or item["expected_policy_effect"] != expected_policy[tri_state]:
            raise ValueError("fixture independent ABI classification drift")
    ids = {item["id"] for item in items}
    required = {"record-v1-zero", "record-v1-all-legacy-flags", "record-v2-rust-default-legacy-zero", "elf32-v2-canonical", "elf64-v1-canonical", "elf64-v2-canonical", "elf64-manifest-absent", "elf64-duplicate-manifest", "elf64-manifest-sht-nobits", "elf64-unsupported-version-byte-03"}
    if not required <= ids or len([item for item in items if item["mutation_family"] == "one-bit"]) != 128:
        raise ValueError("required Phase 05 record corpus is incomplete")
    if len([item for item in items if item["id"].startswith("python-elf64-truncation-")]) != 321 or len([item for item in items if item["id"].startswith("python-elf64-stride7-mutation-")]) != 46:
        raise ValueError("required Python ELF corpus is incomplete")
    if digest(items) != doc["corpus_sha256"]:
        raise ValueError("corpus aggregate drift")
