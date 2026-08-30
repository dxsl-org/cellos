"""Digest bindings to canonical authority and journal contracts."""

import hashlib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
OPERATIONS = (
    "OpenBoot",
    "ReadCommittedRelayState",
    "RequestSignedTime",
    "AcceptSignedTime",
    "BeginRelayEnrollment",
    "ReadRelayCsrChunk",
    "ValidateAndStageRelayProfile",
    "ConsumeStagedRelayProfile",
    "CommitRelayGeneration",
    "AbortRelayEnrollment",
    "GetRelayActivePublicKey",
    "SignTls13ClientCertificateVerify",
    "BeginRelayProfileUpload",
    "WriteRelayProfileChunk",
)


def _source(path: str) -> dict:
    digest = hashlib.sha256((REPO_ROOT / path).read_bytes()).hexdigest()
    return {"path": path, "sha256": digest}

def _tree(path: str) -> dict:
    root = REPO_ROOT / path
    files = [root / "Cargo.toml", *sorted((root / "src").rglob("*.rs"))]
    digest = hashlib.sha256()
    for source in files:
        relative = source.relative_to(REPO_ROOT).as_posix().encode()
        content = source.read_bytes()
        digest.update(len(relative).to_bytes(4, "big"))
        digest.update(relative)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return {
        "path": path,
        "algorithm": "sha256-path-length-content-v1",
        "file_count": len(files),
        "sha256": digest.hexdigest(),
    }


def build_contract_bindings() -> dict:
    """Bind sources that own typed operations, records, and transitions."""
    return {
        "authority_protocol": {
            "source_path": "libs/authority-protocol",
            "source_tree": _tree("libs/authority-protocol"),
            "protocol_version": 2,
            "lane": "DEV_REFERENCE",
            "operation_set": list(OPERATIONS),
            "operation_authority": _source(
                "libs/authority-protocol/src/wire/types.rs"
            ),
            "protected_record_schema": "ProtectedAuthorityRecord-v2",
            "protected_record_codec": _source(
                "libs/authority-protocol/src/state/persistence/codec/decode.rs"
            ),
            "protected_transition_authority": _source(
                "libs/authority-protocol/src/state/persistence/successor.rs"
            ),
        },
        "journal_core": {
            "source_path": "authority/stm32h573i-dk/journal-core",
            "source_tree": _tree("authority/stm32h573i-dk/journal-core"),
            "record_schema": "PERSIST-003/FullRecord-v2",
            "record_magic": "SAJR",
            "record_version": 2,
            "record_max": 1888,
            "codec_authority": _source(
                "authority/stm32h573i-dk/journal-core/src/codec/mod.rs"
            ),
            "transition_authority": _source(
                "authority/stm32h573i-dk/journal-core/src/successor.rs"
            ),
        },
    }
