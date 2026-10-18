#!/usr/bin/env python3
"""Offline host companion regressions; no upstream network or host-clock authority."""
import concurrent.futures
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import socket
import sqlite3
import threading
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

SPEC = importlib.util.spec_from_file_location("qemu_dev_time", Path(__file__).with_name("qemu-dev-time.py"))
assert SPEC is not None and SPEC.loader is not None
TIME = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TIME)


class AuthenticationTests(unittest.TestCase):
    def fetch(self, midpoint=100, radius=3, times=(10.0, 10.1)):
        with (
            patch.object(TIME.os, "urandom", return_value=b"n" * 32),
            patch.object(TIME, "build_request", return_value=b"request"),
            patch.object(TIME, "udp_exchange", return_value=b"packet"),
            patch.object(TIME, "verify_response", return_value=SimpleNamespace(midpoint=midpoint, radius=radius)),
            patch.object(TIME.time, "monotonic", side_effect=times),
        ):
            result = TIME.authenticated_sample()
        return result

    def test_open_interval_and_no_host_wallclock(self):
        with patch.object(TIME.time, "time", side_effect=AssertionError("wallclock is forbidden")):
            self.assertEqual(self.fetch(), (100, 98, 102))
            self.assertEqual(self.fetch(times=(10, 12)), (100, 98, 102))

    def test_bad_elapsed_uncertainty_and_arithmetic_fail(self):
        for times in ((10, 12.001), (10, 9), (10, float("nan")), (10, float("inf"))):
            with self.subTest(times=times), self.assertRaises(TIME.Rejected):
                self.fetch(times=times)
        for midpoint, radius in ((100, 6), (100, 0), (2, 3), (TIME.MAX_UINT64, 3)):
            with self.subTest(midpoint=midpoint, radius=radius), self.assertRaises(TIME.Rejected):
                self.fetch(midpoint, radius)

    def test_real_pinned_signature_vector_and_tampering(self):
        # Deliberately fixed NONCE/captured packet are confined to this offline
        # signature regression, never an admissible runtime freshness fallback.
        path = TIME.SOURCE.parent / "vectors/txryan-roughtime-draft11.json"
        vector = json.loads(path.read_text())
        nonce, packet = bytes.fromhex(vector["nonce"]), bytes.fromhex(vector["reply"])
        for response, valid in ((packet, True), (packet[:-1] + bytes([packet[-1] ^ 1]), False)):
            with (
                patch.object(TIME.os, "urandom", return_value=nonce),
                patch.object(TIME, "udp_exchange", return_value=response) as exchange,
                patch.object(TIME.time, "monotonic", return_value=1.0),
            ):
                if valid:
                    self.assertEqual(TIME.authenticated_sample(), (vector["midpoint"], vector["midpoint"] - 2, vector["midpoint"] + 2))
                else:
                    with self.assertRaises(TIME.Rejected):
                        TIME.authenticated_sample()
                self.assertEqual(exchange.call_count, 1 if valid else len(TIME.APPROVED_UPSTREAMS))

    def test_upstream_outage_has_no_retry_or_secret_error(self):
        with (
            patch.object(TIME, "udp_exchange", side_effect=OSError("secret")) as exchange,
            self.assertRaises(TIME.Rejected) as caught,
        ):
            TIME.authenticated_sample()
        self.assertEqual(exchange.call_count, len(TIME.APPROVED_UPSTREAMS))
        self.assertNotIn("secret", str(caught.exception))

    def test_failover_to_secondary_source_on_primary_failure(self):
        with (
            patch.object(TIME.os, "urandom", return_value=b"n" * 32),
            patch.object(TIME, "build_request", return_value=b"request"),
            patch.object(TIME, "udp_exchange", side_effect=[OSError("primary down"), b"packet2"]) as mock_exchange,
            patch.object(TIME, "verify_response", return_value=SimpleNamespace(midpoint=200, radius=3)),
            patch.object(TIME.time, "monotonic", return_value=1.0),
        ):
            sample = TIME.authenticated_sample()
            self.assertEqual(sample, (200, 198, 202))
            self.assertEqual(mock_exchange.call_count, 2)

class StateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "time.sqlite3"
        self.store = TIME.StateStore(self.path)

    def state(self):
        with contextlib.closing(self.store.connect()) as connection:
            return self.store.read(connection)

    def test_floor_sequence_durable_across_restart_and_equal_midpoint(self):
        with patch.object(TIME, "authenticated_sample", return_value=(100, 98, 102)):
            self.assertEqual(self.store.accept(), (100, 98, 102, 1))
            self.store = TIME.StateStore(self.path)
            self.assertEqual(self.store.accept(), (100, 98, 102, 2))
        with patch.object(TIME, "authenticated_sample", return_value=(99, 97, 101)):
            self.assertEqual(self.store.accept(), (100, 97, 101, 3))
        self.assertEqual(self.state(), (100, 3))
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o600)

    def test_rollback_outage_and_commit_failure_never_advance(self):
        with patch.object(TIME, "authenticated_sample", return_value=(100, 98, 102)):
            self.store.accept()
        for sample in (Mock(return_value=(97, 95, 99)), Mock(side_effect=TIME.Rejected("outage"))):
            with patch.object(TIME, "authenticated_sample", sample), self.assertRaises(TIME.Rejected):
                self.store.accept()
            self.assertEqual(self.state(), (100, 1))
        connection = self.store.connect()
        failing = Mock(wraps=connection)
        failing.commit.side_effect = sqlite3.OperationalError("disk failed")
        with (
            patch.object(self.store, "connect", return_value=failing),
            patch.object(TIME, "authenticated_sample", return_value=(101, 99, 103)),
            self.assertRaises(sqlite3.Error),
        ):
            self.store.accept()
        self.assertEqual(self.state(), (100, 1))

    def test_concurrent_requests_have_unique_durable_sequences(self):
        with (
            patch.object(TIME, "authenticated_sample", return_value=(100, 98, 102)) as fetch,
            concurrent.futures.ThreadPoolExecutor(max_workers=8) as workers,
        ):
            results = list(workers.map(lambda _: self.store.accept(), range(8)))
        self.assertEqual(sorted(result[3] for result in results), list(range(1, 9)))
        self.assertEqual(fetch.call_count, 8)
        self.assertEqual(self.state(), (100, 8))

    def test_existing_empty_corrupt_unknown_version_and_missing_row_fail_closed(self):
        for contents in (b"", b"not a database"):
            path = Path(self.directory.name) / ("bad-empty" if not contents else "bad-corrupt")
            path.write_bytes(contents)
            path.chmod(0o600)
            with self.assertRaises((TIME.Rejected, sqlite3.Error)):
                TIME.StateStore(path)
            self.assertEqual(path.read_bytes(), contents)
        with contextlib.closing(self.store.connect()) as connection:
            connection.execute("PRAGMA user_version = 2")
        with self.assertRaises(TIME.Rejected):
            TIME.StateStore(self.path)
        with contextlib.closing(self.store.connect()) as connection:
            connection.execute("PRAGMA user_version = 1")
            connection.execute("DELETE FROM time_state")
        with self.assertRaises(TIME.Rejected):
            TIME.StateStore(self.path)

    def test_disappeared_state_does_not_reinitialize(self):
        self.path.unlink()
        with self.assertRaises(sqlite3.Error):
            self.store.accept()
        self.assertFalse(self.path.exists())

    def test_sequence_exhaustion_is_not_wrapped(self):
        with contextlib.closing(self.store.connect()) as connection:
            connection.execute("UPDATE time_state SET floor = ?, sequence = ?", ((100).to_bytes(8, "little"), TIME.MAX_UINT64.to_bytes(8, "little")))
        with patch.object(TIME, "authenticated_sample", return_value=(100, 98, 102)), self.assertRaises(TIME.Rejected):
            self.store.accept()
        self.assertEqual(self.state(), (100, TIME.MAX_UINT64))


class ProtocolTests(unittest.TestCase):
    def test_fragmented_request_and_rejected_framing(self):
        channel = Mock()
        channel.recv.side_effect = [TIME.MAGIC[:3], TIME.MAGIC[3:] + b"n" * 32]
        self.assertEqual(TIME.read_request(channel), b"n" * 32)
        for message in (b"", b"short", b"WRONG001" + bytes(32), TIME.MAGIC + bytes(33)):
            channel = Mock()
            channel.recv.side_effect = [message, b""]
            with self.subTest(message=message), self.assertRaises(TIME.Rejected):
                TIME.read_request(channel)

    def test_slow_drip_deadline_is_total_not_per_read(self):
        channel = Mock()
        channel.recv.return_value = b"C"
        with patch.object(TIME.time, "monotonic", side_effect=(1, 1, 3.01)), self.assertRaises(TIME.Rejected):
            TIME.read_request(channel)
        channel.recv.assert_called_once()

    def test_loopback_response_after_commit_and_failures_close_without_success(self):
        with tempfile.TemporaryDirectory() as directory:
            store = TIME.StateStore(Path(directory) / "time.sqlite3")
            with TIME.TimeServer(0, store) as server:
                self.assertEqual(server.server_address[0], "127.0.0.1")
                runner = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True)
                runner.start()
                try:
                    with patch.object(TIME, "authenticated_sample", return_value=(100, 98, 102)):
                        with socket.create_connection(server.server_address, timeout=2) as channel:
                            channel.sendall(TIME.MAGIC + b"n" * 32)
                            response = bytearray()
                            while len(response) < TIME.RESPONSE.size:
                                chunk = channel.recv(TIME.RESPONSE.size - len(response))
                                if not chunk:
                                    break
                                response.extend(chunk)
                            self.assertEqual(len(response), 72)
                            self.assertEqual(TIME.RESPONSE.unpack(response), (TIME.MAGIC, b"n" * 32, 100, 98, 102, 1))
                            with contextlib.closing(store.connect()) as connection:
                                self.assertEqual(store.read(connection), (100, 1))
                            self.assertEqual(channel.recv(1), b"")
                    with patch.object(TIME, "authenticated_sample", side_effect=TIME.Rejected("authentication failed")), contextlib.redirect_stderr(io.StringIO()):
                        with socket.create_connection(server.server_address, timeout=2) as channel:
                            channel.sendall(TIME.MAGIC + b"n" * 32)
                            self.assertEqual(channel.recv(72), b"")
                finally:
                    server.shutdown()
                    runner.join(timeout=2)


if __name__ == "__main__":
    unittest.main()
