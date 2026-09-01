"""Immutable Cloudflare Roughtime provider constants for draft-11."""

from base64 import b64decode
from dataclasses import dataclass
from typing import Any, NoReturn

PROVIDER_PROTOCOL = "roughtime-draft-11"
PROVIDER_TRANSPORT = "udp"
PROVIDER_HOST = "roughtime.cloudflare.com"
PROVIDER_PORT = 2003
PROVIDER_PUBLIC_KEY_BASE64 = "0GD7c3yP8xEc4Zl2zeuN2SlLvDVVocjsPSL8/Rl/7zg="
PROVIDER_PUBLIC_KEY = b64decode(PROVIDER_PUBLIC_KEY_BASE64, validate=True)
PROVIDER_VERSION = 0x8000000B
PROVIDER_TIMEOUT_MILLISECONDS = 2000
REQUEST_MESSAGE_BYTES = 1012
MAX_PACKET_BYTES = 1024
MAX_MESSAGE_PAIRS = 32
MAX_MERKLE_PATH_NODES = 32
_ERROR = "invalid roughtime provider configuration"


class RoughtimeConfigError(ValueError):
    __slots__ = ()


def _fail() -> NoReturn:
    raise RoughtimeConfigError(_ERROR) from None


@dataclass(frozen=True, slots=True)
class RoughtimeProviderConfig:
    protocol: str
    transport: str
    host: str
    port: int
    public_key: bytes
    version: int
    timeout_milliseconds: int
    request_message_bytes: int
    max_packet_bytes: int


_EXPECTED = RoughtimeProviderConfig(
    PROVIDER_PROTOCOL,
    PROVIDER_TRANSPORT,
    PROVIDER_HOST,
    PROVIDER_PORT,
    PROVIDER_PUBLIC_KEY,
    PROVIDER_VERSION,
    PROVIDER_TIMEOUT_MILLISECONDS,
    REQUEST_MESSAGE_BYTES,
    MAX_PACKET_BYTES,
)


def provider_config() -> RoughtimeProviderConfig:
    """Return the sole runtime provider configuration."""
    return _EXPECTED


def validate_provider_config(value: Any) -> None:
    """Reject subclasses, equality tricks, and every alternate provider value."""
    if type(value) is not RoughtimeProviderConfig:
        _fail()
    for field in value.__dataclass_fields__:
        actual = getattr(value, field)
        expected = getattr(_EXPECTED, field)
        if type(actual) is not type(expected) or actual != expected:
            _fail()
