import copy
import path_bootstrap
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519

from allocation import AdmittedSample, AllocationState, allocate_response
from receipt import construct_receipt
from request_protocol import request_signing_bytes
from protocol_models import SignedRequest, UnsignedRequest
from state_codec import (
    AuthorityRegistration, encode_allocation_state, encode_authority_registration,
    encode_receipt,
)
from vector_support import request_fixture

TABLE = "signed-time-state"
EPOCH = 7
KEY_ID = "manifest-key"
def response_metadata(request_id="test-request", status=200):
    return {
        "HTTPStatusCode": status,
        "RequestId": request_id,
    }


def write_success(request_id="test-request"):
    return {"ResponseMetadata": response_metadata(request_id)}



def request_signer():
    private_key = ed25519.Ed25519PrivateKey.generate()
    public_key = private_key.public_key().public_bytes(
        serialization.Encoding.DER,
        serialization.PublicFormat.SubjectPublicKeyInfo,
    )

    def make(*, request_id=b"r" * 16, nonce=b"n" * 32):
        request = UnsignedRequest(
            b"d" * 32, b"a" * 32, 9, request_id, 2, nonce, public_key,
        )
        return SignedRequest(
            request.device_id, request.authority_id, request.boot_epoch,
            request.request_id, request.purpose, request.nonce,
            request.authority_pubkey, private_key.sign(request_signing_bytes(request)),
        )

    return make



class FakeClient:
    def __init__(self, *, write_result=None, write_error=None, read_result=None, read_error=None):
        self.write_result = write_success() if write_result is None else write_result
        self.write_error = write_error
        self.read_result = read_result
        self.read_error = read_error
        self.calls = []

    def transact_write_items(self, **kwargs):
        self.calls.append(("write", copy.deepcopy(kwargs)))
        if self.write_error is not None:
            raise self.write_error
        return self.write_result

    def transact_get_items(self, **kwargs):
        self.calls.append(("read", copy.deepcopy(kwargs)))
        if self.read_error is not None:
            raise self.read_error
        return self.read_result


def fixture(*, state=None, sample=None, floor=None, request=None):
    request = request or request_fixture()[1]
    registration = AuthorityRegistration(
        request.device_id, request.authority_id, request.authority_pubkey, False,
    )
    state = state or AllocationState(EPOCH, 42, 1_700_000_000)
    sample = sample or AdmittedSample(1_700_000_001, 1_700_000_030, 1_700_000_061)
    floor = sample.sample_floor if floor is None else floor
    allocation = allocate_response(
        configured_source_epoch=EPOCH,
        manifest_key_id=KEY_ID,
        state=state,
        protected_server_floor=floor,
        sample=sample,
        request=request,
    )
    return registration, state, floor, sample, request, construct_receipt(allocation)


def receipt_read(receipt):
    return {
        "Responses": [{"Item": encode_receipt(receipt)}],
        "ResponseMetadata": response_metadata("receipt-read"),
    }


def expected_read(request):
    key = f"request#{request.authority_id.hex()}/{request.request_id.hex()}"
    return {"TransactItems": [{"Get": {
        "TableName": TABLE, "Key": {"pk": {"S": key}},
    }}]}


def expected_write(registration, prior_state, receipt):
    registration_item = encode_authority_registration(registration)
    prior = encode_allocation_state(prior_state)
    state = encode_allocation_state(AllocationState(
        receipt.response.source_epoch,
        receipt.response.source_sequence,
        receipt.response.unix_seconds,
    ))
    reg_names = {
        "#pk": "pk", "#sv": "schema_version", "#rt": "record_type",
        "#di": "device_id", "#ai": "authority_id", "#key": "public_key_der",
        "#revoked": "revoked",
    }
    reg_values = {
        ":pk": registration_item["pk"], ":sv": registration_item["schema_version"],
        ":rt": registration_item["record_type"], ":di": registration_item["device_id"],
        ":ai": registration_item["authority_id"],
        ":key": registration_item["public_key_der"], ":revoked": {"BOOL": False},
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
    return {"TransactItems": [
        {"ConditionCheck": {
            "TableName": TABLE, "Key": {"pk": registration_item["pk"]},
            "ConditionExpression": (
                "#pk = :pk AND #sv = :sv AND #rt = :rt AND #di = :di AND "
                "#ai = :ai AND #key = :key AND #revoked = :revoked"
            ),
            "ExpressionAttributeNames": reg_names,
            "ExpressionAttributeValues": reg_values,
        }},
        {"Put": {
            "TableName": TABLE, "Item": state,
            "ConditionExpression": (
                "#pk = :pk AND #sv = :sv AND #rt = :rt AND #epoch = :epoch AND "
                "#sequence = :sequence AND #time = :time"
            ),
            "ExpressionAttributeNames": state_names,
            "ExpressionAttributeValues": state_values,
        }},
        {"Put": {
            "TableName": TABLE, "Item": encode_receipt(receipt),
            "ConditionExpression": "attribute_not_exists(#pk)",
            "ExpressionAttributeNames": {"#pk": "pk"},
        }},
    ]}
