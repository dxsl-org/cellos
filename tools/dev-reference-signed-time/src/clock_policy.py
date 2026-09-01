"""Stateless admission policy for provider-authenticated time observations.

This module does not authenticate observations.  Callers must supply observations
only from the future reviewed single-provider adapter after that adapter has
successfully authenticated the provider response and measured its age.
"""

from dataclasses import dataclass
from typing import Any, NoReturn

from allocation import AdmittedSample
from protocol_models import MAX_UINT64


class ClockPolicyError(ValueError):
    """Stable fail-closed error whose message is a value-free error code."""

    __slots__ = ("code",)

    def __init__(self, code: str) -> None:
        self.code = code
        super().__init__(code)


@dataclass(frozen=True, slots=True)
class ClockPolicy:
    """Pinned identity, epoch, freshness, and uncertainty admission limits."""

    upstream_identity: str
    source_epoch: int
    max_sample_age_seconds: int
    max_uncertainty_seconds: int


@dataclass(frozen=True, slots=True)
class ProviderTimeObservation:
    """Time data already authenticated and age-measured by the future adapter."""

    upstream_identity: str
    source_epoch: int
    sample_floor: int
    sample_ceiling: int
    sample_valid_until: int
    sample_age_seconds: int
    uncertainty_seconds: int


def _fail(code: str) -> NoReturn:
    raise ClockPolicyError(code)


def _require_uint64(value: Any, code: str) -> None:
    if type(value) is not int or not 0 <= value <= MAX_UINT64:
        _fail(code)


def _require_identity(value: Any, code: str) -> None:
    if type(value) is not str or not value:
        _fail(code)


def admit_time_observation(
    policy: ClockPolicy,
    observation: ProviderTimeObservation,
    protected_server_floor: int,
) -> AdmittedSample:
    """Admit one authenticated observation without ambient time, I/O, or state.

    This function performs policy arithmetic only; it does not authenticate the
    observation.  ``observation`` must come directly from the future reviewed
    single-provider adapter, which is responsible for authentication and age
    measurement.  Every invocation evaluates only its supplied arguments and
    neither caches nor continues a previously admitted sample.
    """
    if type(policy) is not ClockPolicy:
        _fail("invalid-policy-type")
    _require_identity(policy.upstream_identity, "invalid-policy-identity")
    _require_uint64(policy.source_epoch, "invalid-source-epoch")
    _require_uint64(policy.max_sample_age_seconds, "invalid-policy-max-age")
    _require_uint64(policy.max_uncertainty_seconds, "invalid-policy-max-uncertainty")

    if type(observation) is not ProviderTimeObservation:
        _fail("invalid-observation-type")
    _require_identity(observation.upstream_identity, "invalid-observation-identity")
    _require_uint64(observation.source_epoch, "invalid-observation-source-epoch")
    _require_uint64(observation.sample_floor, "invalid-observation-sample-floor")
    _require_uint64(observation.sample_ceiling, "invalid-observation-sample-ceiling")
    _require_uint64(observation.sample_valid_until, "invalid-observation-valid-until")
    _require_uint64(observation.sample_age_seconds, "invalid-observation-age")
    _require_uint64(observation.uncertainty_seconds, "invalid-observation-uncertainty")
    _require_uint64(protected_server_floor, "invalid-protected-floor")

    if observation.upstream_identity != policy.upstream_identity:
        _fail("upstream-identity-mismatch")
    if observation.source_epoch != policy.source_epoch:
        _fail("source-epoch-mismatch")
    if observation.sample_floor > observation.sample_ceiling:
        _fail("reversed-sample-interval")
    if observation.uncertainty_seconds != observation.sample_ceiling - observation.sample_floor:
        _fail("uncertainty-mismatch")
    if observation.sample_age_seconds > policy.max_sample_age_seconds:
        _fail("sample-too-old")
    if observation.uncertainty_seconds > policy.max_uncertainty_seconds:
        _fail("uncertainty-too-large")
    if observation.sample_valid_until <= observation.sample_floor:
        _fail("invalid-sample-valid-until")
    if not observation.sample_floor <= protected_server_floor <= observation.sample_ceiling:
        _fail("protected-floor-outside-sample")

    return AdmittedSample(
        sample_floor=observation.sample_floor,
        sample_ceiling=observation.sample_ceiling,
        sample_valid_until=observation.sample_valid_until,
    )
