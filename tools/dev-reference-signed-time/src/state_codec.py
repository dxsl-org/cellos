"""Strict deterministic DynamoDB AttributeValue codecs for durable state."""

from dataclasses import dataclass
from typing import Any

import cbor_codec
from allocation import AllocationState
from protocol import ProtocolError, response_signing_bytes
from protocol_crypto import CryptoError, load_ed25519_spki
from protocol_models import (
    MAX_UINT64, MAX_UNSIGNED_RESPONSE_BYTES, PROTOCOL_VERSION, SIGNING_ALGORITHM,
    SOURCE_ID, UnsignedResponse,
)
from receipt import (
    SOURCE_STATE_KEY, Receipt, authority_registration_key, request_receipt_key,
)

_SCHEMA = "1"
_REGISTRATION = "authority_registration"
_STATE = "allocation_state"
_RECEIPT = "request_receipt"


class StateCodecError(ValueError):
    """Stable value-free failure at the persistence record boundary."""

    __slots__ = ("code",)

    def __init__(self, code: str) -> None:
        self.code = code
        super().__init__(code)


@dataclass(frozen=True, slots=True)
class AuthorityRegistration:
    device_id: bytes
    authority_id: bytes
    public_key_der: bytes
    revoked: bool


def _fail(record_type: str) -> None:
    raise StateCodecError(f"invalid-{record_type.replace('_', '-')}")


def _bytes(value: Any, length: int) -> bool:
    return type(value) is bytes and len(value) == length


def _uint(value: Any) -> bool:
    return type(value) is int and 0 <= value <= MAX_UINT64


def _key_is_valid(value: Any) -> bool:
    if not _bytes(value, 44):
        return False
    valid = True
    try:
        load_ed25519_spki(value)
    except CryptoError:
        valid = False
    return valid


def _av(kind: str, value: Any) -> dict[str, Any]:
    return {kind: value}


def _read(value: Any, kind: str) -> Any:
    if type(value) is not dict or len(value) != 1:
        return None
    av_key = next(iter(value))
    if type(av_key) is not str or av_key != kind:
        return None
    result = value[av_key]
    if kind == "S":
        return result if type(result) is str else None
    if kind == "B":
        return result if type(result) is bytes else None
    if kind == "BOOL":
        return result if type(result) is bool else None
    if type(result) is not str or not result or not result.isascii():
        return None
    if result != "0" and (result[0] == "0" or not result.isdecimal()):
        return None
    if not result.isdecimal() or len(result) > 20:
        return None
    maximum = str(MAX_UINT64)
    return result if len(result) < 20 or result <= maximum else None


def _item(item: Any, fields: set[str], record_type: str) -> bool:
    return (
        type(item) is dict and all(type(key) is str for key in item)
        and set(item) == fields
        and _read(item["schema_version"], "N") == _SCHEMA
        and _read(item["record_type"], "S") == record_type
    )


def encode_authority_registration(value: AuthorityRegistration) -> dict[str, dict[str, Any]]:
    if type(value) is not AuthorityRegistration or not (
        _bytes(value.device_id, 32) and _bytes(value.authority_id, 32)
        and _key_is_valid(value.public_key_der) and type(value.revoked) is bool
    ):
        _fail(_REGISTRATION)
    return {
        "pk": _av("S", authority_registration_key(value.authority_id)),
        "schema_version": _av("N", _SCHEMA), "record_type": _av("S", _REGISTRATION),
        "device_id": _av("B", value.device_id), "authority_id": _av("B", value.authority_id),
        "public_key_der": _av("B", value.public_key_der), "revoked": _av("BOOL", value.revoked),
    }


def decode_authority_registration(item: Any) -> AuthorityRegistration:
    fields = {"pk", "schema_version", "record_type", "device_id", "authority_id", "public_key_der", "revoked"}
    if not _item(item, fields, _REGISTRATION):
        _fail(_REGISTRATION)
    value = AuthorityRegistration(
        _read(item["device_id"], "B"), _read(item["authority_id"], "B"),
        _read(item["public_key_der"], "B"), _read(item["revoked"], "BOOL"),
    )
    if not (_bytes(value.device_id, 32) and _bytes(value.authority_id, 32)
            and _key_is_valid(value.public_key_der) and type(value.revoked) is bool
            and _read(item["pk"], "S") == authority_registration_key(value.authority_id)):
        _fail(_REGISTRATION)
    return value


def encode_allocation_state(value: AllocationState) -> dict[str, dict[str, Any]]:
    if type(value) is not AllocationState or not all(_uint(field) for field in (
        value.source_epoch, value.source_sequence, value.last_unix_seconds,
    )):
        _fail(_STATE)
    return {
        "pk": _av("S", SOURCE_STATE_KEY), "schema_version": _av("N", _SCHEMA),
        "record_type": _av("S", _STATE), "source_epoch": _av("N", str(value.source_epoch)),
        "source_sequence": _av("N", str(value.source_sequence)),
        "last_unix_seconds": _av("N", str(value.last_unix_seconds)),
    }


def decode_allocation_state(item: Any) -> AllocationState:
    fields = {"pk", "schema_version", "record_type", "source_epoch", "source_sequence", "last_unix_seconds"}
    if not _item(item, fields, _STATE) or _read(item["pk"], "S") != SOURCE_STATE_KEY:
        _fail(_STATE)
    values = [_read(item[name], "N") for name in ("source_epoch", "source_sequence", "last_unix_seconds")]
    if any(value is None for value in values):
        _fail(_STATE)
    return AllocationState(*(int(value) for value in values))


def _response_wire(response: Any) -> bytes | None:
    if type(response) is not UnsignedResponse:
        return None
    try:
        return response_signing_bytes(response)
    except ProtocolError:
        return None


def encode_receipt(value: Receipt) -> dict[str, dict[str, Any]]:
    wire = _response_wire(value.response) if type(value) is Receipt else None
    if wire is None or not _bytes(value.request_digest, 32):
        _fail(_RECEIPT)
    return {
        "pk": _av("S", request_receipt_key(value.response.authority_id, value.response.request_id)),
        "schema_version": _av("N", _SCHEMA), "record_type": _av("S", _RECEIPT),
        "request_digest": _av("B", value.request_digest), "response_signing_bytes": _av("B", wire),
    }


def decode_receipt(item: Any) -> Receipt:
    fields = {"pk", "schema_version", "record_type", "request_digest", "response_signing_bytes"}
    if not _item(item, fields, _RECEIPT):
        _fail(_RECEIPT)
    digest, wire = _read(item["request_digest"], "B"), _read(item["response_signing_bytes"], "B")
    value = None
    if _bytes(digest, 32) and type(wire) is bytes and len(wire) <= MAX_UNSIGNED_RESPONSE_BYTES:
        try:
            value = cbor_codec.loads(wire, max_size=MAX_UNSIGNED_RESPONSE_BYTES)
        except cbor_codec.CborError:
            pass
    if type(value) is not dict or set(value) != set(range(1, 15)):
        _fail(_RECEIPT)
    if not (type(value[1]) is int and value[1] == PROTOCOL_VERSION
            and type(value[2]) is str and value[2] == SOURCE_ID
            and type(value[14]) is str and value[14] == SIGNING_ALGORITHM):
        _fail(_RECEIPT)
    response = UnsignedResponse(*(value[label] for label in range(3, 14)))
    canonical = _response_wire(response)
    if canonical != wire or _read(item["pk"], "S") != request_receipt_key(response.authority_id, response.request_id):
        _fail(_RECEIPT)
    return Receipt(digest, response)
