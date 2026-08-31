"""Bounded non-authoritative host-clock sanity floor for runtime admission."""

import math
import time
from collections.abc import Callable

from protocol_models import MAX_UINT64

_ERROR = "host clock floor unavailable"


class RuntimeFloorError(RuntimeError):
    """Stable value-free failure at the host-clock sanity boundary."""


def load_host_time_floor(clock: Callable[[], float] = time.time) -> int:
    """Read once and return a bounded Unix-second floor or fail closed.

    The result may only deny an authenticated upstream interval; it is never a
    response-time source. ``clock`` is injectable for deterministic tests.
    """
    failed = False
    result = 0
    try:
        value = clock()
        if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
            raise TypeError("host clock did not return a nonnegative finite number")
        result = int(value)
        if not 0 <= result <= MAX_UINT64:
            raise ValueError("host clock is outside uint64 Unix seconds")
    except Exception:
        failed = True
    if failed:
        raise RuntimeFloorError(_ERROR) from None
    return result
