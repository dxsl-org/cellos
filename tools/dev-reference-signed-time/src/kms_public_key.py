"""Fail-closed retrieval of one manifest-pinned AWS KMS public key."""

import hashlib
import hmac
from typing import Any

from protocol_crypto import load_p256_spki

_CONFIGURATION_FAILURE = "invalid KMS public key loader configuration"
_RETRIEVAL_FAILURE = "KMS public key retrieval failed"
_P256_KEY_SPEC = "ECC_NIST_P256"
_SIGNING_ALGORITHM = "ECDSA_SHA_256"


class KmsPublicKeyError(RuntimeError):
    """Stable value-free failure at the KMS public-key boundary."""


class KmsPublicKeyLoader:
    """Retrieve and verify one exact manifest-pinned KMS P-256 public key.

    ``get_public_key`` is an injected callable with the same keyword interface as
    ``boto3.client("kms").get_public_key``. The loader performs one call and has
    no retry, fallback, client-construction, or signing surface.
    """

    __slots__ = ("_get_public_key", "_key_id", "_manifest_sha256")

    def __init__(self, get_public_key: Any, key_id: str, manifest_sha256: bytes) -> None:
        """Pin one callable, exact nonempty KMS key ID, and 32-byte SPKI digest."""
        failed = False
        try:
            if not callable(get_public_key):
                raise TypeError("KMS get-public-key callable is not callable")
            if type(key_id) is not str or not key_id:
                raise ValueError("invalid KMS key ID")
            if type(manifest_sha256) is not bytes or len(manifest_sha256) != 32:
                raise ValueError("invalid manifest public-key digest")
        except Exception:
            failed = True
        if failed:
            raise KmsPublicKeyError(_CONFIGURATION_FAILURE)
        self._get_public_key = get_public_key
        self._key_id = key_id
        self._manifest_sha256 = manifest_sha256

    def load(self) -> bytes:
        """Return the exact canonical DER-SPKI bytes or fail without provider detail."""
        failed = False
        try:
            result = self._get_public_key(KeyId=self._key_id)
            if type(result) is not dict:
                raise TypeError("KMS response is not an exact dict")
            metadata = result.get("ResponseMetadata")
            if not (
                type(metadata) is dict
                and type(metadata.get("HTTPStatusCode")) is int
                and metadata["HTTPStatusCode"] == 200
                and type(metadata.get("RequestId")) is str
                and bool(metadata["RequestId"])
            ):
                raise ValueError("KMS response metadata is not successful")
            returned_key_id = result.get("KeyId")
            if type(returned_key_id) is not str or returned_key_id != self._key_id:
                raise ValueError("KMS returned a different key")
            public_key_der = result.get("PublicKey")
            if type(public_key_der) is not bytes:
                raise TypeError("KMS public key is not exact bytes")
            key_spec_present = "KeySpec" in result
            legacy_spec_present = "CustomerMasterKeySpec" in result
            if not key_spec_present and not legacy_spec_present:
                raise ValueError("KMS response has no key specification")
            if key_spec_present and (
                type(result["KeySpec"]) is not str
                or result["KeySpec"] != _P256_KEY_SPEC
            ):
                raise ValueError("KMS returned a different key specification")
            if legacy_spec_present and (
                type(result["CustomerMasterKeySpec"]) is not str
                or result["CustomerMasterKeySpec"] != _P256_KEY_SPEC
            ):
                raise ValueError("KMS returned a different legacy key specification")
            if type(result.get("KeyUsage")) is not str or result["KeyUsage"] != "SIGN_VERIFY":
                raise ValueError("KMS returned a different key usage")
            algorithms = result.get("SigningAlgorithms")
            if not (
                type(algorithms) is list
                and all(type(algorithm) is str for algorithm in algorithms)
                and _SIGNING_ALGORITHM in algorithms
            ):
                raise ValueError("KMS returned incompatible signing algorithms")
            load_p256_spki(public_key_der)
            digest = hashlib.sha256(public_key_der).digest()
            if not hmac.compare_digest(digest, self._manifest_sha256):
                raise ValueError("KMS public key does not match the manifest")
        except Exception:
            failed = True
        if failed:
            raise KmsPublicKeyError(_RETRIEVAL_FAILURE)
        return public_key_der
