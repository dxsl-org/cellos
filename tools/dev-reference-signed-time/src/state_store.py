"""Fail-closed transactional DynamoDB allocation and receipt recovery."""

from typing import Any

from allocation import AdmittedSample, AllocationState, allocate_response
from lineage import LineageContract
from protocol_models import SignedRequest, UnsignedResponse
from request_protocol import encode_request
from receipt import Receipt, construct_receipt
from state_codec import (
    AuthorityRegistration, encode_allocation_state, encode_receipt,
)
from state_store_recovery import (
    _read_committed_receipt, encode_active_registration, operation_succeeded,
)
from state_store_transaction import build_write_transaction

_CONFIGURATION_FAILURE = "invalid state store configuration"
_STORE_FAILURE = "state store operation failed"


class StoreError(RuntimeError):
    """Stable value-free failure at the transactional persistence boundary."""




class DynamoStateStore:
    """Commit one allocation through an injected low-level DynamoDB client."""

    __slots__ = (
        "_contract", "_transact_get_items", "_transact_write_items",
    )

    def __init__(self, client: Any, contract: LineageContract) -> None:
        failed = False
        try:
            write = client.transact_write_items
            read = client.transact_get_items
            if not callable(write) or not callable(read):
                raise TypeError("DynamoDB transaction operation is not callable")
            if type(contract) is not LineageContract:
                raise TypeError("invalid lineage contract")
        except Exception:
            failed = True
        if failed:
            raise StoreError(_CONFIGURATION_FAILURE)
        self._transact_write_items = write
        self._transact_get_items = read
        self._contract = contract

    def commit_allocation(
        self,
        registration: AuthorityRegistration,
        state: AllocationState,
        protected_server_floor: int,
        sample: AdmittedSample,
        request: SignedRequest,
    ) -> Receipt:
        """Conditionally advance state and durably bind one exact request."""
        failed = False
        try:
            registration_item = encode_active_registration(request, registration)
            prior_item = encode_allocation_state(state)
            allocation = allocate_response(
                configured_source_epoch=self._contract.transition.source_epoch,
                manifest_key_id=self._contract.transition.response_key_id,
                state=state,
                protected_server_floor=protected_server_floor,
                sample=sample,
                request=request,
            )
            receipt = construct_receipt(allocation)
            state_item = encode_allocation_state(allocation.state)
            receipt_item = encode_receipt(receipt)
            transaction = build_write_transaction(
                self._contract, registration_item, prior_item, state_item, receipt_item,
            )
        except Exception:
            failed = True
        if failed:
            raise StoreError(_STORE_FAILURE)

        ambiguous = False
        try:
            result = self._transact_write_items(TransactItems=transaction)
            ambiguous = not operation_succeeded(result)
        except Exception:
            ambiguous = True
        if ambiguous:
            return self._recover(request, registration)
        return receipt


    def recover_committed(
        self,
        request: SignedRequest,
        registration: AuthorityRegistration,
    ) -> UnsignedResponse | None:
        """Recover exact committed response labels for an active registration."""
        receipt = self._read_committed(request, registration)
        return None if receipt is None else receipt.response

    def _read_committed(
        self,
        request: SignedRequest,
        registration: AuthorityRegistration,
    ) -> Receipt | None:
        failed = False
        try:
            canonical_request = encode_request(request)
            encode_active_registration(request, registration)
            receipt = _read_committed_receipt(
                self._transact_get_items,
                self._contract,
                request,
                canonical_request,
            )
        except Exception:
            failed = True
        if failed:
            raise StoreError(_STORE_FAILURE)
        return receipt

    def _recover(
        self,
        request: SignedRequest,
        registration: AuthorityRegistration,
    ) -> Receipt:
        receipt = self._read_committed(request, registration)
        if receipt is None:
            raise StoreError(_STORE_FAILURE)
        return receipt
