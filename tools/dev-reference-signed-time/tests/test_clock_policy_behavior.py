import unittest
from dataclasses import FrozenInstanceError, replace
import path_bootstrap

from allocation import AdmittedSample
from clock_policy import (
    ClockPolicy, ClockPolicyError, ProviderTimeObservation,
    admit_time_observation,
)
from protocol_models import MAX_UINT64


class ClockPolicyBehaviorTests(unittest.TestCase):
    def setUp(self):
        self.policy = ClockPolicy("provider.example", 7, 10, 10)
        self.observation = ProviderTimeObservation(
            "provider.example", 7, 100, 110, 111, 10, 10,
        )

    def admit(self, *, policy=None, observation=None, floor=100):
        return admit_time_observation(
            self.policy if policy is None else policy,
            self.observation if observation is None else observation,
            floor,
        )

    def assert_error(self, code, **changes):
        with self.assertRaises(ClockPolicyError) as caught:
            self.admit(**changes)
        error = caught.exception
        self.assertEqual((str(error), error.code, error.args), (code, code, (code,)))
        self.assertIsNone(error.__cause__)
        self.assertIsNone(error.__context__)

    def test_exact_success_returns_frozen_admitted_sample_without_rescaling(self):
        result = self.admit()
        self.assertIs(type(result), AdmittedSample)
        self.assertEqual(result, AdmittedSample(100, 110, 111))
        with self.assertRaises(FrozenInstanceError):
            result.sample_floor = 99
        for value in (self.policy, self.observation):
            with self.assertRaises(FrozenInstanceError):
                value.source_epoch = 8

    def test_age_and_provider_uncertainty_limits_are_inclusive(self):
        self.assertEqual(self.admit(), AdmittedSample(100, 110, 111))
        self.assert_error(
            "sample-too-old",
            observation=replace(self.observation, sample_age_seconds=11),
        )
        self.assert_error(
            "uncertainty-too-large",
            policy=replace(self.policy, max_uncertainty_seconds=9),
        )
        self.assert_error(
            "uncertainty-mismatch",
            observation=replace(self.observation, uncertainty_seconds=9),
        )
        self.assertEqual(
            self.admit(
                policy=replace(self.policy, max_uncertainty_seconds=11),
                observation=replace(self.observation, uncertainty_seconds=11),
            ),
            AdmittedSample(100, 110, 111),
        )

    def test_identity_and_epoch_must_match_exactly(self):
        self.assert_error(
            "upstream-identity-mismatch",
            observation=replace(self.observation, upstream_identity="other.example"),
        )
        self.assert_error(
            "source-epoch-mismatch",
            observation=replace(self.observation, source_epoch=8),
        )

    def test_reversed_zero_width_and_full_uint64_intervals(self):
        self.assert_error(
            "reversed-sample-interval",
            observation=replace(
                self.observation, sample_floor=111, sample_ceiling=110,
                sample_valid_until=112, uncertainty_seconds=0,
            ),
            floor=111,
        )
        zero_width = replace(
            self.observation, sample_ceiling=100, sample_valid_until=101,
            uncertainty_seconds=0,
        )
        self.assertEqual(self.admit(observation=zero_width), AdmittedSample(100, 100, 101))
        maximum = ProviderTimeObservation(
            "provider.example", MAX_UINT64, 0, MAX_UINT64, MAX_UINT64,
            MAX_UINT64, MAX_UINT64,
        )
        policy = ClockPolicy(
            "provider.example", MAX_UINT64, MAX_UINT64, MAX_UINT64,
        )
        self.assertEqual(
            self.admit(policy=policy, observation=maximum, floor=MAX_UINT64),
            AdmittedSample(0, MAX_UINT64, MAX_UINT64),
        )

    def test_valid_until_must_be_strictly_above_sample_floor(self):
        for valid_until in (99, 100):
            with self.subTest(valid_until=valid_until):
                self.assert_error(
                    "invalid-sample-valid-until",
                    observation=replace(
                        self.observation, sample_valid_until=valid_until,
                    ),
                )
        admitted = self.admit(
            observation=replace(self.observation, sample_valid_until=101),
        )
        self.assertEqual(admitted.sample_valid_until, 101)

    def test_protected_floor_accepts_endpoints_and_rejects_outside(self):
        for floor in (100, 110):
            with self.subTest(floor=floor):
                self.assertEqual(self.admit(floor=floor), AdmittedSample(100, 110, 111))
        self.assert_error("protected-floor-outside-sample", floor=99)
        self.assert_error("protected-floor-outside-sample", floor=111)

    def test_repeated_calls_are_independent_without_cache_or_holdover(self):
        first = self.admit()
        self.assert_error("sample-too-old", observation=replace(self.observation, sample_age_seconds=11))
        second_observation = ProviderTimeObservation(
            "provider.example", 7, 200, 205, 201, 1, 5,
        )
        second = self.admit(observation=second_observation, floor=203)
        self.assertEqual((first, second), (
            AdmittedSample(100, 110, 111), AdmittedSample(200, 205, 201),
        ))
        self.assertIsNot(first, second)
