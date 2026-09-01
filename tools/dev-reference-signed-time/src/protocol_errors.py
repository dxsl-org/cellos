"""Shared public exception boundary for signed-time protocol failures."""


class ProtocolError(ValueError):
    """Raised when bytes, claims, keys, or signatures violate protocol v1."""
