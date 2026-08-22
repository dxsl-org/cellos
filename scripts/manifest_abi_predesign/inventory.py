"""Consumer occurrence discovery and inventory validation."""
from __future__ import annotations

import re
from pathlib import Path
from typing import Callable

from .common import (
    CLASSIFICATION_KEYS, ENTRY_KEYS, EXCLUDED_SCAN_PATHS, INVENTORY_KEYS,
    OCCURRENCE_KEYS, ROOTS, SUFFIXES, TOKENS, digest, exact_keys, file_digest,
)
from .state import validate_derived_source_state


def line_symbol(line_number: int, line: str) -> str:
    return f"L{line_number}: {line.strip()}"


def scan_sources(root: Path) -> list[dict]:
    found = []
    token_pattern = re.compile("|".join(re.escape(token) for token in sorted(TOKENS, key=lambda token: (-len(token), token))))
    for base in ROOTS:
        for path in (root / base).rglob("*"):
            if not path.is_file() or path.suffix not in SUFFIXES:
                continue
            if path.is_symlink():
                raise ValueError(f"unsafe source symlink: {path}")
            relative = path.relative_to(root).as_posix()
            if relative in EXCLUDED_SCAN_PATHS:
                continue
            source_sha256 = file_digest(path)
            for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                for match in token_pattern.finditer(line):
                    found.append({"path": relative, "source_sha256": source_sha256, "line": line_number, "symbol": line_symbol(line_number, line), "token": match.group(0)})
    return sorted(found, key=lambda item: (item["path"], item["line"], item["token"], item["symbol"]))


def validate_inventory(doc: dict, root: Path, scan: bool, scanner: Callable[[Path], list[dict]] = scan_sources) -> None:
    exact_keys(doc, INVENTORY_KEYS, "inventory")
    validate_derived_source_state(doc, root, "inventory")
    entries = doc["entries"]
    if entries != sorted(entries, key=lambda item: (item["path"], item["symbol"], item["operation"])):
        raise ValueError("consumer entries are not stable sorted")
    if len({entry["path"] for entry in entries}) != len(entries):
        raise ValueError("consumer path classification is ambiguous")
    for entry in entries:
        exact_keys(entry, ENTRY_KEYS, f"consumer {entry.get('consumer_id')}")
        if entry["emitted_version_class"] not in ("none", "v1", "v2"):
            raise ValueError("unsupported emitted version")
        source = root / entry["path"]
        if not source.is_file() or source.is_symlink() or file_digest(source) != entry["source_sha256"]:
            raise ValueError("consumer source digest drift")
        occurrences = entry["classified_occurrences"]
        if occurrences != sorted(occurrences, key=lambda item: (item["line"], item["token"], item["symbol"])):
            raise ValueError("consumer occurrences are not stable sorted")
        for occurrence in occurrences:
            exact_keys(occurrence, OCCURRENCE_KEYS, f"consumer occurrence {entry['consumer_id']}")
            exact_keys(occurrence["classification"], CLASSIFICATION_KEYS, f"consumer occurrence classification {entry['consumer_id']}")
            if occurrence["classification"] != {"consumer_id": entry["consumer_id"], "operation": entry["operation"], "roles": entry["roles"]}:
                raise ValueError("consumer occurrence classification drift")
    contract = doc["discovery_contract"]
    required_contract = {"roots", "filename_suffixes", "tokens", "exclusions", "required_match_count", "required_match_sha256", "required_manual_paths", "required_manual_paths_sha256", "source_scan_repin"}
    exact_keys(contract, required_contract, "discovery contract")
    repin = contract["source_scan_repin"]
    required_repin = {"invalidation_reason", "invalidated_record_format", "invalidated_match_count", "invalidated_match_sha256", "repinned_record_format", "repinned_match_count", "repinned_match_sha256", "change_classification"}
    exact_keys(repin, required_repin, "source scan re-pin provenance")
    if repin["invalidated_record_format"] != "path-token-set-v1" or repin["repinned_record_format"] != "occurrence-symbol-source-v2":
        raise ValueError("source scan re-pin format drift")
    if contract["roots"] != list(ROOTS) or contract["filename_suffixes"] != list(SUFFIXES) or contract["tokens"] != list(TOKENS) or set(contract["exclusions"][-len(EXCLUDED_SCAN_PATHS):]) != EXCLUDED_SCAN_PATHS:
        raise ValueError("source scan contract drift")
    if doc["unclassified_matches"] or digest(entries) != doc["inventory_sha256"]:
        raise ValueError("inventory omission or relabel drift")
    if digest(contract["required_manual_paths"]) != contract["required_manual_paths_sha256"]:
        raise ValueError("manual required set drift")
    if scan:
        found = scanner(root)
        if len(found) != contract["required_match_count"] or digest(found) != contract["required_match_sha256"] or len(found) != repin["repinned_match_count"] or digest(found) != repin["repinned_match_sha256"]:
            raise ValueError("source consumer occurrence set drift")
        by_path, raw_paths = {entry["path"]: entry for entry in entries}, {item["path"] for item in found}
        if not raw_paths <= set(by_path) or not set(contract["required_manual_paths"]) <= set(by_path):
            raise ValueError("consumer inventory is incomplete")
        for path, entry in by_path.items():
            expected = [{"line": item["line"], "symbol": item["symbol"], "token": item["token"], "classification": {"consumer_id": entry["consumer_id"], "operation": entry["operation"], "roles": entry["roles"]}} for item in found if item["path"] == path]
            if entry["classified_occurrences"] != expected:
                raise ValueError("consumer occurrence inventory is incomplete")
