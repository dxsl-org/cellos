"""Strict RFC 8949 deterministic CBOR subset.

The public ``dumps`` and ``loads`` APIs support unsigned 64-bit integers,
byte strings, UTF-8 text strings, and maps with scalar keys. All other CBOR
features are rejected. Decoding also rejects every non-deterministic encoding.
"""

from collections.abc import Mapping
from typing import Any

MAX_UINT64 = (1 << 64) - 1


class CborError(ValueError):
    """Raised when a value or byte stream is outside the supported subset."""


def _head(major: int, value: int) -> bytes:
    if type(value) is not int or not 0 <= value <= MAX_UINT64:
        raise CborError("integer or length is outside uint64")
    prefix = major << 5
    if value < 24:
        return bytes((prefix | value,))
    if value <= 0xFF:
        return bytes((prefix | 24, value))
    if value <= 0xFFFF:
        return bytes((prefix | 25,)) + value.to_bytes(2, "big")
    if value <= 0xFFFFFFFF:
        return bytes((prefix | 26,)) + value.to_bytes(4, "big")
    return bytes((prefix | 27,)) + value.to_bytes(8, "big")


def dumps(value: Any) -> bytes:
    """Encode *value* in the supported deterministic CBOR subset."""
    return _encode(value, 0)


def _encode(value: Any, depth: int) -> bytes:
    if depth > 32:
        raise CborError("maximum nesting depth exceeded")
    if type(value) is int:
        return _head(0, value)
    if type(value) is bytes:
        return _head(2, len(value)) + value
    if type(value) is str:
        try:
            encoded = value.encode("utf-8")
        except UnicodeEncodeError as exc:
            raise CborError("invalid UTF-8 text string") from exc
        return _head(3, len(encoded)) + encoded
    if isinstance(value, Mapping):
        entries: list[tuple[bytes, bytes]] = []
        seen: set[bytes] = set()
        for key, item in value.items():
            if type(key) not in (int, bytes, str):
                raise CborError("map keys must be unsigned integers, bytes, or text")
            encoded_key = _encode(key, depth + 1)
            if encoded_key in seen:
                raise CborError("duplicate encoded map key")
            seen.add(encoded_key)
            entries.append((encoded_key, _encode(item, depth + 1)))
        entries.sort(key=lambda pair: pair[0])
        return _head(5, len(entries)) + b"".join(k + v for k, v in entries)
    raise CborError(f"unsupported CBOR value type: {type(value).__name__}")


class _Decoder:
    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0

    def take(self, count: int) -> bytes:
        end = self.offset + count
        if end > len(self.data):
            raise CborError("truncated CBOR item")
        part = self.data[self.offset:end]
        self.offset = end
        return part

    def argument(self, additional: int) -> int:
        if additional < 24:
            return additional
        widths = {24: 1, 25: 2, 26: 4, 27: 8}
        if additional not in widths:
            raise CborError("indefinite or reserved additional information")
        width = widths[additional]
        value = int.from_bytes(self.take(width), "big")
        minima = {1: 24, 2: 0x100, 4: 0x10000, 8: 0x100000000}
        if value < minima[width]:
            raise CborError("non-minimal additional information")
        return value

    def item(self, depth: int = 0) -> Any:
        if depth > 32:
            raise CborError("maximum nesting depth exceeded")
        initial = self.take(1)[0]
        major, additional = initial >> 5, initial & 31
        value = self.argument(additional)
        if major == 0:
            return value
        if major == 2:
            return self.take(value)
        if major == 3:
            try:
                return self.take(value).decode("utf-8", "strict")
            except UnicodeDecodeError as exc:
                raise CborError("invalid UTF-8 text string") from exc
        if major == 5:
            result: dict[Any, Any] = {}
            previous: bytes | None = None
            for _ in range(value):
                start = self.offset
                key = self.item(depth + 1)
                encoded_key = self.data[start:self.offset]
                if type(key) not in (int, bytes, str):
                    raise CborError("map keys must be unsigned integers, bytes, or text")
                if previous is not None and encoded_key <= previous:
                    raise CborError("duplicate or non-deterministically ordered map key")
                if key in result:
                    raise CborError("duplicate map key")
                previous = encoded_key
                result[key] = self.item(depth + 1)
            return result
        raise CborError("unsupported CBOR major type")


def loads(data: bytes, *, max_size: int | None = None) -> Any:
    """Decode one canonical item, rejecting unsupported features and trailing bytes."""
    if type(data) is not bytes:
        raise CborError("CBOR input must be bytes")
    if max_size is not None and len(data) > max_size:
        raise CborError("CBOR input exceeds size limit")
    decoder = _Decoder(data)
    value = decoder.item()
    if decoder.offset != len(data):
        raise CborError("trailing bytes after CBOR item")
    return value
