from dataclasses import replace
import hashlib

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.asymmetric.utils import Prehashed

import path_bootstrap
from lineage import (
    ZERO_DIGEST, SignedLineageTransition, admit_lineage_contract,
    encode_transition, transition_signing_bytes,
)
from protocol_crypto import canonicalize_p256_der_signature

ALLOCATOR_TABLE = "cellos-dev-signed-time-allocator"
ALLOCATOR_TABLE_ID = "11111111-2222-4333-8444-555555555555"
LINEAGE_TABLE = "cellos-dev-signed-time-lineage"
LINEAGE_TABLE_ID = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
RESPONSE_KEY_ID = (
    "arn:aws:kms:us-east-1:000000000000:key/"
    "00000000-0000-4000-8000-000000000000"
)
RESPONSE_KEY_DIGEST = bytes.fromhex("22" * 32)
LINEAGE_KEY_ID = (
    "arn:aws:kms:us-east-1:000000000000:key/"
    "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"
)

LINEAGE_PRIVATE = ec.derive_private_key(7, ec.SECP256R1())
LINEAGE_PUBLIC_DER = LINEAGE_PRIVATE.public_key().public_bytes(
    serialization.Encoding.DER,
    serialization.PublicFormat.SubjectPublicKeyInfo,
)
LINEAGE_PUBLIC_DIGEST = hashlib.sha256(LINEAGE_PUBLIC_DER).digest()


def signed_transition(
    *,
    epoch=1,
    parent_digest=ZERO_DIGEST,
    table_name=ALLOCATOR_TABLE,
    table_id=ALLOCATOR_TABLE_ID,
    response_key_id=RESPONSE_KEY_ID,
    response_key_digest=RESPONSE_KEY_DIGEST,
    reason="initialize",
):
    unsigned = SignedLineageTransition(
        epoch, parent_digest, table_name, table_id, response_key_id,
        response_key_digest, reason, b"",
    )
    digest = hashlib.sha256(transition_signing_bytes(unsigned)).digest()
    signature = LINEAGE_PRIVATE.sign(digest, ec.ECDSA(Prehashed(hashes.SHA256())))
    return replace(unsigned, signature=canonicalize_p256_der_signature(signature))


def contract(transition=None, previous=None):
    selected = signed_transition() if transition is None else transition
    return admit_lineage_contract(
        LINEAGE_TABLE,
        LINEAGE_TABLE_ID,
        encode_transition(selected),
        LINEAGE_PUBLIC_DER,
        previous,
    )
