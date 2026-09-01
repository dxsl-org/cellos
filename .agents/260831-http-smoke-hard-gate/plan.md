---
title: "HTTP Smoke Hard Gate"
status: completed
priority: P1
branch: main
created: 2026-08-31
---

# HTTP Smoke Hard Gate

## Scope Contract

Run the independently supported plain-HTTP QEMU smoke in CI. Reuse the `boot-suite` image producer and evidence artifact; make no claim from the default image's generic HTTPS connect failure.

## Boundaries

- Do not add a clock/provider, generate TLS certificates, rotate roots, or weaken TLS.
- Do not claim positive or negative HTTPS runtime evidence from this smoke.
- Do not duplicate disk generation.
- Preserve the host/QEMU evidence ceiling.

## Phases

| Phase | Work | Status |
|---|---|---|
| 01 | [Gate the plain-HTTP smoke](./phase-01-http-smoke-hard-gate.md) | completed |

## Evidence

`cells/services/net/src/tls/clock.rs` returns `None`; the generic guest HTTPS connect error cannot prove which boundary failed. Precedent `826fdb627f10064116d98a369d5f7668797068c4` created the smoke and disk inclusion, while `boot-suite` already builds the complete image and retains logs.

The focused target compiles, workflow YAML parses with 19 jobs, and the exact
`CI=1` QEMU smoke passes 1/1 in an isolated network namespace. An occupied host
port now fails before QEMU with a specific ownership error.