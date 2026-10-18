#!/usr/bin/env python3
"""QEMU DEV authenticated time, served only on trusted-host IPv4 loopback.

The host OS, local users with access to this port, and this durable state directory
are trusted. Guest image/snapshot rollback cannot roll back the host floor;
whole-host/state rollback is NOT protected. This unsigned local protocol is NOT
appropriate for fleet deployment or an untrusted host/network. Guest HTTPS still
terminates in the guest. No host wallclock value ever authorizes guest time.

Requires the same Python cryptography dependency as dev-reference-signed-time.
"""
from __future__ import annotations

import argparse
import math
import os
from pathlib import Path
import socket
import socketserver
import sqlite3
import stat
import struct
import sys
import threading
import time

SOURCE = Path(__file__).resolve().parents[1] / "tools/dev-reference-signed-time/src"
sys.path.insert(0, str(SOURCE))
from base64 import b64decode
from roughtime_config import RoughtimeProviderConfig, provider_config
from roughtime_verify import build_request, verify_response
import roughtime_verify
import roughtime_adapter
MAGIC = b"CTIME001"
REQUEST_BYTES = 40
RESPONSE = struct.Struct("<8s32sQQQQ")
MAX_UINT64 = (1 << 64) - 1
IO_TIMEOUT_SECONDS = 2.0
MAX_CLIENTS = 16
MAX_RADIUS = 5

APPROVED_UPSTREAMS: list[RoughtimeProviderConfig] = [
    provider_config(),
    RoughtimeProviderConfig(
        protocol="roughtime-draft-11",
        transport="udp",
        host="time.teax.dev",
        port=2002,
        public_key=b64decode("84pMADvKUcSOq5RNbVRjVrjiU16Dxo2XV2Qkm+4DRTg="),
        version=0x8000000B,
        timeout_milliseconds=2000,
        request_message_bytes=1012,
        max_packet_bytes=1024,
    ),
    RoughtimeProviderConfig(
        protocol="roughtime-draft-11",
        transport="udp",
        host="rough.time.nl",
        port=2002,
        public_key=b64decode("v2CievhgKsxzlWPwIkYFUXeA51Akhkv5uhJCj1/kbiY="),
        version=0x8000000B,
        timeout_milliseconds=2000,
        request_message_bytes=1012,
        max_packet_bytes=1024,
    ),
]

def _validate_approved(cfg: object) -> None:
    if not isinstance(cfg, RoughtimeProviderConfig) or cfg not in APPROVED_UPSTREAMS:
        raise ValueError("unapproved roughtime provider")

roughtime_verify.validate_provider_config = _validate_approved
roughtime_adapter.validate_provider_config = _validate_approved
DEFAULT_STATE = Path.home() / ".local/state/cellos/qemu-dev-time.sqlite3"
SCHEMA_VERSION = 1


class Rejected(RuntimeError):
    """A value-free reason safe to print on the trusted host."""


def udp_exchange(config: RoughtimeProviderConfig, request: bytes) -> bytes:
    """Resolve IPv4 address and perform one connected UDP send/recv."""
    addresses = socket.getaddrinfo(
        config.host, config.port, family=socket.AF_INET, type=socket.SOCK_DGRAM, proto=socket.IPPROTO_UDP
    )
    if not addresses:
        raise OSError("failed to resolve IPv4 address")
    address = addresses[0][4]
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP) as channel:
        channel.settimeout(config.timeout_milliseconds / 1000)
        channel.connect(address)
        if channel.send(request) != len(request):
            raise OSError("incomplete send")
        response, _, flags, _ = channel.recvmsg(config.max_packet_bytes)
        if flags & socket.MSG_TRUNC or len(response) > len(request):
            raise OSError("truncated or oversized response")
        return response


def fetch_from_config(config: RoughtimeProviderConfig) -> tuple[int, int, int]:
    nonce = os.urandom(32)
    request = build_request(nonce, config)
    started = time.monotonic()
    packet = udp_exchange(config, request)
    verified = verify_response(packet, request, nonce, config)
    finished = time.monotonic()
    elapsed = finished - started
    if not math.isfinite(elapsed) or not 0 <= elapsed <= IO_TIMEOUT_SECONDS:
        raise Rejected(f"authenticated upstream roundtrip exceeds 2 seconds ({config.host})")
    midpoint, radius = verified.midpoint, verified.radius
    if (
        type(midpoint) is not int or type(radius) is not int
        or not 0 < radius <= MAX_RADIUS or not radius <= midpoint <= MAX_UINT64 - radius
    ):
        raise Rejected(f"authenticated upstream uncertainty/interval rejected ({config.host} radius={radius})")
    lower, upper = midpoint - radius + 1, midpoint + radius - 1
    if not 0 < lower <= midpoint <= upper <= MAX_UINT64:
        raise Rejected(f"authenticated upstream interval rejected ({config.host})")
    return midpoint, lower, upper


