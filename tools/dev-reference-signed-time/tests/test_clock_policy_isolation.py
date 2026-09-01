import builtins
import datetime
import http.client
import socket
import sys
import time
import types
import unittest
import urllib.request
from unittest import mock
import path_bootstrap

from allocation import AdmittedSample
from clock_policy import (
    ClockPolicy, ClockPolicyError, ProviderTimeObservation,
    admit_time_observation,
)


def forbidden(*args, **kwargs):
    raise AssertionError("ambient clock or I/O access")


class ForbiddenDateTime:
    now = classmethod(forbidden)
    utcnow = classmethod(forbidden)
    fromtimestamp = classmethod(forbidden)


class ForbiddenDate:
    today = classmethod(forbidden)


class ClockPolicyIsolationTests(unittest.TestCase):
    def test_admission_uses_no_time_datetime_socket_url_or_aws_access(self):
        policy = ClockPolicy("provider.example", 7, 10, 10)
        observation = ProviderTimeObservation(
            "provider.example", 7, 100, 110, 111, 10, 10,
        )
        real_import = builtins.__import__

        def guarded_import(name, *args, **kwargs):
            if name.split(".", 1)[0] in {
                "boto3", "botocore", "requests", "urllib3",
            }:
                return forbidden(name)
            return real_import(name, *args, **kwargs)

        fake_boto3 = types.SimpleNamespace(
            client=forbidden, resource=forbidden, Session=forbidden,
        )
        patches = (
            mock.patch.object(time, "time", forbidden),
            mock.patch.object(time, "monotonic", forbidden),
            mock.patch.object(time, "monotonic_ns", forbidden),
            mock.patch.object(datetime, "datetime", ForbiddenDateTime),
            mock.patch.object(datetime, "date", ForbiddenDate),
            mock.patch.object(socket, "socket", forbidden),
            mock.patch.object(socket, "create_connection", forbidden),
            mock.patch.object(socket, "getaddrinfo", forbidden),
            mock.patch.object(urllib.request, "urlopen", forbidden),
            mock.patch.object(urllib.request, "urlretrieve", forbidden),
            mock.patch.object(http.client, "HTTPConnection", forbidden),
            mock.patch.object(http.client, "HTTPSConnection", forbidden),
            mock.patch.object(builtins, "__import__", guarded_import),
            mock.patch.dict(sys.modules, {"boto3": fake_boto3}),
        )
        with patches[0], patches[1], patches[2], patches[3], patches[4], \
                patches[5], patches[6], patches[7], patches[8], patches[9], \
                patches[10], patches[11], patches[12], patches[13]:
            first = admit_time_observation(policy, observation, 100)
            with self.assertRaises(ClockPolicyError) as caught:
                admit_time_observation(
                    policy,
                    ProviderTimeObservation(
                        "provider.example", 7, 100, 110, 111, 11, 10,
                    ),
                    100,
                )
            second = admit_time_observation(
                policy,
                ProviderTimeObservation(
                    "provider.example", 7, 200, 210, 201, 0, 10,
                ),
                210,
            )
        self.assertEqual(first, AdmittedSample(100, 110, 111))
        self.assertEqual(caught.exception.code, "sample-too-old")
        self.assertEqual(second, AdmittedSample(200, 210, 201))
