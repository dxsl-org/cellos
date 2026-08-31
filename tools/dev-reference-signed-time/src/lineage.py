"""Authenticated single-head allocator lineage transitions."""

from dataclasses import dataclass
import hashlib
import re
import uuid
from typing import NoReturn

import cbor_codec
from protocol_crypto import CryptoError, parse_p256_der_signature, verify_p256_digest
from protocol_models import MAX_UINT64, SOURCE_ID

LINEAGE_HEAD_KEY = f"lineage#{SOURCE_ID}/head"
ZERO_DIGEST = b"\0" * 32
_REASONS = frozenset({"initialize", "restore", "fork", "key_rotation"})
_TABLE_NAME = re.compile(r"[A-Za-z0-9_.-]{3,255}").fullmatch
_ERROR = "invalid allocator lineage"


class LineageError(ValueError):
    """Stable rejection for malformed, unauthenticated, or regressed lineage."""


def _fail() -> NoReturn:
    raise LineageError(_ERROR) from None


@dataclass(frozen=True, slots=True)
class SignedLineageTransition:
    """One signed selection of an allocator incarnation and response key."""

    source_epoch: int
    parent_digest: bytes
    allocator_table_name: str
    allocator_table_id: str
    response_key_id: str
    response_public_key_der_sha256: bytes
    reason: str
    signature: bytes


@dataclass(frozen=True, slots=True)
class LineageContract:
    """Manifest-admitted transition plus the pinned external lineage table."""

    lineage_table_name: str
    lineage_table_id: str
    transition: SignedLineageTransition
    encoded_transition: bytes
    transition_digest: bytes


def _valid_table(name: object, table_id: object) -> bool:
    if type(name) is not str or _TABLE_NAME(name) is None or type(table_id) is not str:
        return False
    try:
        return str(uuid.UUID(table_id)) == table_id
    except (ValueError, AttributeError):
        return False


def _valid_transition(value: object) -> bool:
    return (
        type(value) is SignedLineageTransition
        and type(value.source_epoch) is int
        and 1 <= value.source_epoch <= MAX_UINT64
        and type(value.parent_digest) is bytes
        and len(value.parent_digest) == 32
        and _valid_table(value.allocator_table_name, value.allocator_table_id)
        and type(value.response_key_id) is str
        and 0 < len(value.response_key_id) <= 2048
        and type(value.response_public_key_der_sha256) is bytes
        and len(value.response_public_key_der_sha256) == 32
        and type(value.reason) is str
        and value.reason in _REASONS
        and type(value.signature) is bytes
    )


def transition_signing_bytes(value: SignedLineageTransition) -> bytes:
    """Return canonical fields signed by the lineage KMS key; reject invalid input."""
    if not _valid_transition(value):
        _fail()
    if value.source_epoch == 1:
        if value.parent_digest != ZERO_DIGEST or value.reason != "initialize":
            _fail()
    elif value.parent_digest == ZERO_DIGEST or value.reason == "initialize":
        _fail()
    return cbor_codec.dumps({
        1: 1,
        2: SOURCE_ID,
        3: value.source_epoch,
        4: value.parent_digest,
        5: value.allocator_table_name,
        6: value.allocator_table_id,
        7: value.response_key_id,
        8: value.response_public_key_der_sha256,
        9: value.reason,
    })


def encode_transition(value: SignedLineageTransition) -> bytes:
    """Encode one canonical transition with a strict low-S signature."""
    signing = transition_signing_bytes(value)
    try:
        parse_p256_der_signature(value.signature)
        decoded = cbor_codec.loads(signing)
        return cbor_codec.dumps(decoded | {10: value.signature})
    except (CryptoError, cbor_codec.CborError, TypeError, ValueError):
        _fail()


def decode_transition(encoded: bytes) -> SignedLineageTransition:
    """Decode one exact canonical transition without authenticating its signature."""
    try:
        value = cbor_codec.loads(encoded, max_size=4096)
        if type(value) is not dict or set(value) != set(range(1, 11)):
            _fail()
        if value[1] != 1 or value[2] != SOURCE_ID:
            _fail()
        result = SignedLineageTransition(
            value[3], value[4], value[5], value[6], value[7], value[8], value[9], value[10],
        )
        if not _valid_transition(result) or encode_transition(result) != encoded:
            _fail()
        return result
    except LineageError:
        raise
    except (cbor_codec.CborError, TypeError, ValueError):
        _fail()


def require_direct_child(
    previous: LineageContract,
    child: LineageContract,
) -> None:
    """Reject unless ``child`` is the complete exact direct edge from ``previous``."""
    if (
        type(previous) is not LineageContract
        or type(child) is not LineageContract
        or child.lineage_table_name != previous.lineage_table_name
        or child.lineage_table_id != previous.lineage_table_id
        or child.transition.source_epoch != previous.transition.source_epoch + 1
        or child.transition.parent_digest != previous.transition_digest
        or child.transition.response_key_id == previous.transition.response_key_id
        or child.transition.reason == "initialize"
        or (
            child.transition.reason == "key_rotation"
            and (
                child.transition.allocator_table_name
                != previous.transition.allocator_table_name
                or child.transition.allocator_table_id
                != previous.transition.allocator_table_id
            )
        )
        or (
            child.transition.reason in {"restore", "fork"}
            and (
                child.transition.allocator_table_name
                == previous.transition.allocator_table_name
                or child.transition.allocator_table_id
                == previous.transition.allocator_table_id
            )
        )
    ):
        _fail()

def admit_lineage_contract(
    lineage_table_name: str,
    lineage_table_id: str,
    encoded_transition: bytes,
    lineage_public_key_der: bytes,
    previous: LineageContract | None = None,
) -> LineageContract:
    """Authenticate a selected head; if supplied, enforce its exact direct parent."""
    try:
        if not _valid_table(lineage_table_name, lineage_table_id):
            _fail()
        transition = decode_transition(encoded_transition)
        digest = hashlib.sha256(encoded_transition).digest()
        verify_p256_digest(
            lineage_public_key_der,
            transition.signature,
            hashlib.sha256(transition_signing_bytes(transition)).digest(),
        )
        selected = LineageContract(
            lineage_table_name,
            lineage_table_id,
            transition,
            encoded_transition,
            digest,
        )
        if previous is not None:
            require_direct_child(previous, selected)
        return selected
    except LineageError:
        raise
    except (CryptoError, OverflowError, TypeError, ValueError):
        _fail()
