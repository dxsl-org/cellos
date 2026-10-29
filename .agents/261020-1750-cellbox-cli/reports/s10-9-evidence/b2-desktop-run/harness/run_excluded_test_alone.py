#!/usr/bin/env python3
"""Runs the ONE excluded retained test alone against the CURRENT environment (evidence only; never part of the gate; nothing mocked).

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_excluded_test_alone.py > excluded-retained-test-current-state.txt
"""
import io
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
RUNTIME = next(p for p in Path(__file__).resolve().parents if p.name == 'reports') / 's10-4-evidence/runtime/observer-s10-9'
sys.path.insert(0, str(RUNTIME))
import s109_gate as G  # noqa: E402

with G.retained_paths() as links:
    print(f'# Excluded retained test run ALONE against the CURRENT environment (not part of the gate).\n# links: {links}')
    suite = unittest.TestLoader().loadTestsFromName('test_t7_t11_flow.PreservationAndPolicy.test_preflight_defines_every_go_no_go_criterion')
    stream = io.StringIO()
    result = unittest.TextTestRunner(stream=stream, verbosity=2).run(suite)
    print(stream.getvalue())
    print(f'# wasSuccessful = {result.wasSuccessful()} failures {len(result.failures)} errors {len(result.errors)}')
