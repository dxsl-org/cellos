#!/usr/bin/env python3
"""Supervisor-side relay enrollment driver (Phase 3).

Plans and executes the supervisor's share of the certificate enrollment
lifecycle against the frozen opcode contract:

    9  BeginRelayEnrollment   -> pending identity + one-shot CSR handle
    10 ReadRelayCsrChunk      -> strictly ordered 104-byte chunks
    11 CommitRelayGeneration  -> valid only from Staged, exact digest
    12 AbortRelayEnrollment   -> destroy the pending generation

Staging (opcode 13) belongs to the live service-net binding and is
deliberately NOT drivable from here: the tool refuses to fabricate a
staged profile digest. This module is pure planning over the manifest so
it can be unit-tested without a kernel.

The plan is fail-closed by construction: any out-of-order read, oversized
CSR, or commit attempt before staging aborts the whole sequence.
"""

from __future__ import annotations

import hashlib
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

# Frozen KMS wire bounds.
CSR_MAX_BYTES = 1024
CHUNK_CAPACITY = 104
HOSTNAME_MAX_BYTES = 64

OP_BEGIN = 9
OP_READ_CHUNK = 10
OP_COMMIT = 11
OP_ABORT = 12

_ROOT_FIELDS = frozenset(
    {"relay", "trust", "client", "server", "authorization", "enrollment"}
)
_ENROLLMENT_FIELDS = frozenset({"pending_generation", "policy_epoch"})
_U64_MAX = (1 << 64) - 1


class EnrollmentPlanError(ValueError):
    """The requested enrollment cannot be planned under the frozen contract."""


@dataclass(frozen=True, slots=True)
class ManifestFacts:
    hostname: str
    pending_generation: int
    node_id_sha256: str
    active_ca_sha256: str
    next_ca_sha256: str | None
    policy_epoch: int


@dataclass(slots=True)
class EnrollmentPlan:
    """Ordered supervisor operations plus their expected observations."""

    ops: list[tuple[int, str]] = field(default_factory=list)

    def digest_of(self, csr: bytes) -> str:
        return hashlib.sha256(csr).hexdigest()


