"""Strict canonical DEV_REFERENCE signed-time manifest schema."""

from dataclasses import dataclass, fields
import json
import re
from typing import Any, NoReturn

from clock_policy import ClockPolicy
from protocol_models import MAX_UINT64, PROTOCOL_VERSION, SIGNING_ALGORITHM, SOURCE_ID
import manifest_validation as validation

MAX_MANIFEST_BYTES = 4096
SCHEMA_VERSION = 1
CLASSIFICATION = "DEV_REFERENCE"
PRODUCTION_REJECTION_MARKERS = frozenset({
    "AWS_DEV_SIGNED_TIME",
    "DEV_REFERENCE",
    "SOFTWARE_HARNESS",
    "aws-dev-signed-time",
    "cellos-dev-time-v1",
})
_ERROR = "invalid signed-time manifest"
_HEX_32 = re.compile(r"[0-9a-f]{64}").fullmatch
class ManifestError(ValueError):
    """Stable value-free rejection for every malformed manifest."""
    __slots__ = ()
class _DecodeFailure(Exception):
    pass
def _fail() -> NoReturn:
    raise ManifestError(_ERROR) from None
@dataclass(frozen=True, slots=True)
class SignedTimeManifest:
    schema_version: int
    classification: str
    protocol_version: int
    source_id: str
    aws_region: str
    endpoint_url: str
    endpoint_spki_sha256: bytes
    source_epoch: int
    kms_key_id: str
    kms_public_key_der_sha256: bytes
    signing_algorithm: str
    upstream_identity: str
    max_sample_age_seconds: int
    max_uncertainty_seconds: int
_FIELD_NAMES = frozenset(field.name for field in fields(SignedTimeManifest))
_UINT_FIELDS = ("source_epoch", "max_sample_age_seconds", "max_uncertainty_seconds")
_CONSTANTS = {
    "schema_version": SCHEMA_VERSION, "classification": CLASSIFICATION,
    "protocol_version": PROTOCOL_VERSION, "source_id": SOURCE_ID,
    "signing_algorithm": SIGNING_ALGORITHM,
}
_DIGEST_FIELDS = ("endpoint_spki_sha256", "kms_public_key_der_sha256")
def _validate_manifest(manifest: Any) -> None:
    if type(manifest) is not SignedTimeManifest:
        _fail()
    for name, expected in _CONSTANTS.items():
        value = getattr(manifest, name)
        if type(value) is not type(expected) or value != expected:
            _fail()
    for name in _UINT_FIELDS:
        value = getattr(manifest, name)
        if type(value) is not int or not 0 <= value <= MAX_UINT64:
            _fail()
    for name in _DIGEST_FIELDS:
        value = getattr(manifest, name)
        if type(value) is not bytes or len(value) != 32:
            _fail()
    if not validation.bounded_strings_are_valid(
        manifest.aws_region,
        manifest.endpoint_url,
        manifest.kms_key_id,
        manifest.upstream_identity,
    ):
        _fail()
    if not validation.endpoint_is_valid(manifest.endpoint_url):
        _fail()
    if not validation.kms_arn_is_valid(manifest.kms_key_id, manifest.aws_region):
        _fail()
def _as_json_object(manifest: SignedTimeManifest) -> dict[str, Any]:
    result = {field.name: getattr(manifest, field.name) for field in fields(manifest)}
    for name in _DIGEST_FIELDS:
        result[name] = result[name].hex()
    return result
def encode_manifest(manifest: SignedTimeManifest) -> bytes:
    """Encode one validated manifest as its sole canonical JSON representation."""
    _validate_manifest(manifest)
    encoded = json.dumps(
        _as_json_object(manifest), sort_keys=True, separators=(",", ":"),
        ensure_ascii=True, allow_nan=False,
    ).encode("ascii")
    if len(encoded) > MAX_MANIFEST_BYTES:
        _fail()
    return encoded
def _object_from_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise _DecodeFailure
        result[key] = value
    return result
def _reject_constant(_: str) -> NoReturn:
    raise _DecodeFailure
def _from_json_object(value: Any) -> SignedTimeManifest:
    if type(value) is not dict or frozenset(value) != _FIELD_NAMES:
        _fail()
    converted = dict(value)
    for name in _DIGEST_FIELDS:
        digest = converted[name]
        if type(digest) is not str or _HEX_32(digest) is None:
            _fail()
        converted[name] = bytes.fromhex(digest)
    return SignedTimeManifest(**converted)
def decode_manifest(data: bytes) -> SignedTimeManifest:
    """Decode only bounded UTF-8 bytes already in exact canonical JSON form."""
    if type(data) is not bytes or len(data) > MAX_MANIFEST_BYTES:
        _fail()
    failed = False
    try:
        text = data.decode("utf-8", errors="strict")
        value = json.loads(
            text, object_pairs_hook=_object_from_pairs,
            parse_constant=_reject_constant,
        )
    except (UnicodeError, json.JSONDecodeError, _DecodeFailure, RecursionError):
        failed = True
        value = None
    if failed:
        _fail()
    manifest = _from_json_object(value)
    _validate_manifest(manifest)
    if encode_manifest(manifest) != data:
        _fail()
    return manifest
def derive_clock_policy(manifest: SignedTimeManifest) -> ClockPolicy:
    """Derive the existing clock boundary without ambient inputs or I/O."""
    _validate_manifest(manifest)
    return ClockPolicy(
        manifest.upstream_identity, manifest.source_epoch,
        manifest.max_sample_age_seconds, manifest.max_uncertainty_seconds,
    )
def derive_kms_key_pins(manifest: SignedTimeManifest) -> tuple[str, bytes]:
    """Return the exact KMS key ARN and DER-SPKI SHA-256 pin."""
    _validate_manifest(manifest)
    return manifest.kms_key_id, manifest.kms_public_key_der_sha256
