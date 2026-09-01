from base64 import b64encode
from copy import deepcopy
import unittest

import path_bootstrap  # noqa: F401

from lambda_entrypoint import LambdaEntrypoint
from protocol_models import MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES

CONTENT_TYPE = "application/cellos-signed-time+cbor"


def event(data=b"request"):
    return {
        "version": "2.0",
        "rawPath": "/v1/time",
        "headers": {"content-type": CONTENT_TYPE},
        "requestContext": {"http": {"method": "POST"}},
        "body": b64encode(data).decode("ascii"),
        "isBase64Encoded": True,
    }


class Runtime:
    def __init__(self, response=b"response", error=None):
        self.response = response
        self.error = error
        self.calls = []

    def handle(self, data):
        self.calls.append(data)
        if self.error is not None:
            raise self.error
        return self.response


class LambdaEntrypointTests(unittest.TestCase):
    def test_lazily_caches_runtime_and_returns_base64_cbor(self):
        runtime = Runtime()
        factory_calls = []

        def factory():
            factory_calls.append(None)
            return runtime

        entrypoint = LambdaEntrypoint(factory)
        expected = {
            "statusCode": 200,
            "headers": {"cache-control": "no-store", "content-type": CONTENT_TYPE},
            "body": b64encode(b"response").decode("ascii"),
            "isBase64Encoded": True,
        }
        self.assertEqual(entrypoint.handle(event(), object()), expected)
        self.assertEqual(entrypoint.handle(event(b"second"), None), expected)
        self.assertEqual(len(factory_calls), 1)
        self.assertEqual(runtime.calls, [b"request", b"second"])

    def test_invalid_http_boundaries_return_empty_400_before_composition(self):
        cases = []
        for path, value in (
            (("version",), "1.0"),
            (("rawPath",), "/wrong"),
            (("isBase64Encoded",), False),
            (("requestContext", "http", "method"), "GET"),
            (("headers", "content-type"), CONTENT_TYPE + "; charset=utf-8"),
            (("body",), "not base64"),
            (("body",), b64encode(b"x" * (MAX_REQUEST_BYTES + 1)).decode("ascii")),
            (("body",), ""),
        ):
            candidate = deepcopy(event())
            target = candidate
            for name in path[:-1]:
                target = target[name]
            target[path[-1]] = value
            cases.append(candidate)
        duplicate = event()
        duplicate["headers"]["Content-Type"] = CONTENT_TYPE
        cases.append(duplicate)

        def forbidden_factory():
            raise AssertionError("invalid request reached composition")

        entrypoint = LambdaEntrypoint(forbidden_factory)
        for index, candidate in enumerate(cases):
            with self.subTest(index=index):
                result = entrypoint.handle(candidate, None)
                self.assertEqual(result["statusCode"], 400)
                self.assertEqual(result["body"], "")
                self.assertFalse(result["isBase64Encoded"])

    def test_runtime_failures_and_invalid_results_return_empty_503(self):
        cases = (
            lambda: (_ for _ in ()).throw(OSError("cold start detail")),
            lambda: object(),
            lambda: Runtime(error=ValueError("handler detail")),
            lambda: Runtime(response="not bytes"),
            lambda: Runtime(response=b"x" * (MAX_RESPONSE_BYTES + 1)),
        )
        for index, factory in enumerate(cases):
            with self.subTest(index=index):
                result = LambdaEntrypoint(factory).handle(event(), None)
                self.assertEqual(result["statusCode"], 503)
                self.assertEqual(result["body"], "")
                self.assertFalse(result["isBase64Encoded"])

    def test_rejects_noncallable_runtime_factory(self):
        with self.assertRaises(TypeError):
            LambdaEntrypoint(None)


if __name__ == "__main__":
    unittest.main()
