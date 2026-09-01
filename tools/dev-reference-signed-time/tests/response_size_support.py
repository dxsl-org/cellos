from dataclasses import replace
import path_bootstrap

from protocol import response_signing_bytes
from protocol_models import MAX_UNSIGNED_RESPONSE_BYTES, UnsignedResponse


def response_at_unsigned_limit(response: UnsignedResponse) -> UnsignedResponse:
    """Grow key_id so canonical labels 1..14 occupy the exact frozen limit."""
    short = replace(response, key_id="k")
    # Replacing one-byte CBOR text with >255-byte text adds key length plus one.
    key_id_length = MAX_UNSIGNED_RESPONSE_BYTES - len(response_signing_bytes(short)) - 1
    return replace(short, key_id="k" * key_id_length)
