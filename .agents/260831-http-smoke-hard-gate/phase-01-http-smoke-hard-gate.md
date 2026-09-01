---
phase: 1
status: completed
priority: P1
dependencies: []
---

# Phase 01: HTTP Smoke Hard Gate

## Context Links

- [Parent plan](./plan.md)
- `tests/integration/tests/http-smoke.rs`
- `cells/services/net/src/tls/clock.rs`
- `.github/workflows/ci.yml`

## Overview

Scope the existing smoke to its independently executable plain-HTTP contract and run it in the existing QEMU image job.

## Key Insights

Default `service-net` deliberately has no authenticated certificate time. The guest's generic HTTPS connect failure is not positive or negative certificate-path evidence. HTTP remains independently testable.

## Requirements

- Start only the plain host mock.
- Require guest `HTTP PASS`.
- Make no HTTPS assertion.
- Run under `boot-suite` with `CI` prerequisite enforcement and retain the log.

## Architecture

`boot-suite image → plain mock → guest HTTP success → retained log`.

## Related Code Files

- Modify `tests/integration/tests/http-smoke.rs`.
- Modify `.github/workflows/ci.yml`.
- Update roadmap risk/changelog and this plan after verification.

## Implementation Steps

1. Remove TLS mock startup and positive-HTTPS documentation.
2. Assert only the supported HTTP success marker.
3. Invoke the test from `boot-suite` and retain its output.
4. Compile, run the exact QEMU test, parse workflow YAML, and review.

## Todo List

- [x] Implement truthful HTTP-only assertions.
- [x] Wire the existing CI producer.
- [x] Verify and document the final contract.

## Success Criteria

- CI requires the plain-HTTP round trip.
- Missing CI prerequisites cannot skip.
- No HTTPS claim, duplicate disk build, trust-root change, or TLS weakening.

## Evidence

Focused verification: the Rust target compiles; workflow YAML parses with 19
jobs; the file is 188 lines; and two sequential exact `CI=1` smoke runs pass
1/1 in the same isolated network namespace after observing completion and
`HTTP PASS`. The second pass proves `MockProcess` kills and reaps the plain mock
and releases port 8080. On this workstation, an unrelated listener owns that
port; the hardened preflight fails immediately with
`required port 8080 is already in use` instead of mistaking that service for
the mock.

## Risk Assessment

The guest still attempts HTTPS and emits a generic failure. Review must prevent
that incidental output from becoming evidence.

## Security Considerations

Test-only convenience must not bypass the authenticated-time boundary or turn a transport failure into a security claim.

## Next Steps

Positive HTTPS remains blocked until admitted authenticated time or a separately reviewed test-only provider exists.
