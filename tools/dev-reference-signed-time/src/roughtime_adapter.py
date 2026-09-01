"""Synchronous one-datagram Cloudflare Roughtime clock adapter."""

import math
import os
import socket
import time
from typing import Callable, NoReturn

from allocation import AdmittedSample
from clock_policy import (
    ClockPolicy, ProviderTimeObservation, admit_time_observation,
)
from protocol_models import MAX_UINT64
from roughtime_config import (
    RoughtimeProviderConfig, provider_config, validate_provider_config,
)
from roughtime_verify import build_request, verify_response

_ERROR = "roughtime adapter failed"
_TRANSPORT_ERROR = "roughtime transport failed"


class RoughtimeAdapterError(RuntimeError):
    __slots__ = ()


class RoughtimeTransportError(RuntimeError):
    __slots__ = ()


def _fail() -> NoReturn:
    raise RoughtimeAdapterError(_ERROR) from None


def _transport_fail() -> NoReturn:
    raise RoughtimeTransportError(_TRANSPORT_ERROR) from None


def udp_exchange(config: RoughtimeProviderConfig, request: bytes) -> bytes:
    """Resolve once and perform exactly one connected UDP send and receive."""
    failed = False
    response = b""
    try:
        validate_provider_config(config)
        if type(request) is not bytes or len(request) != config.max_packet_bytes:
            _transport_fail()
        addresses = socket.getaddrinfo(
            config.host,
            config.port,
            family=socket.AF_UNSPEC,
            type=socket.SOCK_DGRAM,
            proto=socket.IPPROTO_UDP,
        )
        family, socktype, protocol, _name, address = addresses[0]
        if (
            family not in (socket.AF_INET, socket.AF_INET6)
            or socktype != socket.SOCK_DGRAM
            or protocol != socket.IPPROTO_UDP
        ):
            _transport_fail()
        with socket.socket(family, socket.SOCK_DGRAM, socket.IPPROTO_UDP) as channel:
            channel.settimeout(config.timeout_milliseconds / 1000)
            channel.connect(address)
            if channel.send(request) != len(request):
                _transport_fail()
            response, _ancillary, flags, _peer = channel.recvmsg(
                config.max_packet_bytes,
            )
        if flags & socket.MSG_TRUNC or len(response) > len(request):
            _transport_fail()
        return response
    except RoughtimeTransportError:
        raise
    except (IndexError, OSError, TypeError, ValueError):
        failed = True
    if failed:
        _transport_fail()
    return response


Exchange = Callable[[RoughtimeProviderConfig, bytes], bytes]


class RoughtimeClockAdapter:
    """Callable admitted-sample loader for ``SignedTimeHandler``."""

    __slots__ = ("_config", "_exchange", "_policy")

    def __init__(
        self,
        policy: ClockPolicy,
        config: RoughtimeProviderConfig | None = None,
        exchange: Exchange = udp_exchange,
    ) -> None:
        selected = provider_config() if config is None else config
        failed = False
        try:
            validate_provider_config(selected)
            if type(policy) is not ClockPolicy or not callable(exchange):
                failed = True
        except Exception:
            failed = True
        if failed:
            _fail()
        self._policy = policy
        self._config = selected
        self._exchange = exchange

    def __call__(self, protected_server_floor: int) -> AdmittedSample:
        failed = False
        result = None
        try:
            nonce = os.urandom(32)
            request = build_request(nonce, self._config)
            started = time.monotonic()
            response = self._exchange(self._config, request)
            verified = verify_response(response, request, nonce, self._config)
            finished = time.monotonic()
            if (
                type(started) not in (int, float)
                or type(finished) not in (int, float)
                or not math.isfinite(started)
                or not math.isfinite(finished)
                or finished < started
            ):
                _fail()
            midpoint, radius = verified.midpoint, verified.radius
            if (
                midpoint < radius
                or radius > MAX_UINT64 // 2
                or radius > MAX_UINT64 - midpoint
            ):
                _fail()
            floor = midpoint - radius + 1
            upper = midpoint + radius
            ceiling = upper - 1
            uncertainty = 2 * radius
            age = math.ceil(finished - started)
            if not 0 <= age <= MAX_UINT64:
                _fail()
            observation = ProviderTimeObservation(
                upstream_identity=self._config.host,
                source_epoch=self._policy.source_epoch,
                sample_floor=floor,
                sample_ceiling=ceiling,
                sample_valid_until=upper,
                sample_age_seconds=age,
                uncertainty_seconds=uncertainty,
            )
            result = admit_time_observation(
                self._policy, observation, protected_server_floor,
            )
        except RoughtimeAdapterError:
            raise
        except Exception:
            failed = True
        if failed or type(result) is not AdmittedSample:
            _fail()
        return result
