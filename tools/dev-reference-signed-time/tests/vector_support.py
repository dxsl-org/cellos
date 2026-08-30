"""Public-vector loaders shared by the protocol unit tests."""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))

from protocol_models import RegisteredAuthority, SignedRequest, SignedResponse, UnsignedRequest


def vector(name: str) -> dict:
    with (ROOT / "vectors" / name).open("r", encoding="utf-8") as source:
        return json.load(source)


def request_fixture() -> tuple[dict, SignedRequest, RegisteredAuthority]:
    data = vector("request-v1.json")
    claims = data["claims"]
    request = SignedRequest(
        bytes.fromhex(claims["device_id_hex"]),
        bytes.fromhex(claims["authority_id_hex"]),
        claims["boot_epoch"],
        bytes.fromhex(claims["request_id_hex"]),
        claims["purpose"],
        bytes.fromhex(claims["nonce_hex"]),
        bytes.fromhex(claims["authority_pubkey_der_hex"]),
        bytes.fromhex(data["signature_hex"]),
    )
    registration = RegisteredAuthority(request.device_id, request.authority_id, request.authority_pubkey)
    return data, request, registration


def response_fixture() -> tuple[dict, SignedResponse]:
    data = vector("response-v1.json")
    claims = data["claims"]
    response = SignedResponse(
        claims["source_epoch"], claims["source_sequence"], claims["unix_seconds"],
        claims["expires_at"], bytes.fromhex(claims["device_id_hex"]),
        bytes.fromhex(claims["authority_id_hex"]), claims["boot_epoch"],
        bytes.fromhex(claims["request_id_hex"]), claims["purpose"],
        bytes.fromhex(claims["nonce_hex"]), claims["key_id"],
        bytes.fromhex(data["signature_der_hex"]),
    )
    return data, response


def unsigned_request(request: SignedRequest) -> UnsignedRequest:
    return UnsignedRequest(
        request.device_id, request.authority_id, request.boot_epoch, request.request_id,
        request.purpose, request.nonce, request.authority_pubkey,
    )
