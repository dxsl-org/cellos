"""Private VMware lab JSON/Ed25519 carrier; not a DCO1 codec or trust root.

Canonical bytes are compact, ASCII-escaped JSON with lexicographically sorted
string keys and no trailing newline. Only exact JSON Python types are accepted;
floats, surrogate code points, duplicate decoded keys and unbounded integers are
rejected. Limits below are lab-only resource caps, not production profiles.

Identities live in root/identities, outside the source payload. Provision is an
explicit, exclusive operation: an interrupted provision must be retired, never
repaired. The roster is written last. Open requires every pinned private key and
never generates or rewrites anything. All roles share the guest/root trust
domain: roster continuity does not provide disk rollback or root resistance.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import stat
from typing import Any

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import (
    Ed25519PrivateKey,
    Ed25519PublicKey,
)

PROTOCOL = "cellos-durable-charge-lab-json-ed25519"
LANE = "isolated-vmware-lab"
VERSION = 1
ROLES = ("issuer", "authority", "successor", "recovery", "destination")
MAX_JSON_BYTES = 1_048_576
MAX_DEPTH = 32
MAX_NODES = 16_384
MAX_MEMBERS = 1_024
MAX_STRING_BYTES = 65_536
MIN_INTEGER = -(1 << 63)
MAX_INTEGER = (1 << 64) - 1
_HEX32 = re.compile(r"[0-9a-f]{64}\Z")
_HEX64 = re.compile(r"[0-9a-f]{128}\Z")
_ROSTER_FIELDS = {"protocol", "lane", "version", "scope", "roster_id", "roles"}
_ENVELOPE_FIELDS = {
    "protocol", "lane", "version", "scope", "roster_pin", "role", "purpose",
    "key_pin", "body", "signature",
}


def canonical(value: Any) -> bytes:
    """Encode strict finite JSON; reject rather than coerce non-JSON values."""
    chunks: list[str] = []
    size = 0
    nodes = 0
    active: set[int] = set()

    def emit(text: str) -> None:
        nonlocal size
        size += len(text)  # Every emitted character is ASCII.
        if size > MAX_JSON_BYTES:
            raise ValueError("lab JSON exceeds byte cap")
        chunks.append(text)

    def string(text: str) -> None:
        if len(text) > MAX_STRING_BYTES:
            raise ValueError("lab JSON string exceeds byte cap")
        try:
            length = len(text.encode("utf-8", errors="strict"))
        except UnicodeError:
            raise ValueError("lab JSON contains a non-scalar string") from None
        if length > MAX_STRING_BYTES:
            raise ValueError("lab JSON string exceeds byte cap")
        emit(json.dumps(text, ensure_ascii=True))

    def visit(item: Any, depth: int) -> None:
        nonlocal nodes
        nodes += 1
        if nodes > MAX_NODES or depth > MAX_DEPTH:
            raise ValueError("lab JSON exceeds structural cap")
        kind = type(item)
        if item is None:
            emit("null")
        elif kind is bool:
            emit("true" if item else "false")
        elif kind is int:
            if not MIN_INTEGER <= item <= MAX_INTEGER:
                raise ValueError("lab JSON integer exceeds finite range")
            emit(str(item))
        elif kind is str:
            string(item)
        elif kind is list or kind is dict:
            if len(item) > MAX_MEMBERS or id(item) in active:
                raise ValueError("lab JSON container exceeds cap or is cyclic")
            active.add(id(item))
            try:
                if kind is list:
                    emit("[")
                    for index, child in enumerate(item):
                        if index:
                            emit(",")
                        visit(child, depth + 1)
                    emit("]")
                else:
                    if any(type(key) is not str for key in item):
                        raise ValueError("lab JSON object keys must be strings")
                    emit("{")
                    for index, key in enumerate(sorted(item)):
                        if index:
                            emit(",")
                        nodes += 1
                        if nodes > MAX_NODES:
                            raise ValueError("lab JSON exceeds structural cap")
                        string(key)
                        emit(":")
                        visit(item[key], depth + 1)
                    emit("}")
            finally:
                active.remove(id(item))
        else:
            raise ValueError("lab JSON accepts no floats or non-JSON types")

    visit(value, 1)
    return "".join(chunks).encode("ascii")


def strict_loads(data: bytes | str) -> Any:
    """Bounded UTF-8 JSON decoding, including duplicate-key rejection."""
    if type(data) not in (bytes, str) or len(data) > MAX_JSON_BYTES:
        raise ValueError("invalid or oversized lab JSON input")
    try:
        text = data.decode("utf-8", errors="strict") if type(data) is bytes else data
        if len(text.encode("utf-8", errors="strict")) > MAX_JSON_BYTES:
            raise ValueError("oversized lab JSON input")
    except UnicodeError:
        raise ValueError("invalid lab JSON UTF-8") from None

    # Bound nesting before the general JSON decoder allocates nested containers.
    depth = 0
    quoted = False
    escaped = False
    for char in text:
        if quoted:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char in "[{":
            depth += 1
            if depth > MAX_DEPTH:
                raise ValueError("lab JSON exceeds nesting cap")
        elif char in "]}":
            depth -= 1
            if depth < 0:
                raise ValueError("invalid lab JSON nesting")

    def unique(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        if len(pairs) > MAX_MEMBERS:
            raise ValueError("lab JSON object exceeds member cap")
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate lab JSON object key")
            result[key] = value
        return result

    def integer(token: str) -> int:
        if len(token) > 20:
            raise ValueError("lab JSON integer exceeds finite range")
        value = int(token)
        if not MIN_INTEGER <= value <= MAX_INTEGER:
            raise ValueError("lab JSON integer exceeds finite range")
        return value

    def forbidden_number(_token: str) -> Any:
        raise ValueError("lab JSON accepts no floats or non-finite numbers")

    try:
        value = json.loads(
            text, object_pairs_hook=unique, parse_int=integer,
            parse_float=forbidden_number, parse_constant=forbidden_number,
        )
    except (ValueError, RecursionError):
        raise ValueError("invalid strict lab JSON") from None
    canonical(value)
    return value


def _label(value: str, limit: int, name: str) -> None:
    if type(value) is not str or not value or len(value) > limit:
        raise ValueError("invalid lab " + name)
    try:
        valid = len(value.encode("utf-8", errors="strict")) <= limit
    except UnicodeError:
        valid = False
    if not valid or any(ord(char) < 32 or 127 <= ord(char) < 160 for char in value):
        raise ValueError("invalid lab " + name)


def _role(role: str) -> None:
    if type(role) is not str or role not in ROLES:
        raise ValueError("unknown lab signing role")


def _hex(value: Any, pattern: re.Pattern[str]) -> bool:
    return type(value) is str and pattern.fullmatch(value) is not None


def _open_directory(path: Path | str, *, parent: int | None = None,
                    private: bool = False) -> int:
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                 dir_fd=parent)
    info = os.fstat(fd)
    mode = stat.S_IMODE(info.st_mode)
    if info.st_uid != os.geteuid() or mode & 0o022 or (private and mode != 0o700):
        os.close(fd)
        raise ValueError("lab identity directory has unsafe ownership or mode")
    return fd


def _read_file(directory: int, name: str, cap: int) -> bytes:
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK,
                 dir_fd=directory)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                or stat.S_IMODE(info.st_mode) != 0o600 or info.st_nlink != 1
                or info.st_size > cap):
            raise ValueError("lab identity file has invalid type, mode or size")
        raw = stream.read(cap + 1)
    if len(raw) > cap:
        raise ValueError("lab identity file exceeds size cap")
    return raw


def _write_file(directory: int, name: str, raw: bytes) -> None:
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
                 | os.O_CLOEXEC, 0o600, dir_fd=directory)
    with os.fdopen(fd, "wb") as stream:
        os.fchmod(stream.fileno(), 0o600)
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    os.fsync(directory)


def _public(key: Ed25519PrivateKey) -> bytes:
    return key.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw,
    )


class Keyring:
    """Complete pinned role assignment; construction is through provision/open."""

    def __init__(self, roster: dict[str, Any], keys: dict[str, Ed25519PrivateKey]):
        # Private constructor state never aliases caller-owned containers.
        self._roster_bytes = canonical(roster)
        self._roster_pin = hashlib.sha256(self._roster_bytes).hexdigest()
        self._scope = roster["scope"]
        self._pins = dict(roster["roles"])
        self._keys = dict(keys)
        self._verifiers = {
            role: Ed25519PublicKey.from_public_bytes(bytes.fromhex(pin))
            for role, pin in self._pins.items()
        }

    @classmethod
    def provision(cls, root: Path, scope: str) -> Keyring:
        """Exclusively create fresh durable lab identities, never repair/reuse."""
        _label(scope, 256, "scope")
        root = Path(root)
        root_fd = None
        identity_fd = None
        try:
            try:
                root.mkdir(mode=0o700)
            except FileExistsError:
                pass
            else:
                parent_fd = _open_directory(root.parent)
                try:
                    os.fsync(parent_fd)
                finally:
                    os.close(parent_fd)
            root_fd = _open_directory(root)
            os.mkdir("identities", mode=0o700, dir_fd=root_fd)
            os.fsync(root_fd)
            identity_fd = _open_directory("identities", parent=root_fd, private=True)
            keys = {role: Ed25519PrivateKey.generate() for role in ROLES}
            roster = {
                "protocol": PROTOCOL, "lane": LANE, "version": VERSION,
                "scope": scope, "roster_id": os.urandom(32).hex(),
                "roles": {role: _public(key).hex() for role, key in keys.items()},
            }
            for role, key in keys.items():
                raw = key.private_bytes(
                    serialization.Encoding.Raw, serialization.PrivateFormat.Raw,
                    serialization.NoEncryption(),
                )
                _write_file(identity_fd, role + ".key", raw)
            # A complete durable roster is the publication point, never a repair hint.
            _write_file(identity_fd, "roster.json", canonical(roster))
            return cls(roster, keys)
        except OSError:
            raise ValueError("lab identity provisioning refused; no repair attempted") from None
        finally:
            if identity_fd is not None:
                os.close(identity_fd)
            if root_fd is not None:
                os.close(root_fd)

    @classmethod
    def open(cls, root: Path, scope: str) -> Keyring:
        """Load only a complete canonical roster and its exact pinned key set."""
        _label(scope, 256, "scope")
        root_fd = None
        identity_fd = None
        try:
            root_fd = _open_directory(Path(root))
            identity_fd = _open_directory("identities", parent=root_fd, private=True)
            expected = {"roster.json"} | {role + ".key" for role in ROLES}
            if set(os.listdir(identity_fd)) != expected:
                raise ValueError("incomplete or unexpected lab identity files")
            raw = _read_file(identity_fd, "roster.json", MAX_JSON_BYTES)
            roster = strict_loads(raw)
            if (type(roster) is not dict or set(roster) != _ROSTER_FIELDS
                    or canonical(roster) != raw or roster["protocol"] != PROTOCOL
                    or roster["lane"] != LANE or type(roster["version"]) is not int
                    or roster["version"] != VERSION or roster["scope"] != scope
                    or not _hex(roster["roster_id"], _HEX32)
                    or type(roster["roles"]) is not dict
                    or set(roster["roles"]) != set(ROLES)):
                raise ValueError("lab assignment roster does not match context")
            pins = roster["roles"]
            if (any(not _hex(pin, _HEX32) for pin in pins.values())
                    or len(set(pins.values())) != len(ROLES)):
                raise ValueError("lab role assignments are invalid or not distinct")
            keys = {}
            for role in ROLES:
                seed = _read_file(identity_fd, role + ".key", 32)
                if len(seed) != 32:
                    raise ValueError("invalid lab private key length")
                key = Ed25519PrivateKey.from_private_bytes(seed)
                if _public(key).hex() != pins[role]:
                    raise ValueError("lab private key does not match pinned assignment")
                keys[role] = key
            return cls(roster, keys)
        except OSError:
            raise ValueError("lab identities unavailable; no regeneration attempted") from None
        finally:
            if identity_fd is not None:
                os.close(identity_fd)
            if root_fd is not None:
                os.close(root_fd)

    def public_roster(self) -> dict[str, Any]:
        """Return a detached public assignment; never include private key bytes."""
        return strict_loads(self._roster_bytes)

    def sign(self, role: str, purpose: str, body: dict[str, Any]) -> dict[str, Any]:
        _role(role)
        _label(purpose, 128, "purpose")
        if type(body) is not dict:
            raise ValueError("lab proof body must be an object")
        payload = {
            "protocol": PROTOCOL, "lane": LANE, "version": VERSION,
            "scope": self._scope, "roster_pin": self._roster_pin,
            "role": role, "purpose": purpose, "key_pin": self._pins[role],
            "body": strict_loads(canonical(body)),
        }
        signature = self._keys[role].sign(canonical(payload)).hex()
        envelope = {**payload, "signature": signature}
        canonical(envelope)  # Envelope overhead is included in the finite byte cap.
        return envelope

    def verify(self, envelope: dict[str, Any], role: str,
               purpose: str) -> dict[str, Any]:
        """Verify exact assignment/context/signature before returning body data."""
        _role(role)
        _label(purpose, 128, "purpose")
        if type(envelope) is not dict or set(envelope) != _ENVELOPE_FIELDS:
            raise ValueError("invalid lab proof envelope fields")
        proof = strict_loads(canonical(envelope))
        if (proof["protocol"] != PROTOCOL or proof["lane"] != LANE
                or type(proof["version"]) is not int or proof["version"] != VERSION
                or proof["scope"] != self._scope or proof["role"] != role
                or proof["purpose"] != purpose or proof["key_pin"] != self._pins[role]
                or proof["roster_pin"] != self._roster_pin
                or type(proof["body"]) is not dict
                or not _hex(proof["signature"], _HEX64)):
            raise ValueError("lab proof assignment or context mismatch")
        signature = bytes.fromhex(proof.pop("signature"))
        try:
            self._verifiers[role].verify(signature, canonical(proof))
        except InvalidSignature:
            raise ValueError("invalid lab Ed25519 signature") from None
        return proof["body"]
