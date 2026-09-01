"""Strict bounded codec for draft-11 Roughtime messages and packets."""

from dataclasses import dataclass
import struct
from typing import NoReturn, Sequence

PACKET_MAGIC = b"ROUGHTIM"
PACKET_HEADER_BYTES = 12
_ERROR = "invalid roughtime encoding"


class RoughtimeCodecError(ValueError):
    """Stable value-free rejection for malformed wire data."""

    __slots__ = ()


def _fail() -> NoReturn:
    raise RoughtimeCodecError(_ERROR) from None


def _tag_wire(tag: object) -> bytes:
    if type(tag) is not str or not 1 <= len(tag) <= 4 or not tag.isascii():
        _fail()
    encoded = tag.encode("ascii")
    if any(byte < 65 or byte > 90 for byte in encoded):
        _fail()
    return encoded.ljust(4, b"\0")


def _decode_tag(raw: bytes) -> str:
    end = raw.find(b"\0")
    if end < 0:
        end = 4
    if end == 0 or raw[end:] != b"\0" * (4 - end):
        _fail()
    name = raw[:end]
    if any(byte < 65 or byte > 90 for byte in name):
        _fail()
    return name.decode("ascii")


@dataclass(frozen=True, slots=True)
class RoughtimeMessage:
    """An immutable ordered sequence of validated tag values."""

    entries: tuple[tuple[str, bytes], ...]

    def names(self) -> tuple[str, ...]:
        return tuple(name for name, _ in self.entries)

    def value(self, name: str) -> bytes:
        for candidate, value in self.entries:
            if candidate == name:
                return value
        _fail()


def encode_message(
    entries: Sequence[tuple[str, bytes]], *, max_pairs: int, max_bytes: int,
) -> bytes:
    """Encode one message after bounding, sorting, and validating every pair."""
    if (
        type(max_pairs) is not int or max_pairs < 1
        or type(max_bytes) is not int or max_bytes < 4
        or type(entries) not in (tuple, list)
        or not 1 <= len(entries) <= max_pairs
    ):
        _fail()
    prepared: list[tuple[int, bytes, bytes]] = []
    seen: set[int] = set()
    for entry in entries:
        if type(entry) is not tuple or len(entry) != 2 or type(entry[1]) is not bytes:
            _fail()
        tag = _tag_wire(entry[0])
        numeric = int.from_bytes(tag, "little")
        if numeric in seen:
            _fail()
        seen.add(numeric)
        prepared.append((numeric, tag, entry[1]))
    prepared.sort(key=lambda item: item[0])
    values = [item[2] for item in prepared]
    if any(len(value) % 4 for value in values[:-1]):
        _fail()
    offsets: list[int] = []
    position = 0
    for value in values[:-1]:
        position += len(value)
        if position > 0xFFFFFFFF:
            _fail()
        offsets.append(position)
    header = (
        struct.pack("<I", len(prepared))
        + b"".join(struct.pack("<I", offset) for offset in offsets)
        + b"".join(item[1] for item in prepared)
    )
    encoded = header + b"".join(values)
    if len(encoded) > max_bytes:
        _fail()
    return encoded


def decode_message(data: bytes, *, max_pairs: int, max_bytes: int) -> RoughtimeMessage:
    """Decode exactly one nonrecursive message; callers bound nested values."""
    if (
        type(data) is not bytes or type(max_pairs) is not int or max_pairs < 1
        or type(max_bytes) is not int or not 4 <= len(data) <= max_bytes
    ):
        _fail()
    count = struct.unpack_from("<I", data)[0]
    if not 1 <= count <= max_pairs:
        _fail()
    header_bytes = 8 * count
    if header_bytes > len(data):
        _fail()
    offsets = (0,) + tuple(
        struct.unpack_from("<I", data, 4 + index * 4)[0]
        for index in range(count - 1)
    )
    value_bytes = len(data) - header_bytes
    if any(
        offset % 4 or offset > value_bytes
        for offset in offsets[1:]
    ) or any(left > right for left, right in zip(offsets, offsets[1:])):
        _fail()
    tag_start = 4 * count
    tags: list[tuple[int, str]] = []
    for index in range(count):
        raw = data[tag_start + index * 4:tag_start + (index + 1) * 4]
        name = _decode_tag(raw)
        tags.append((int.from_bytes(raw, "little"), name))
    if any(left >= right for (left, _), (right, _) in zip(tags, tags[1:])):
        _fail()
    ends = offsets[1:] + (value_bytes,)
    values = data[header_bytes:]
    return RoughtimeMessage(tuple(
        (tags[index][1], values[offsets[index]:ends[index]])
        for index in range(count)
    ))


def encode_packet(message: bytes, *, max_packet_bytes: int) -> bytes:
    if (
        type(message) is not bytes or type(max_packet_bytes) is not int
        or len(message) > 0xFFFFFFFF
        or PACKET_HEADER_BYTES + len(message) > max_packet_bytes
    ):
        _fail()
    return PACKET_MAGIC + struct.pack("<I", len(message)) + message


def decode_packet(packet: bytes, *, max_packet_bytes: int) -> bytes:
    if (
        type(packet) is not bytes or type(max_packet_bytes) is not int
        or not PACKET_HEADER_BYTES <= len(packet) <= max_packet_bytes
        or packet[:8] != PACKET_MAGIC
    ):
        _fail()
    length = struct.unpack_from("<I", packet, 8)[0]
    if length != len(packet) - PACKET_HEADER_BYTES:
        _fail()
    return packet[PACKET_HEADER_BYTES:]
