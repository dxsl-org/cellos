"""Pure fail-closed allocation for an already-admitted signed-time sample."""

import hashlib
from dataclasses import dataclass
from typing import Any

from protocol import ProtocolError, encode_request
from protocol_models import MAX_UINT64, SignedRequest, UnsignedResponse


class AllocationError(ValueError):
    """Stable local failure; its message is a value-free error code."""

    __slots__ = ("code",)

    def __init__(self, code: str) -> None:
        self.code = code
        super().__init__(code)


@dataclass(frozen=True, slots=True)
class AllocationState:
    """Durable allocator values read before a conditional state advance."""

    source_epoch: int
    source_sequence: int
    last_unix_seconds: int


@dataclass(frozen=True, slots=True)
class AdmittedSample:
    """Interval already admitted by a separate authenticated freshness gate."""

    sample_floor: int
    sample_ceiling: int
    sample_valid_until: int


@dataclass(frozen=True, slots=True)
class AllocationResult:
    """The state, unsigned response, and request digest committed atomically."""

    state: AllocationState
    response: UnsignedResponse
    request_digest: bytes


def _fail(code: str) -> None:
    raise AllocationError(code)


def _is_uint64(value: Any) -> bool:
    return type(value) is int and 0 <= value <= MAX_UINT64


def _require_uint64(value: Any, code: str) -> None:
    if not _is_uint64(value):
        _fail(code)


def allocate_response(
    *,
    configured_source_epoch: int,
    manifest_key_id: str,
    state: AllocationState,
    protected_server_floor: int,
    sample: AdmittedSample,
    request: SignedRequest,
) -> AllocationResult:
    """Allocate from an already-admitted interval without authenticating it.

    The caller can use ``state`` as the expected values and commit the returned
    state, response, and request digest in one conditional durable transaction.
    """
    _require_uint64(configured_source_epoch, "invalid-source-epoch")
    if type(manifest_key_id) is not str or not manifest_key_id:
        _fail("invalid-key-id")
    if type(state) is not AllocationState:
        _fail("invalid-state-type")
    _require_uint64(state.source_epoch, "invalid-state")
    _require_uint64(state.source_sequence, "invalid-state")
    _require_uint64(state.last_unix_seconds, "invalid-state")
    if state.source_epoch != configured_source_epoch:
        _fail("source-epoch-mismatch")
    _require_uint64(protected_server_floor, "invalid-protected-floor")
    if type(sample) is not AdmittedSample:
        _fail("invalid-sample-type")
    _require_uint64(sample.sample_floor, "invalid-sample")
    _require_uint64(sample.sample_ceiling, "invalid-sample")
    _require_uint64(sample.sample_valid_until, "invalid-sample")
    if type(request) is not SignedRequest:
        _fail("invalid-request")
    try:
        canonical_request = encode_request(request)
    except ProtocolError:
        _fail("invalid-request")

    if sample.sample_floor > sample.sample_ceiling:
        _fail("reversed-sample-interval")
    if not sample.sample_floor <= protected_server_floor <= sample.sample_ceiling:
        _fail("protected-floor-outside-sample")
    if state.source_sequence == MAX_UINT64:
        _fail("sequence-overflow")
    if state.last_unix_seconds == MAX_UINT64:
        _fail("time-overflow")

    unix_seconds = max(sample.sample_floor, state.last_unix_seconds + 1)
    if unix_seconds > sample.sample_ceiling:
        _fail("candidate-above-ceiling")
    if unix_seconds > MAX_UINT64 - 60:
        _fail("time-overflow")
    expires_at = min(unix_seconds + 60, sample.sample_valid_until)
    if not unix_seconds < expires_at <= unix_seconds + 60:
        _fail("invalid-expiry")

    next_state = AllocationState(
        source_epoch=configured_source_epoch,
        source_sequence=state.source_sequence + 1,
        last_unix_seconds=unix_seconds,
    )
    response = UnsignedResponse(
        source_epoch=configured_source_epoch,
        source_sequence=next_state.source_sequence,
        unix_seconds=unix_seconds,
        expires_at=expires_at,
        device_id=request.device_id,
        authority_id=request.authority_id,
        boot_epoch=request.boot_epoch,
        request_id=request.request_id,
        purpose=request.purpose,
        nonce=request.nonce,
        key_id=manifest_key_id,
    )
    return AllocationResult(next_state, response, hashlib.sha256(canonical_request).digest())
