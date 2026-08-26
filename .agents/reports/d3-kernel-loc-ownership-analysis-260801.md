# D3 — canonical kernel LOC definition and owner

**Status:** approved for Part 6 application.

## Finding

Frozen totals in specs range from 5,600 to 22,600 lines and have already drifted again. Spec 21
correctly requires generated status, but no generated LOC artifact or enforcement existed.
Raw lines also mix comments/tests with privileged implementation and overstate the measured nLOC
by roughly one third.

## Recommended ruling [FINAL]

**Use generated nLOC excluding test files as the canonical metric.**

1. `docs/code-metrics.generated.md`, produced by `scripts/generate-code-metrics.py`, owns the
   moving number and is checked by CI.
2. The canonical scope is non-blank, non-comment Rust lines under `kernel/src`, excluding
   `*test*.rs` files.
3. Report a second core lens excluding `task/drivers/**` and `hypervisor/**` for Spec 15 boundary
   migration; never substitute that smaller lens for total kernel nLOC.
4. Withdraw the fixed <=5,000 G2 target. Kernel responsibility and generated trend are the
   binding controls; a static number without an approved baseline invites gaming.
