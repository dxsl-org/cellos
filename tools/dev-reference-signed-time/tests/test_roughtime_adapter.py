import socket
import unittest
from unittest.mock import Mock, patch

import path_bootstrap
from allocation import AdmittedSample
from clock_policy import ClockPolicy
from protocol_models import MAX_UINT64
from roughtime_adapter import (
    RoughtimeAdapterError, RoughtimeClockAdapter, RoughtimeTransportError, udp_exchange,
)
from roughtime_config import provider_config
from roughtime_verify import build_request
from roughtime_vector_support import (
    CONFIG, NONCE, ResponseOptions, exact_request, response_packet,
)

class FakeSocket:
    def __init__(self, response=b"reply", flags=0, failure=None, sent=None):
        self.response = response
        self.flags = flags
        self.failure = failure
        self.sent = sent
        self.calls = []

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.calls.append(("close",))

    def settimeout(self, value):
        self.calls.append(("timeout", value))

    def connect(self, address):
        self.calls.append(("connect", address))
        if self.failure:
            raise self.failure

    def send(self, request):
        self.calls.append(("send", request))
        return len(request) if self.sent is None else self.sent

    def recvmsg(self, size):
        self.calls.append(("recv", size))
        return self.response, [], self.flags, None

class RoughtimeTransportTests(unittest.TestCase):
    def request(self):
        return build_request(NONCE)

    def test_resolves_once_selects_first_address_sends_once_and_receives_once(self):
        channel = FakeSocket()
        addresses = [
            (socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP, "", ("192.0.2.1", 2003)),
            (socket.AF_INET6, socket.SOCK_DGRAM, socket.IPPROTO_UDP, "", ("::1", 2003, 0, 0)),
        ]
        with (
            patch("roughtime_adapter.socket.getaddrinfo", return_value=addresses) as resolve,
            patch("roughtime_adapter.socket.socket", return_value=channel) as create,
        ):
            self.assertEqual(udp_exchange(provider_config(), self.request()), b"reply")
        resolve.assert_called_once_with(
            "roughtime.cloudflare.com", 2003, family=socket.AF_UNSPEC,
            type=socket.SOCK_DGRAM, proto=socket.IPPROTO_UDP,
        )
        create.assert_called_once_with(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
        self.assertEqual(channel.calls[0:2], [
            ("timeout", 2.0), ("connect", ("192.0.2.1", 2003)),
        ])
        self.assertEqual(sum(entry[0] == "send" for entry in channel.calls), 1)
        self.assertEqual(sum(entry[0] == "recv" for entry in channel.calls), 1)

    def test_first_address_failure_never_tries_second(self):
        channel = FakeSocket(failure=OSError("secret"))
        addresses = [
            (socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP, "", ("192.0.2.1", 2003)),
            (socket.AF_INET6, socket.SOCK_DGRAM, socket.IPPROTO_UDP, "", ("::1", 2003, 0, 0)),
        ]
        with (
            patch("roughtime_adapter.socket.getaddrinfo", return_value=addresses) as resolve,
            patch("roughtime_adapter.socket.socket", return_value=channel) as create,
            self.assertRaises(RoughtimeTransportError) as caught,
        ):
            udp_exchange(provider_config(), self.request())
        resolve.assert_called_once()
        create.assert_called_once()
        self.assertEqual(str(caught.exception), "roughtime transport failed")
        self.assertIsNone(caught.exception.__cause__)

    def test_timeout_dns_failure_truncation_and_oversize_are_stable(self):
        request = self.request()
        cases = (
            ([], FakeSocket(), None),
            (None, FakeSocket(failure=socket.timeout("secret")), None),
            (None, FakeSocket(flags=socket.MSG_TRUNC), None),
            (None, FakeSocket(response=b"x" * (len(request) + 1)), None),
            (None, FakeSocket(sent=0), None),
            (None, FakeSocket(), OSError("dns secret")),
        )
        address = [(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP, "", ("192.0.2.1", 2003))]
        for resolved, channel, error in cases:
            with self.subTest(resolved=resolved, flags=channel.flags):
                resolver = Mock(side_effect=error) if error else Mock(
                    return_value=address if resolved is None else resolved,
                )
                with (
                    patch("roughtime_adapter.socket.getaddrinfo", resolver),
                    patch("roughtime_adapter.socket.socket", return_value=channel),
                    self.assertRaises(RoughtimeTransportError) as caught,
                ):
                    udp_exchange(provider_config(), request)
                self.assertEqual(str(caught.exception), "roughtime transport failed")
                self.assertNotIn("secret", str(caught.exception))

class RoughtimeClockAdapterTests(unittest.TestCase):
    def policy(self, uncertainty=6):
        return ClockPolicy("roughtime.cloudflare.com", 7, 2, uncertainty)

    def run_adapter(
        self, options=ResponseOptions(), *, policy=None, times=(10.0, 10.1),
        floor=None,
    ):
        request = exact_request()
        response = response_packet(options, request)
        exchange = Mock(return_value=response)
        with (
            patch("roughtime_adapter.validate_provider_config"),
            patch("roughtime_verify.validate_provider_config"),
            patch("roughtime_adapter.os.urandom", return_value=NONCE),
            patch("roughtime_adapter.time.monotonic", side_effect=times),
        ):
            adapter = RoughtimeClockAdapter(policy or self.policy(), CONFIG, exchange)
            admitted_floor = (
                options.midpoint - options.radius + 1 if floor is None else floor
            )
            result = adapter(admitted_floor)
        exchange.assert_called_once_with(CONFIG, request)
        return result

    def test_signed_open_interval_maps_to_closed_inner_seconds(self):
        self.assertEqual(
            self.run_adapter(),
            AdmittedSample(1_699_999_998, 1_700_000_002, 1_700_000_003),
        )
        for excluded in (1_699_999_997, 1_700_000_003):
            with self.subTest(excluded=excluded), self.assertRaises(RoughtimeAdapterError):
                self.run_adapter(floor=excluded)
        with self.assertRaises(RoughtimeAdapterError):
            self.run_adapter(policy=self.policy(5))

    def test_elapsed_monotonic_time_is_rounded_up_as_sample_age(self):
        result = self.run_adapter(times=(10.0, 11.01))
        self.assertEqual(result.sample_floor, 1_699_999_998)
        with self.assertRaises(RoughtimeAdapterError):
            self.run_adapter(times=(11.0, 10.0))
        with self.assertRaises(RoughtimeAdapterError):
            self.run_adapter(times=(10.0, float("nan")))

    def test_interval_underflow_and_overflow_fail(self):
        cases = (
            ResponseOptions(midpoint=2, radius=3, minimum=0, maximum=5),
            ResponseOptions(
                midpoint=MAX_UINT64, radius=3, minimum=0, maximum=MAX_UINT64,
            ),
        )
        for options in cases:
            with self.subTest(options=options), self.assertRaises(RoughtimeAdapterError):
                self.run_adapter(options)

    def test_policy_is_reused_for_uncertainty_floor_and_transport_errors(self):
        with self.assertRaises(RoughtimeAdapterError):
            self.run_adapter(policy=self.policy(uncertainty=5))
        with self.assertRaises(RoughtimeAdapterError):
            self.run_adapter(floor=1_700_000_003)
        request = exact_request()
        exchange = Mock(side_effect=socket.timeout("secret"))
        with (
            patch("roughtime_adapter.validate_provider_config"),
            patch("roughtime_verify.validate_provider_config"),
            patch("roughtime_adapter.os.urandom", return_value=NONCE),
            patch("roughtime_adapter.time.monotonic", return_value=10.0),
            self.assertRaises(RoughtimeAdapterError) as caught,
        ):
            RoughtimeClockAdapter(self.policy(), CONFIG, exchange)(1_699_999_997)
        exchange.assert_called_once_with(CONFIG, request)
        self.assertEqual(str(caught.exception), "roughtime adapter failed")
        self.assertNotIn("secret", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
