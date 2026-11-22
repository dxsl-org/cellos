---
phase: 5
title: "`python-net`: client-network host module"
status: pending
priority: P2
effort: "1.5d"
dependencies: [4]
tier: medium
checkpoint: false
---

# Phase 5: `python-net`: client-network host module

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Overview

Monty has no sockets; network is reachable only through host functions. This phase installs the same signed binary at
`/bin/python-net` with the `net_client` launch edge (outbound DNS, TCP/UDP connect, no listen, no raw frames), as `lua-net` does,
and exposes a small host module to scripts.

## Requirements

- Functional: a host module `cellos_net` (through `external_modules`) offers `resolve(host)`, `tcp_connect(host, port)` → handle,
  `send(h, bytes)`, `recv(h, max)`, `close(h)` and `http_get(url)` (only if `ostd::http` fits the ELF budget). Calls go to the net service through
  `ostd::clients::net`.
- Functional: under `/bin/python` the module is absent (`ModuleNotFoundError`) and the net service refuses anyway. The right comes from the
  resolved install path, never from the script.
- Non-functional: handles are cell-local integers, and a closed or unknown handle raises `OSError`. Every call counts against Monty's `max_suspensions`.

## Architecture

The module answers `OsFunctionCall::Import("cellos_net")` with a host-backed object only when the cell runs as `python-net`.
Detection compares the kernel-provided launch path (argv[0] / launch record) with `/bin/python-net`, and the net service enforces
the right independently. Catalog: `net_client("/bin/python-net")` beside `lua-net`, with phase 2's quota and stack.

## Assumptions

- **Claim:** the cell can learn its resolved launch path from argv[0] as Lua does (`lua` vs `lua-net`).
  **Confidence:** medium. **How to verify:** `cells/runtimes/lua/src/main.rs` and `bindings_net.rs`.
- **Claim:** `ostd::clients::net` client calls work from a Tier 2 std cell.
  **Confidence:** medium. **How to verify:** step 2 lane.

## Related Files

- Create: `cells/runtimes/python/src/net.rs`
- Modify: `kernel/src/catalog.rs` (+ export), image cell lists, `scripts/qemu-*` lane for net (reuse `scripts/qemu-lua-vnet.py`'s shape)

## Implementation Steps

1. Implement the module and the launch-path gate; host-test the handle table.
2. Lane: the same script under `python` (ModuleNotFoundError) and `python-net` (resolve, plus a TCP echo to the QEMU user-net host forward).
3. Owner decision recorded for the new network edge, as for `lua-net` on 2026-11-10.

## Success Criteria

- [ ] `python` refuses `import cellos_net`; `python-net` resolves and completes a TCP echo in QEMU.
- [ ] Handle-table host tests pass.

## Security Considerations

This phase grants a capability. Listening, raw frames and binding stay unavailable. The module is a convenience; the net service's
per-path edge is the boundary.

## Risk Notes

- `http_get` may push the ELF past budget. Drop it and keep raw TCP if so, and log the decision.

## Deviation Log

None.
