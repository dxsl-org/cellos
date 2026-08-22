"""Report, plan, and cross-artifact validation orchestration."""
from __future__ import annotations

from pathlib import Path
from typing import Callable

from .common import IMMUTABLE_BASE_REVISION, REPORT_KEYS, digest, exact_keys, file_digest, reject_promotional_claims
from .corpus import validate_corpus
from .inventory import scan_sources, validate_inventory
from .matrix import validate_matrix
from .schema import validate_artifact_schemas
from .state import validate_common_base_revision


def validate_report(report: dict, corpus: dict, inventory: dict, matrix: dict, root: Path) -> None:
    exact_keys(report, REPORT_KEYS, "predesign report")
    validate_common_base_revision(corpus, inventory, matrix)
    if report["base_revision"] != IMMUTABLE_BASE_REVISION or report["terminal_state"] != "PREDESIGN_COMPLETE / PHASE08_BLOCKED":
        raise ValueError("predesign report identity drift")
    if report["phase08_readiness"] or report["approval_claims"] or report["required_dependencies"] != ["03", "05", "07"]:
        raise ValueError("predesign report promotion or dependency drift")
    if report["counts"] != {"fixtures": len(corpus["fixtures"]), "consumers": len(inventory["entries"]), "matrix_rows": len(matrix["rows"]), "hostile_tuples": len(matrix["mandatory_hostile_tuples"])}:
        raise ValueError("predesign report count drift")
    if report["content_digests"] != {"corpus_sha256": corpus["corpus_sha256"], "inventory_sha256": inventory["inventory_sha256"], "matrix_sha256": matrix["matrix_sha256"], "source_occurrence_sha256": inventory["discovery_contract"]["required_match_sha256"]}:
        raise ValueError("predesign report content digest drift")
    if report["derived_source_state_digests"] != {"corpus": corpus["derived_source_state"]["derived_source_state_sha256"], "inventory": inventory["derived_source_state"]["derived_source_state_sha256"], "matrix": matrix["derived_source_state"]["derived_source_state_sha256"]}:
        raise ValueError("predesign report derived source-state digest drift")
    names = ("manifest-v1-v2-corpus.schema.json", "manifest-consumer-inventory.schema.json", "manifest-downgrade-matrix.schema.json", "manifest-v1-v2-corpus.json", "manifest-consumer-inventory.json", "manifest-downgrade-matrix.json")
    actual = {name: file_digest(root / ".agents/260822-phase08-manifest-predesign/artifacts" / name) for name in names}
    if report["artifact_sha256"] != actual:
        raise ValueError("predesign report artifact digest drift")


def validate_loaded(corpus: dict, inventory: dict, matrix: dict, root: Path, scan: bool = False, scanner: Callable[[Path], list[dict]] = scan_sources) -> None:
    validate_artifact_schemas(corpus, inventory, matrix, root)
    validate_common_base_revision(corpus, inventory, matrix)
    reject_promotional_claims(corpus, inventory, matrix)
    validate_corpus(corpus, root)
    validate_inventory(inventory, root, scan, scanner)
    validate_matrix(matrix, root)


def validate_plan_text(plan: str) -> None:
    required = ("phase_03:", "phase_05:", "phase_07:", 'completion_state: "PREDESIGN_COMPLETE / PHASE08_BLOCKED"')
    front = plan.split("---", 2)[1].lower()
    if any(value not in plan for value in required) or any(value in front for value in ("status: approved", "status: phase08_ready", "status: phase08_complete")):
        raise ValueError("dependency 03+05+07, blocked terminal, or non-promotional status missing")


def validate_parent_plan_text(plan: str) -> None:
    rows = [line for line in plan.splitlines() if line.startswith("| 08 |")]
    expected = "| 08 | [Manifest-v3 ABI C7-B](phase-08-manifest-v3-abi.md) | 4w | 03,05,07 | pending — `PREDESIGN_COMPLETE / PHASE08_BLOCKED` verified; direct dependencies are 03,05,07; no V3 code/readiness/approval |"
    if rows != [expected]:
        raise ValueError("parent Phase 08 direct dependency must be exactly 03+05+07")
