"""Shared public-vector fixtures for transactional state-reader tests."""

import copy
import path_bootstrap  # noqa: F401

from allocation import AllocationState
from lineage_state import encode_lineage_head, lineage_head_get
from lineage_test_support import ALLOCATOR_TABLE, contract
from receipt import SOURCE_STATE_KEY, authority_registration_key
from state_codec import (
    AuthorityRegistration,
    encode_allocation_state,
    encode_authority_registration,
)
from vector_support import request_fixture

CONTRACT = contract()
TABLE = ALLOCATOR_TABLE
EPOCH = CONTRACT.transition.source_epoch
_DEFAULT = object()


def metadata(status=200, request_id="snapshot-read"):
    return {"HTTPStatusCode": status, "RequestId": request_id}


def fixture():
    request = request_fixture()[1]
    registration = AuthorityRegistration(
        request.device_id, request.authority_id, request.authority_pubkey, False,
    )
    state = AllocationState(EPOCH, 42, 1_700_000_000)
    return request, registration, state


def read_result(
    registration=None, state=None, *, responses=None, response_metadata=_DEFAULT,
):
    request, default_registration, default_state = fixture()
    del request
    if responses is None:
        responses = [
            {"Item": encode_lineage_head(CONTRACT)},
            {"Item": encode_authority_registration(registration or default_registration)},
            {"Item": encode_allocation_state(state or default_state)},
        ]
    return {
        "Responses": responses,
        "ResponseMetadata": (
            metadata() if response_metadata is _DEFAULT else response_metadata
        ),
    }


def expected_transaction(request):
    return {"TransactItems": [
        lineage_head_get(CONTRACT),
        {"Get": {
            "TableName": TABLE,
            "Key": {"pk": {"S": authority_registration_key(request.authority_id)}},
        }},
        {"Get": {
            "TableName": TABLE,
            "Key": {"pk": {"S": SOURCE_STATE_KEY}},
        }},
    ]}


class FakeClient:
    def __init__(self, result=_DEFAULT, error=None):
        self.result = read_result() if result is _DEFAULT else result
        self.error = error
        self.calls = []

    def transact_get_items(self, **kwargs):
        self.calls.append(copy.deepcopy(kwargs))
        if self.error is not None:
            raise self.error
        return self.result
