import unittest
from dataclasses import replace
import path_bootstrap

from clock_policy import (
    ClockPolicy, ClockPolicyError, ProviderTimeObservation,
    admit_time_observation,
)
from protocol_models import MAX_UINT64


class IntChild(int):
    pass


class StrChild(str):
    pass


class ClockPolicyValidationTests(unittest.TestCase):
    def setUp(self):
        self.policy = ClockPolicy("provider.example", 7, 10, 10)
        self.observation = ProviderTimeObservation(
            "provider.example", 7, 100, 110, 111, 10, 10,
        )

    def assert_error(self, code, *, policy=None, observation=None, floor=100):
        with self.assertRaises(ClockPolicyError) as caught:
            admit_time_observation(
                self.policy if policy is None else policy,
                self.observation if observation is None else observation,
                floor,
            )
        error = caught.exception
        self.assertEqual(str(error), code)
        self.assertEqual(error.code, code)
        self.assertEqual(error.args, (code,))
        self.assertIsNone(error.__cause__)
        self.assertIsNone(error.__context__)

    def test_policy_and_observation_require_exact_container_types(self):
        class PolicyChild(ClockPolicy):
            pass

        class ObservationChild(ProviderTimeObservation):
            pass

        child_policy = PolicyChild("provider.example", 7, 10, 10)
        child_observation = ObservationChild(
            "provider.example", 7, 100, 110, 111, 10, 10,
        )
        for value in (object(), child_policy):
            with self.subTest(policy_type=type(value).__name__):
                self.assert_error("invalid-policy-type", policy=value)
        for value in (object(), child_observation):
            with self.subTest(observation_type=type(value).__name__):
                self.assert_error("invalid-observation-type", observation=value)
        with self.assertRaises(ClockPolicyError) as caught:
            admit_time_observation(None, self.observation, 100)
        self.assertEqual(caught.exception.code, "invalid-policy-type")
        with self.assertRaises(ClockPolicyError) as caught:
            admit_time_observation(self.policy, None, 100)
        self.assertEqual(caught.exception.code, "invalid-observation-type")

    def test_every_policy_uint_field_rejects_range_and_inexact_types(self):
        fields = {
            "source_epoch": "invalid-source-epoch",
            "max_sample_age_seconds": "invalid-policy-max-age",
            "max_uncertainty_seconds": "invalid-policy-max-uncertainty",
        }
        bad_values = (-1, MAX_UINT64 + 1, True, IntChild(1), None, 1.0, "1")
        for field, code in fields.items():
            for value in bad_values:
                with self.subTest(field=field, value=repr(value)):
                    self.assert_error(
                        code, policy=replace(self.policy, **{field: value}),
                    )

    def test_every_observation_uint_field_rejects_range_and_inexact_types(self):
        fields = {
            "source_epoch": "invalid-observation-source-epoch",
            "sample_floor": "invalid-observation-sample-floor",
            "sample_ceiling": "invalid-observation-sample-ceiling",
            "sample_valid_until": "invalid-observation-valid-until",
            "sample_age_seconds": "invalid-observation-age",
            "uncertainty_seconds": "invalid-observation-uncertainty",
        }
        bad_values = (-1, MAX_UINT64 + 1, True, IntChild(1), None, 1.0, "1")
        for field, code in fields.items():
            for value in bad_values:
                with self.subTest(field=field, value=repr(value)):
                    self.assert_error(
                        code,
                        observation=replace(self.observation, **{field: value}),
                    )

    def test_protected_floor_rejects_range_and_inexact_types(self):
        for value in (-1, MAX_UINT64 + 1, True, IntChild(1), None, 1.0, "1"):
            with self.subTest(value=repr(value)):
                self.assert_error("invalid-protected-floor", floor=value)

    def test_identities_require_nonempty_exact_strings(self):
        bad_values = (
            "", StrChild("provider.example"), -1, MAX_UINT64 + 1, True,
            IntChild(1), 1.0, b"provider.example", None,
        )
        for value in bad_values:
            with self.subTest(target="policy", value=repr(value)):
                self.assert_error(
                    "invalid-policy-identity",
                    policy=replace(self.policy, upstream_identity=value),
                )
            with self.subTest(target="observation", value=repr(value)):
                self.assert_error(
                    "invalid-observation-identity",
                    observation=replace(
                        self.observation, upstream_identity=value,
                    ),
                )

    def test_uint64_zero_and_max_are_valid_field_values_before_policy_checks(self):
        zero_policy = ClockPolicy("provider.example", 0, 0, 0)
        zero_observation = ProviderTimeObservation(
            "provider.example", 0, 0, 0, 1, 0, 0,
        )
        result = admit_time_observation(zero_policy, zero_observation, 0)
        self.assertEqual(
            (result.sample_floor, result.sample_ceiling, result.sample_valid_until),
            (0, 0, 1),
        )
        maximum_policy = ClockPolicy(
            "provider.example", MAX_UINT64, MAX_UINT64, MAX_UINT64,
        )
        maximum_observation = ProviderTimeObservation(
            "provider.example", MAX_UINT64, 0, MAX_UINT64, MAX_UINT64,
            MAX_UINT64, MAX_UINT64,
        )
        result = admit_time_observation(
            maximum_policy, maximum_observation, MAX_UINT64,
        )
        self.assertEqual(result.sample_ceiling, MAX_UINT64)