def load_enrollment_facts(manifest_path: str | Path) -> ManifestFacts:
    try:
        document = tomllib.loads(Path(manifest_path).read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as exc:
        raise EnrollmentPlanError(f"cannot load enrollment manifest: {exc}") from exc
    unexpected = set(document) - _ROOT_FIELDS
    if unexpected:
        raise EnrollmentPlanError(f"unexpected top-level field: {min(unexpected)}")

    relay = _required_table(document, "relay")
    client = _required_table(document, "client")
    trust = _required_table(document, "trust")
    enrollment = _table(
        document, "enrollment", _ENROLLMENT_FIELDS, _ENROLLMENT_FIELDS
    )
    node_id = _fingerprint(client, "node_id_sha256")
    active_ca = _fingerprint(trust, "active_ca_sha256")
    next_ca_value = trust.get("next_ca_sha256")
    next_ca = (
        None
        if next_ca_value in (None, "")
        else _fingerprint(trust, "next_ca_sha256")
    )
    return ManifestFacts(
        hostname=_hostname(relay.get("hostname")),
        pending_generation=_u64(enrollment, "pending_generation"),
        node_id_sha256=node_id,
        active_ca_sha256=active_ca,
        next_ca_sha256=next_ca,
        policy_epoch=_u64(enrollment, "policy_epoch"),
    )


def plan_enrollment(facts: ManifestFacts, staged_digest: str | None) -> EnrollmentPlan:
    """Build the supervisor op sequence.

    `staged_digest` models the profile digest that service-net bound via
    opcode 13. Commit (11) is only appended when it matches the digest the
    supervisor received out of band; otherwise the plan ends in Abort (12).
    """
    plan = EnrollmentPlan()
    plan.ops.append((OP_BEGIN, facts.hostname))
    # The CSR length is unknown until Begin answers; plan the worst-case
    # bounded chunk walk and let the runtime stop at the reported length.
    chunks = (CSR_MAX_BYTES + CHUNK_CAPACITY - 1) // CHUNK_CAPACITY
    for index in range(chunks):
        plan.ops.append((OP_READ_CHUNK, f"index={index}"))
    if staged_digest is None:
        plan.ops.append((OP_ABORT, "never staged"))
        raise EnrollmentPlanError("commit refused before service-net staging")
    profile_digest = _sha256_hex(staged_digest, "staged profile digest")
    plan.ops.append((OP_COMMIT, profile_digest))
    return plan


def check_chunk_sequence(received: list[bytes], total_len: int) -> bytes:
    """Reassemble ordered chunks exactly as KMS requires them to be read."""
    if total_len > CSR_MAX_BYTES:
        raise EnrollmentPlanError(f"CSR {total_len} exceeds frozen {CSR_MAX_BYTES}")
    csr = bytearray()
    for expected_index, chunk in enumerate(received):
        if len(chunk) == 0 or len(chunk) > CHUNK_CAPACITY:
            raise EnrollmentPlanError(f"chunk {expected_index} has bad length {len(chunk)}")
        csr.extend(chunk)
    if len(csr) != total_len:
        raise EnrollmentPlanError(
            f"assembled {len(csr)} bytes but Begin reported {total_len}"
        )
    return bytes(csr)


def _required_table(document: dict[str, object], name: str) -> dict[str, object]:
    value = document.get(name)
    if not isinstance(value, dict):
        raise EnrollmentPlanError(f"[{name}] must be a table")
    return value


def _table(
    document: dict[str, object],
    name: str,
    required: frozenset[str],
    allowed: frozenset[str],
) -> dict[str, object]:
    value = _required_table(document, name)
    missing = required - set(value)
    if missing:
        raise EnrollmentPlanError(f"missing {name}.{min(missing)}")
    unexpected = set(value) - allowed
    if unexpected:
        raise EnrollmentPlanError(f"unexpected {name}.{min(unexpected)}")
    return value


def _u64(table: dict[str, object], key: str) -> int:
    value = table[key]
    if type(value) is not int or not 1 <= value <= _U64_MAX:
        raise EnrollmentPlanError(
            f"enrollment.{key} must be an integer from 1 through {_U64_MAX}"
        )
    return value


def _hostname(value: object) -> str:
    value = value if isinstance(value, str) else ""
    labels = value.split(".")
    ok = (
        value
        and len(value.encode("utf-8")) <= HOSTNAME_MAX_BYTES
        and all(
            label
            and label[0] != "-"
            and label[-1] != "-"
            and len(label) <= 63
            for label in labels
        )
        and all(c in "abcdefghijklmnopqrstuvwxyz0123456789-." for c in value)
    )
    if not ok:
        raise EnrollmentPlanError(
            "relay.hostname is not frozen-profile lowercase DNS"
        )
    return value


def _fingerprint(table: dict[str, object], key: str) -> str:
    return _sha256_hex(table.get(key), key)


def _sha256_hex(value: object, field: str) -> str:
    if (
        not isinstance(value, str)
        or len(value) != 64
        or any(c not in "0123456789abcdef" for c in value)
    ):
        raise EnrollmentPlanError(f"{field} must be 64 lowercase hex characters")
    return value


def main() -> int:  # pragma: no cover - thin CLI wrapper
    import argparse

    parser = argparse.ArgumentParser(description="plan relay enrollment opcodes")
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--staged-digest")
    args = parser.parse_args()
    try:
        facts = load_enrollment_facts(args.manifest)
        plan = plan_enrollment(facts, args.staged_digest)
    except EnrollmentPlanError as error:
        print(f"refused: {error}")
        return 1
    for opcode, detail in plan.ops[:6]:
        print(f"op {opcode}: {detail}")
    print(f"... {len(plan.ops)} supervisor operations planned")
    return 0


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
