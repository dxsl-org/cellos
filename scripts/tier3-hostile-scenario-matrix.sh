#!/usr/bin/env bash

# Shared Tier 3 hostile runner expectation matrix (probe markers only).
# Not a malformed-input payload corpus; that remains unsolved while transport
# coverage is still owned by Phases 09/10.
TIER3_HOSTILE_CORPUS=(
  "bounds|[HOSTILE_PROBE] BOUNDS_TEST_NOT_APPLICABLE|1|not_applicable"
  "descriptor|[HOSTILE_PROBE] DESC_TEST_NOT_APPLICABLE|2|not_applicable"
  "backend|[HOSTILE_PROBE] BACKEND_TEST_NOT_APPLICABLE|3|not_applicable"
  "budget|[HOSTILE_PROBE] BUDGET_TEST_STARTED|5|hostile_input_not_asserted"
  "reset|[HOSTILE_PROBE] RESET_TEST_STARTED|4|hostile_input_not_asserted"
)
