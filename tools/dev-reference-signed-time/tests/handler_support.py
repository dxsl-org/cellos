import path_bootstrap  # noqa: F401

from protocol_models import SignedResponse
from state_reader import StateSnapshot
from state_store_support import fixture
from vector_support import request_fixture

FAILURE = "signed-time handler operation failed"
SIGNATURE = b"\x30\x06\x02\x01\x01\x02\x01\x01"


def signed_copy(response):
    return SignedResponse(
        response.source_epoch,
        response.source_sequence,
        response.unix_seconds,
        response.expires_at,
        response.device_id,
        response.authority_id,
        response.boot_epoch,
        response.request_id,
        response.purpose,
        response.nonce,
        response.key_id,
        SIGNATURE,
    )


def handler_fixture():
    vector, request, _ = request_fixture()
    registration, state, floor, sample, _, receipt = fixture(request=request)
    data = bytes.fromhex(vector["canonical_cbor_hex"])
    return data, request, StateSnapshot(registration, state), floor, sample, receipt


class Reader:
    def __init__(self, calls, snapshot, error=None):
        self.calls = calls
        self.snapshot = snapshot
        self.error = error
        self.count = 0

    def load_snapshot(self, request):
        self.count += 1
        self.calls.append(("snapshot", request))
        if self.error is not None:
            raise self.error
        return self.snapshot


class Store:
    def __init__(self, calls, recovered=None, receipt=None, recover_error=None, commit_error=None):
        self.calls = calls
        self.recovered = recovered
        self.receipt = receipt
        self.recover_error = recover_error
        self.commit_error = commit_error
        self.recover_count = 0
        self.commit_count = 0

    def recover_committed(self, request, registration):
        self.recover_count += 1
        self.calls.append(("recover", request, registration))
        if self.recover_error is not None:
            raise self.recover_error
        return self.recovered

    def commit_allocation(self, registration, state, floor, sample, request):
        self.commit_count += 1
        self.calls.append(("commit", registration, state, floor, sample, request))
        if self.commit_error is not None:
            raise self.commit_error
        return self.receipt


class Signer:
    def __init__(self, calls, error=None, result=None):
        self.calls = calls
        self.error = error
        self.result = result
        self.count = 0
        self.responses = []

    def sign_response(self, response):
        self.count += 1
        self.responses.append(response)
        self.calls.append(("sign", response))
        if self.error is not None:
            raise self.error
        return signed_copy(response) if self.result is None else self.result


class Loaders:
    def __init__(self, calls, floor, sample, floor_error=None, sample_error=None):
        self.calls = calls
        self.floor = floor
        self.sample = sample
        self.floor_error = floor_error
        self.sample_error = sample_error
        self.floor_count = 0
        self.sample_count = 0
        self.sample_floors = []

    def load_floor(self):
        self.floor_count += 1
        self.calls.append(("floor",))
        if self.floor_error is not None:
            raise self.floor_error
        return self.floor

    def load_sample(self, floor):
        self.sample_count += 1
        self.sample_floors.append(floor)
        self.calls.append(("sample", floor))
        if self.sample_error is not None:
            raise self.sample_error
        return self.sample


def dependencies(*, recovered=None, reader_error=None, recover_error=None,
                 floor_error=None, sample_error=None, commit_error=None, sign_error=None):
    data, request, snapshot, floor, sample, receipt = handler_fixture()
    calls = []
    reader = Reader(calls, snapshot, reader_error)
    store = Store(calls, recovered, receipt, recover_error, commit_error)
    signer = Signer(calls, sign_error)
    loaders = Loaders(calls, floor, sample, floor_error, sample_error)
    return data, request, snapshot, receipt, calls, reader, store, signer, loaders
