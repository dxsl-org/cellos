"""Pure authenticated route admission for the relay server."""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum, auto
from typing import Generic, TypeVar

Session = TypeVar("Session")
NODE_ID_LEN = 32


class AdmissionError(Enum):
    """Fail-closed reasons that never mutate the live route table."""

    UNAUTHENTICATED = auto()
    IDENTITY_MISMATCH = auto()
    DUPLICATE_LIVE = auto()
    CAPACITY = auto()
    STALE_DISCONNECT = auto()


@dataclass(frozen=True)
class AuthenticatedSessionIdentity:
    """Certificate-derived NodeId after TLS and certificate-policy validation."""

    node_id: bytes


@dataclass(frozen=True)
class AdmissionLease(Generic[Session]):
    """Generation-bound ownership of one authenticated NodeId route."""

    node_id: bytes
    generation: int
    session: Session


class AdmissionTable(Generic[Session]):
    """Bounded route table that never replaces a live authenticated identity."""

    def __init__(self, capacity: int) -> None:
        if capacity <= 0:
            raise ValueError("admission capacity must be positive")
        self._capacity = capacity
        self._next_generation = 0
        self._leases: dict[bytes, AdmissionLease[Session]] = {}

    def admit(
        self,
        authenticated: AuthenticatedSessionIdentity | None,
        claimed_node_id: bytes,
        session: Session,
    ) -> AdmissionLease[Session] | AdmissionError:
        """Admit an identity-matched session without displacing a live route."""
        if authenticated is None:
            return AdmissionError.UNAUTHENTICATED
        authenticated_node_id = authenticated.node_id
        if (
            len(authenticated_node_id) != NODE_ID_LEN
            or len(claimed_node_id) != NODE_ID_LEN
            or claimed_node_id != authenticated_node_id
        ):
            return AdmissionError.IDENTITY_MISMATCH
        if claimed_node_id in self._leases:
            return AdmissionError.DUPLICATE_LIVE
        if len(self._leases) >= self._capacity:
            return AdmissionError.CAPACITY
        self._next_generation += 1
        lease = AdmissionLease(claimed_node_id, self._next_generation, session)
        self._leases[claimed_node_id] = lease
        return lease

    def current(
        self,
        node_id: bytes,
        generation: int,
        session: Session | None = None,
    ) -> AdmissionLease[Session] | None:
        """Return the matching live lease, optionally bound to the same session object."""
        lease = self._leases.get(node_id)
        if lease is None or lease.generation != generation:
            return None
        if session is not None and lease.session is not session:
            return None
        return lease

    def lookup(self, node_id: bytes) -> AdmissionLease[Session] | None:
        """Return the current authenticated route for a destination NodeId."""
        return self._leases.get(node_id)

    def release(self, node_id: bytes, generation: int) -> AdmissionError | None:
        """Release the exact generation or reject stale disconnect cleanup."""
        if self.current(node_id, generation) is None:
            return AdmissionError.STALE_DISCONNECT
        del self._leases[node_id]
        return None

    def __len__(self) -> int:
        return len(self._leases)
