"""Fail-closed AWS KMS adapter for signed-time response signing."""

from collections.abc import Mapping
from typing import Any

from protocol import encode_response, response_signing_digest
from protocol_crypto import (
    canonicalize_p256_der_signature, load_p256_spki, verify_p256_digest,
)
from protocol_models import SIGNING_ALGORITHM, SignedResponse, UnsignedResponse

_SIGNING_FAILURE = "KMS signing failed"
_CONFIGURATION_FAILURE = "invalid KMS signer configuration"


class KmsSignerError(RuntimeError):
    """Raised when local configuration or a KMS signing attempt fails closed."""


class KmsSigner:
    """Sign protocol responses with one injected KMS client and pinned P-256 key.

    The client must provide the same ``sign(**kwargs)`` method as a boto3 KMS
    client. This adapter never constructs a client, retries, or selects another
    key.
    """

    __slots__ = ("_key_id", "_public_key_der", "_sign")

    def __init__(self, client: Any, key_id: str, public_key_der: bytes) -> None:
        """Pin an exact KMS key ID and its canonical P-256 DER-SPKI public key."""
        if type(key_id) is not str or not key_id:
            raise KmsSignerError(_CONFIGURATION_FAILURE)
        failed = False
        try:
            sign = client.sign
            if not callable(sign):
                raise TypeError("KMS client sign attribute is not callable")
            load_p256_spki(public_key_der)
        except Exception:
            failed = True
        if failed:
            raise KmsSignerError(_CONFIGURATION_FAILURE)
        self._sign = sign
        self._key_id = key_id
        self._public_key_der = public_key_der

    def sign_response(self, response: UnsignedResponse) -> SignedResponse:
        """Return a verified, low-S signed copy of one valid unsigned response."""
        failed = False
        try:
            if type(response) is not UnsignedResponse or response.key_id != self._key_id:
                raise ValueError("unsigned response does not match the pinned key")
            digest = response_signing_digest(response)
            result = self._sign(
                KeyId=self._key_id,
                Message=digest,
                MessageType="DIGEST",
                SigningAlgorithm=SIGNING_ALGORITHM,
            )
            if not isinstance(result, Mapping):
                raise TypeError("KMS response is not a mapping")
            if type(result.get("KeyId")) is not str or result["KeyId"] != self._key_id:
                raise ValueError("KMS returned a different key")
            if (type(result.get("SigningAlgorithm")) is not str
                    or result["SigningAlgorithm"] != SIGNING_ALGORITHM):
                raise ValueError("KMS returned a different signing algorithm")
            signature = result.get("Signature")
            if type(signature) is not bytes:
                raise TypeError("KMS signature is not bytes")
            signature = canonicalize_p256_der_signature(signature)
            verify_p256_digest(self._public_key_der, signature, digest)
            signed = SignedResponse(
                source_epoch=response.source_epoch,
                source_sequence=response.source_sequence,
                unix_seconds=response.unix_seconds,
                expires_at=response.expires_at,
                device_id=response.device_id,
                authority_id=response.authority_id,
                boot_epoch=response.boot_epoch,
                request_id=response.request_id,
                purpose=response.purpose,
                nonce=response.nonce,
                key_id=response.key_id,
                signature=signature,
            )
            encode_response(signed)
            return signed
        except Exception:
            failed = True
        if failed:
            raise KmsSignerError(_SIGNING_FAILURE)
