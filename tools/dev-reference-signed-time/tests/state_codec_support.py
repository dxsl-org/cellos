import copy
import path_bootstrap

from allocation import AllocationState
from protocol import response_signing_bytes
from protocol_crypto import ED25519_SPKI_PREFIX
from protocol_models import UnsignedResponse
from receipt import Receipt
from state_codec import AuthorityRegistration


class BytesChild(bytes):
    pass


class IntChild(int):
    pass


class StrChild(str):
    pass


class DictChild(dict):
    pass


class RegistrationChild(AuthorityRegistration):
    pass


class StateChild(AllocationState):
    pass


class ReceiptChild(Receipt):
    pass


def registration():
    return AuthorityRegistration(b"d" * 32, b"a" * 32, ED25519_SPKI_PREFIX + b"k" * 32, False)


def state():
    return AllocationState(7, 42, 1_700_000_000)


def response():
    return UnsignedResponse(
        7, 42, 1_700_000_000, 1_700_000_060, b"d" * 32, b"a" * 32,
        9, b"r" * 16, 2, b"n" * 32, "manifest-key",
    )




def receipt():
    return Receipt(b"h" * 32, response())


def replace_av(item, field, value):
    changed = copy.deepcopy(item)
    changed[field] = value
    return changed


def wrong_value(av):
    kind = next(iter(av))
    return {
        "S": {"S": b"not-text"}, "B": {"B": bytearray(b"not-bytes")},
        "N": {"N": 1}, "BOOL": {"BOOL": 0},
    }[kind]


def malformed_avs(av):
    kind = next(iter(av))
    other = "B" if kind != "B" else "S"
    return ({}, {kind: av[kind], other: b"extra"}, wrong_value(av))


def response_wire():
    return response_signing_bytes(response())
