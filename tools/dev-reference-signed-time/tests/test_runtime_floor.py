import math
import unittest

import path_bootstrap  # noqa: F401

from protocol_models import MAX_UINT64
from runtime_floor import RuntimeFloorError, load_host_time_floor


class RuntimeFloorTests(unittest.TestCase):
    def test_reads_once_and_truncates_to_bounded_unix_seconds(self):
        calls = []

        def clock():
            calls.append(None)
            return 1_700_000_000.999

        self.assertEqual(load_host_time_floor(clock), 1_700_000_000)
        self.assertEqual(len(calls), 1)

    def test_accepts_exact_uint64_boundaries(self):
        self.assertEqual(load_host_time_floor(lambda: 0), 0)
        self.assertEqual(load_host_time_floor(lambda: MAX_UINT64), MAX_UINT64)

    def test_rejects_unavailable_or_unbounded_clock_values(self):
        cases = (
            True,
            "1700000000",
            -1,
            -0.5,
            float(MAX_UINT64 + 1),
            math.nan,
            math.inf,
            -math.inf,
        )
        for value in cases:
            with self.subTest(value=repr(value)):
                with self.assertRaisesRegex(RuntimeFloorError, "^host clock floor unavailable$"):
                    load_host_time_floor(lambda value=value: value)

        def failed_clock():
            raise OSError("provider detail must not escape")

        with self.assertRaisesRegex(RuntimeFloorError, "^host clock floor unavailable$"):
            load_host_time_floor(failed_clock)


if __name__ == "__main__":
    unittest.main()
