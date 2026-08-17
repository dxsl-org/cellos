"""Pure TFTP protocol helpers used by the RPi3 netboot server."""

from __future__ import annotations

from pathlib import Path, PurePosixPath


def is_allowed_client(client: tuple[str, int], allowed_address: str) -> bool:
    return client[0] == allowed_address


def parse_rrq(packet: bytes) -> tuple[str, dict[str, str]]:
    if len(packet) < 4 or packet[:2] != b"\x00\x01":
        raise ValueError("not an RRQ")
    parts = packet[2:].split(b"\x00")
    if len(parts) < 3:
        raise ValueError("malformed RRQ")
    name = parts[0].decode("ascii", "strict")
    values = [part.decode("ascii", "strict").lower() for part in parts[2:] if part]
    return name, dict(zip(values[0::2], values[1::2]))


def resolve_tftp_file(root: Path, requested: str) -> Path:
    clean = PurePosixPath(requested.replace("\\", "/"))
    if clean.is_absolute() or ".." in clean.parts:
        raise ValueError("unsafe TFTP path")
    candidate = root.joinpath(*clean.parts)
    if not candidate.is_file() and len(clean.parts) > 1:
        candidate = root / clean.name
    if not candidate.is_file() or candidate.parent.resolve() != root.resolve():
        raise FileNotFoundError(requested)
    return candidate
