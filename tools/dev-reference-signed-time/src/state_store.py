"""Fail-closed transactional DynamoDB allocation and receipt recovery."""

from typing import Any

from allocation import AdmittedSample, AllocationState, allocate_response
from protocol_models import MAX_UINT64, SignedRequest, UnsignedResponse
from request_protocol import encode_request
from receipt import Receipt, construct_receipt
from state_codec import (
    AuthorityRegistration, encode_allocation_state, encode_receipt,
)
from state_store_recovery import (
    _read_committed_receipt, encode_active_registration, operation_succeeded,
)

_CONFIGURATION_FAILURE = "invalid state store configuration"
_STORE_FAILURE = "state store operation failed"


class StoreError(RuntimeError):
    """Stable value-free failure at the transactional persistence boundary."""




class DynamoStateStore:
    """Commit one allocation through an injected low-level DynamoDB client."""

    __slots__ = (
        "_configured_source_epoch", "_manifest_key_id", "_table_name",
        "_transact_get_items", "_transact_write_items",
    )

    def __init__(
        self,
        client: Any,
        table_name: str,
        configured_source_epoch: int,
        manifest_key_id: str,
    ) -> None:
        failed = False
        try:
            write = client.transact_write_items
            read = client.transact_get_items
            if not callable(write) or not callable(read):
                raise TypeError("DynamoDB transaction operation is not callable")
            if type(table_name) is not str or not table_name:
                raise ValueError("invalid table name")
            if not (
                type(configured_source_epoch) is int
                and 0 <= configured_source_epoch <= MAX_UINT64
            ):
                raise ValueError("invalid source epoch")
            if type(manifest_key_id) is not str or not manifest_key_id:
                raise ValueError("invalid manifest key ID")
        except Exception:
            failed = True
        if failed:
            raise StoreError(_CONFIGURATION_FAILURE)
        self._transact_write_items = write
        self._transact_get_items = read
        self._table_name = table_name
        self._configured_source_epoch = configured_source_epoch
        self._manifest_key_id = manifest_key_id

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
                configured_source_epoch=self._configured_source_epoch,
                manifest_key_id=self._manifest_key_id,
                state=state,
                protected_server_floor=protected_server_floor,
                sample=sample,
                request=request,
            )
            receipt = construct_receipt(allocation)
            state_item = encode_allocation_state(allocation.state)
            receipt_item = encode_receipt(receipt)
            transaction = self._write_transaction(
                registration_item, prior_item, state_item, receipt_item,
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

    def _write_transaction(self, registration, prior, state, receipt):
        registration_names = {
            "#pk": "pk", "#sv": "schema_version", "#rt": "record_type",
            "#di": "device_id", "#ai": "authority_id", "#key": "public_key_der",
            "#revoked": "revoked",
        }
        registration_values = {
            ":pk": registration["pk"], ":sv": registration["schema_version"],
            ":rt": registration["record_type"], ":di": registration["device_id"],
            ":ai": registration["authority_id"], ":key": registration["public_key_der"],
            ":revoked": {"BOOL": False},
        }
        state_names = {
            "#pk": "pk", "#sv": "schema_version", "#rt": "record_type",
            "#epoch": "source_epoch", "#sequence": "source_sequence",
            "#time": "last_unix_seconds",
        }
        state_values = {
            ":pk": prior["pk"], ":sv": prior["schema_version"],
            ":rt": prior["record_type"], ":epoch": prior["source_epoch"],
            ":sequence": prior["source_sequence"], ":time": prior["last_unix_seconds"],
        }
        return [
            {"ConditionCheck": {
                "TableName": self._table_name, "Key": {"pk": registration["pk"]},
                "ConditionExpression": " AND ".join(
                    f"{name} = :{name[1:]}" for name in registration_names
                ),
                "ExpressionAttributeNames": registration_names,
                "ExpressionAttributeValues": registration_values,
            }},
            {"Put": {
                "TableName": self._table_name, "Item": state,
                "ConditionExpression": " AND ".join(
                    f"{name} = :{name[1:]}" for name in state_names
                ),
                "ExpressionAttributeNames": state_names,
                "ExpressionAttributeValues": state_values,
            }},
            {"Put": {
                "TableName": self._table_name, "Item": receipt,
                "ConditionExpression": "attribute_not_exists(#pk)",
                "ExpressionAttributeNames": {"#pk": "pk"},
            }},
        ]

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
                self._table_name,
                request,
                canonical_request,
                self._configured_source_epoch,
                self._manifest_key_id,
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