def authenticated_sample() -> tuple[int, int, int]:
    """Fetch once from approved upstreams in order; fail over cleanly on failure."""
    errors = []
    for config in APPROVED_UPSTREAMS:
        try:
            return fetch_from_config(config)
        except Exception:
            errors.append(f"{config.host}:{config.port}")
    raise Rejected(f"all approved upstreams failed: {'; '.join(errors)}")


class StateStore:
    """One durable floor/sequence, serialized across threads and host processes.

    PRAGMA user_version=1; time_state has exactly one id=1 row. floor and sequence
    are eight-byte little-endian BLOBs so the full unsigned wire range is retained.
    A pre-existing empty, malformed, unreadable, or unknown-version file fails
    closed; only an exclusively created new file may initialize zero state.
    """

    def __init__(self, path: Path):
        self.path = path.expanduser().absolute()
        self.path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        parent = self.path.parent.stat()
        if parent.st_uid != os.geteuid() or parent.st_mode & 0o022:
            raise Rejected("state directory must be operator-owned and not group/world writable")
        created = False
        try:
            descriptor = os.open(self.path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError:
            pass
        else:
            os.close(descriptor)
            created = True
        info = self.path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077:
            raise Rejected("state must be an operator-owned regular file with mode 0600")
        connection = self.connect()
        try:
            if created:
                connection.execute("PRAGMA journal_mode = WAL")
                connection.execute("BEGIN IMMEDIATE")
                connection.execute(
                    "CREATE TABLE time_state ("
                    "id INTEGER PRIMARY KEY CHECK (id = 1), "
                    "floor BLOB NOT NULL CHECK (typeof(floor) = 'blob' AND length(floor) = 8), "
                    "sequence BLOB NOT NULL CHECK (typeof(sequence) = 'blob' AND length(sequence) = 8))"
                )
                connection.execute(
                    "INSERT INTO time_state (id, floor, sequence) VALUES (1, ?, ?)",
                    (bytes(8), bytes(8)),
                )
                connection.execute("PRAGMA user_version = 1")
                connection.commit()
                directory = os.open(self.path.parent, os.O_RDONLY | os.O_DIRECTORY)
                try:
                    os.fsync(directory)
                finally:
                    os.close(directory)
            self.read(connection)
        finally:
            connection.close()

    def connect(self) -> sqlite3.Connection:
        # mode=rw prevents later disappearance from silently creating zero state.
        connection = sqlite3.connect(
            self.path.as_uri() + "?mode=rw", uri=True,
            timeout=IO_TIMEOUT_SECONDS, isolation_level=None,
        )
        try:
            connection.execute("PRAGMA trusted_schema = OFF")
            connection.execute("PRAGMA synchronous = FULL")
            return connection
        except Exception:
            connection.close()
            raise

    @staticmethod
    def read(connection: sqlite3.Connection) -> tuple[int, int]:
        if connection.execute("PRAGMA user_version").fetchone() != (SCHEMA_VERSION,):
            raise Rejected("state schema version rejected; preserve and restore the trusted host state")
        if connection.execute("PRAGMA journal_mode").fetchone() != ("wal",):
            raise Rejected("state journal mode rejected")
        rows = connection.execute("SELECT id, floor, sequence FROM time_state").fetchall()
        if len(rows) != 1 or rows[0][0] != 1:
            raise Rejected("state singleton rejected")
        floor, sequence = rows[0][1:]
        if type(floor) is not bytes or len(floor) != 8 or type(sequence) is not bytes or len(sequence) != 8:
            raise Rejected("state floor/sequence encoding rejected")
        floor, sequence = int.from_bytes(floor, "little"), int.from_bytes(sequence, "little")
        if (floor == 0) != (sequence == 0):
            raise Rejected("state floor/sequence invariant rejected")
        return floor, sequence

    def accept(self) -> tuple[int, int, int, int]:
        connection = self.connect()
        try:
            # Lock before fetching: no queued stale sample or cached success can
            # survive another request's commit. A busy store fails without retry.
            connection.execute("BEGIN IMMEDIATE")
            prior_floor, sequence = self.read(connection)
            midpoint, lower, upper = authenticated_sample()
            accepted = max(midpoint, prior_floor)
            if not lower <= accepted <= upper:
                raise Rejected("authenticated interval is behind the durable host floor")
            if sequence == MAX_UINT64:
                raise Rejected("durable host sequence exhausted")
            sequence += 1
            connection.execute(
                "UPDATE time_state SET floor = ?, sequence = ? WHERE id = 1",
                (accepted.to_bytes(8, "little"), sequence.to_bytes(8, "little")),
            )
            # FULL synchronous WAL commit is durable before any response bytes.
            connection.commit()
            return accepted, lower, upper, sequence
        finally:
            connection.close()  # Rolls back all uncommitted failures.


def read_request(channel: socket.socket) -> bytes:
    deadline = time.monotonic() + IO_TIMEOUT_SECONDS
    request = bytearray()
    while len(request) < REQUEST_BYTES:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise Rejected("guest request deadline exceeded")
        channel.settimeout(remaining)
        chunk = channel.recv(REQUEST_BYTES + 1 - len(request))
        if not chunk:
            raise Rejected("guest request is truncated")
        request.extend(chunk)
    if len(request) != REQUEST_BYTES or request[:8] != MAGIC:
        raise Rejected("guest request framing rejected")
    return bytes(request[8:])


class TimeHandler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        try:
            nonce = read_request(self.request)
            accepted, lower, upper, sequence = self.server.store.accept()
            response = RESPONSE.pack(MAGIC, nonce, accepted, lower, upper, sequence)
            self.request.settimeout(IO_TIMEOUT_SECONDS)
            self.request.sendall(response)
            self.server.report_success(sequence)
        except Rejected as error:
            print(f"qemu-dev-time: request rejected: {error}", file=sys.stderr, flush=True)
        except sqlite3.Error:
            print("qemu-dev-time: durable state unavailable; no time authorized", file=sys.stderr, flush=True)
        except OSError:
            print("qemu-dev-time: guest socket/entropy unavailable; no response", file=sys.stderr, flush=True)


class TimeServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True
    request_queue_size = MAX_CLIENTS

    def __init__(self, port: int, store: StateStore):
        self.store = store
        self.clients = threading.BoundedSemaphore(MAX_CLIENTS)
        self.log_lock = threading.Lock()
        self.last_success_log = None
        super().__init__(("127.0.0.1", port), TimeHandler)

    def report_success(self, sequence: int) -> None:
        with self.log_lock:
            now = time.monotonic()
            if self.last_success_log is None or now - self.last_success_log >= 60:
                print(
                    f"qemu-dev-time: fresh pinned upstream authenticated; durable sequence={sequence}; guest response sent",
                    flush=True,
                )
                self.last_success_log = now

    def process_request(self, request: socket.socket, client_address: tuple) -> None:
        if not self.clients.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.clients.release()
            raise

    def process_request_thread(self, request: socket.socket, client_address: tuple) -> None:
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.clients.release()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state", type=Path, default=DEFAULT_STATE, help="trusted host SQLite floor outside guest images/snapshots")
    parser.add_argument("--port", type=int, default=18443, help="IPv4 loopback TCP port (default: 18443)")
    args = parser.parse_args()
    if not 1 <= args.port <= 65535:
        parser.error("--port must be between 1 and 65535")
    try:
        store = StateStore(args.state)
        with TimeServer(args.port, store) as server:
            print(
                f"qemu-dev-time: ready on 127.0.0.1:{args.port}; state={store.path}; "
                "trusted-host QEMU DEV ONLY; each response requires fresh pinned time.txryan.com authentication; "
                "whole-host rollback is NOT protected",
                flush=True,
            )
            server.serve_forever()
    except Rejected as error:
        print(f"qemu-dev-time: startup rejected: {error}", file=sys.stderr)
        return 1
    except (OSError, sqlite3.Error):
        print("qemu-dev-time: startup failed: state unavailable/invalid or loopback port unavailable; existing state was not reset", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
