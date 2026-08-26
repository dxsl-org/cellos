"""Strict loading of relay-server fields from the mounted manifest."""

from __future__ import annotations

import re
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from cryptography import x509

_LABEL = re.compile(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\Z")
_RELAY_FIELDS = frozenset({"bind_host", "hostname", "port", "min_tls_version"})
_SERVER_FIELDS = frozenset(
    {"certificate_pem", "private_key_pem", "client_issuing_ca_pem"}
)
_AUTHORIZATION_FIELDS = frozenset(
    {"net_service_identity", "policy_handle", "relay_denylist"}
)
_ENROLLMENT_FIELDS = frozenset({"pending_generation", "policy_epoch"})
_U64_MAX = (1 << 64) - 1
_HOSTNAME_MAX_BYTES = 64
_ROOT_FIELDS = frozenset(
    {"relay", "trust", "client", "server", "authorization", "enrollment"}
)


class ManifestError(ValueError):
    """The mounted relay manifest is malformed or unsafe to use."""


@dataclass(frozen=True, slots=True)
class RelayServerConfig:
    bind_host: str
    hostname: str
    port: int
    min_tls_version: str
    certificate_pem: Path
    private_key_pem: Path
    client_issuing_ca_pem: Path
    relay_denylist: Path


def load_server_manifest(path: str | Path) -> RelayServerConfig:
    document = _load_toml(Path(path))
    unexpected = set(document) - _ROOT_FIELDS
    if unexpected:
        raise ManifestError(f"unexpected top-level field: {min(unexpected)}")

    relay = _table(document, "relay", _RELAY_FIELDS, _RELAY_FIELDS)
    server = _table(document, "server", _SERVER_FIELDS, _SERVER_FIELDS)
    authorization = _table(
        document, "authorization", {"relay_denylist"}, _AUTHORIZATION_FIELDS
    )
    enrollment = _table(
        document, "enrollment", _ENROLLMENT_FIELDS, _ENROLLMENT_FIELDS
    )

    bind_host = _string(relay, "bind_host")
    if not bind_host or bind_host != bind_host.strip():
        raise ManifestError("relay.bind_host must be a non-empty host string")
    hostname = _hostname(_string(relay, "hostname"))
    port = relay["port"]
    if type(port) is not int or not 1 <= port <= 65535:
        raise ManifestError("relay.port must be an integer from 1 through 65535")
    tls_version = _string(relay, "min_tls_version")
    if tls_version != "1.3":
        raise ManifestError("relay.min_tls_version must be exactly '1.3'")
    _u64(enrollment, "pending_generation")
    _u64(enrollment, "policy_epoch")

    certificate = _regular_file(server, "certificate_pem")
    private_key = _regular_file(server, "private_key_pem")
    client_ca = _regular_file(server, "client_issuing_ca_pem")
    denylist = _regular_file(authorization, "relay_denylist")
    _require_dns_san(certificate, hostname)
    return RelayServerConfig(
        bind_host,
        hostname,
        port,
        tls_version,
        certificate,
        private_key,
        client_ca,
        denylist,
    )


def _load_toml(path: Path) -> dict[str, Any]:
    try:
        with path.open("rb") as manifest:
            document = tomllib.load(manifest)
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise ManifestError(f"cannot load relay manifest: {exc}") from exc
    if not isinstance(document, dict):
        raise ManifestError("relay manifest must be a TOML document")
    return document


def _table(
    document: dict[str, Any],
    name: str,
    required: set[str] | frozenset[str],
    allowed: frozenset[str],
) -> dict[str, Any]:
    value = document.get(name)
    if not isinstance(value, dict):
        raise ManifestError(f"[{name}] must be a table")
    missing = required - set(value)
    if missing:
        raise ManifestError(f"missing {name}.{min(missing)}")
    unexpected = set(value) - allowed
    if unexpected:
        raise ManifestError(f"unexpected {name}.{min(unexpected)}")
    return value


def _string(table: dict[str, Any], field: str) -> str:
    value = table[field]
    if not isinstance(value, str):
        raise ManifestError(f"{field} must be a string")
    return value


def _u64(table: dict[str, Any], field: str) -> int:
    value = table[field]
    if type(value) is not int or not 1 <= value <= _U64_MAX:
        raise ManifestError(
            f"enrollment.{field} must be an integer from 1 through {_U64_MAX}"
        )
    return value


def _regular_file(table: dict[str, Any], field: str) -> Path:
    path = Path(_string(table, field))
    if not path.is_absolute():
        raise ManifestError(f"{field} must be an absolute path")
    if not path.is_file():
        raise ManifestError(f"{field} must name an existing regular file")
    return path


def _hostname(value: str) -> str:
    try:
        encoded = value.encode("ascii")
    except UnicodeEncodeError as exc:
        raise ManifestError(
            "relay.hostname must match the frozen lowercase DNS profile"
        ) from exc
    labels = value.split(".")
    if (
        not value
        or len(encoded) > _HOSTNAME_MAX_BYTES
        or any(
            not label
            or len(label) > 63
            or not _LABEL.fullmatch(label)
            for label in labels
        )
    ):
        raise ManifestError(
            "relay.hostname must match the frozen lowercase DNS profile"
        )
    return value


def _require_dns_san(certificate_path: Path, hostname: str) -> None:
    try:
        certificate = x509.load_pem_x509_certificate(certificate_path.read_bytes())
        san = certificate.extensions.get_extension_for_class(x509.SubjectAlternativeName).value
    except (OSError, ValueError, x509.ExtensionNotFound) as exc:
        raise ManifestError("server certificate must contain a valid DNS SAN") from exc
    names = san.get_values_for_type(x509.DNSName)
    if not any(_dns_name_matches(name.lower(), hostname) for name in names):
        raise ManifestError("server certificate DNS SAN does not match relay.hostname")


def _dns_name_matches(pattern: str, hostname: str) -> bool:
    if pattern == hostname:
        return True
    if not pattern.startswith("*."):
        return False
    suffix = pattern[2:]
    prefix = hostname[: -(len(suffix) + 1)] if hostname.endswith("." + suffix) else ""
    return bool(prefix) and "." not in prefix
