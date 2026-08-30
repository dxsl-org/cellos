"""Injected receipt-first orchestration for canonical signed-time requests."""

from collections.abc import Callable
from typing import Any

from allocation import AdmittedSample, AllocationState
from protocol import encode_response
from protocol_models import MAX_UINT64, SignedResponse, UnsignedResponse
from receipt import Receipt
from request_protocol import parse_request
from state_codec import AuthorityRegistration
from state_reader import StateSnapshot

_HANDLER_FAILURE = "signed-time handler operation failed"


class HandlerError(RuntimeError):
    """Stable value-free failure at the signed-time service boundary."""


class SignedTimeHandler:
    """Compose injected state, clock-admission, persistence, and signing boundaries."""

    __slots__ = (
        "_commit_allocation",
        "_load_admitted_sample",
        "_load_protected_floor",
        "_load_snapshot",
        "_recover_committed",
        "_sign_response",
    )

    def __init__(
        self,
        state_reader: Any,
        state_store: Any,
        signer: Any,
        protected_floor_loader: Callable[[], int],
        admitted_sample_loader: Callable[[int], AdmittedSample],
    ) -> None:
        failed = False
        try:
            load_snapshot = state_reader.load_snapshot
            recover_committed = state_store.recover_committed
            commit_allocation = state_store.commit_allocation
            sign_response = signer.sign_response
            dependencies = (
                load_snapshot,
                recover_committed,
                commit_allocation,
                sign_response,
                protected_floor_loader,
                admitted_sample_loader,
            )
            if not all(callable(dependency) for dependency in dependencies):
                raise TypeError("handler dependency is not callable")
        except Exception:
            failed = True
        if failed:
            raise HandlerError(_HANDLER_FAILURE)
        self._load_snapshot = load_snapshot
        self._recover_committed = recover_committed
        self._commit_allocation = commit_allocation
        self._sign_response = sign_response
        self._load_protected_floor = protected_floor_loader
        self._load_admitted_sample = admitted_sample_loader

    def handle(self, data: bytes) -> bytes:
        """Return canonical signed-response bytes or fail without another path."""
        failed = False
        try:
            request = parse_request(data)
            snapshot = self._load_snapshot(request)
            if (
                type(snapshot) is not StateSnapshot
                or type(snapshot.registration) is not AuthorityRegistration
                or type(snapshot.state) is not AllocationState
            ):
                raise TypeError("state reader returned an invalid snapshot")
            response = self._recover_committed(request, snapshot.registration)
            if response is None:
                floor = self._load_protected_floor()
                if type(floor) is not int or not 0 <= floor <= MAX_UINT64:
                    raise TypeError("protected floor loader returned an invalid floor")
                sample = self._load_admitted_sample(floor)
                if type(sample) is not AdmittedSample:
                    raise TypeError("sample loader returned an invalid sample")
                receipt = self._commit_allocation(
                    snapshot.registration, snapshot.state, floor, sample, request
                )
                if type(receipt) is not Receipt:
                    raise TypeError("state store returned an invalid receipt")
                response = receipt.response
            if type(response) is not UnsignedResponse:
                raise TypeError("state store returned an invalid response")
            signed = self._sign_response(response)
            if type(signed) is not SignedResponse:
                raise TypeError("signer returned an invalid response")
            encoded = encode_response(signed)
        except Exception:
            failed = True
        if failed:
            raise HandlerError(_HANDLER_FAILURE)
        return encoded
