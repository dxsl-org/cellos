"""Strict API Gateway payload-v2 boundary for signed-time Lambda."""

from base64 import b64decode, b64encode
from collections.abc import Callable, Mapping
from typing import Any

from protocol_models import MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES
from runtime_composition import compose_runtime

_CONTENT_TYPE = "application/cellos-signed-time+cbor"
_REQUEST_PATH = "/v1/time"


def _failure(status: int) -> dict[str, Any]:
    return {
        "statusCode": status,
        "headers": {"cache-control": "no-store", "content-length": "0"},
        "body": "",
        "isBase64Encoded": False,
    }


def _request_bytes(event: Any) -> bytes:
    if type(event) is not dict or event.get("version") != "2.0":
        raise ValueError("invalid API Gateway event")
    context = event.get("requestContext")
    http = context.get("http") if isinstance(context, Mapping) else None
    if (
        not isinstance(http, Mapping)
        or http.get("method") != "POST"
        or event.get("rawPath") != _REQUEST_PATH
        or event.get("isBase64Encoded") is not True
    ):
        raise ValueError("invalid request target")
    headers = event.get("headers")
    if not isinstance(headers, Mapping):
        raise ValueError("invalid request headers")
    normalized: dict[str, Any] = {}
    for name, value in headers.items():
        if type(name) is not str or name.lower() in normalized:
            raise ValueError("invalid request header names")
        normalized[name.lower()] = value
    if normalized.get("content-type") != _CONTENT_TYPE:
        raise ValueError("invalid request content type")
    body = event.get("body")
    if type(body) is not str or len(body) > 4 * ((MAX_REQUEST_BYTES + 2) // 3):
        raise ValueError("invalid request body")
    data = b64decode(body, validate=True)
    if not data or len(data) > MAX_REQUEST_BYTES or b64encode(data).decode("ascii") != body:
        raise ValueError("invalid request encoding")
    return data


class LambdaEntrypoint:
    """Lazily compose and cache one immutable signed-time runtime graph."""

    __slots__ = ("_runtime", "_runtime_factory")

    def __init__(self, runtime_factory: Callable[[], Any]) -> None:
        """Store one callable runtime factory; invalid factories raise ``TypeError``."""
        if not callable(runtime_factory):
            raise TypeError("runtime factory is not callable")
        self._runtime_factory = runtime_factory
        self._runtime = None

    def handle(self, event: Any, _context: Any) -> dict[str, Any]:
        """Return bounded CBOR success or an empty stable HTTP failure response."""
        try:
            request = _request_bytes(event)
        except Exception:
            return _failure(400)
        try:
            if self._runtime is None:
                runtime = self._runtime_factory()
                if not callable(getattr(runtime, "handle", None)):
                    raise TypeError("runtime has no callable handler")
                self._runtime = runtime
            response = self._runtime.handle(request)
            if type(response) is not bytes or not 0 < len(response) <= MAX_RESPONSE_BYTES:
                raise TypeError("runtime returned an invalid response")
            return {
                "statusCode": 200,
                "headers": {"cache-control": "no-store", "content-type": _CONTENT_TYPE},
                "body": b64encode(response).decode("ascii"),
                "isBase64Encoded": True,
            }
        except Exception:
            return _failure(503)


_ENTRYPOINT = LambdaEntrypoint(compose_runtime)


def lambda_handler(event: Any, context: Any) -> dict[str, Any]:
    """AWS Lambda entrypoint; ``context`` is accepted but never trusted."""
    return _ENTRYPOINT.handle(event, context)
