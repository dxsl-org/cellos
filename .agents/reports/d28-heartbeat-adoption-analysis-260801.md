# D28 — Correct heartbeat adoption status

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

Spec 12 says only the net service adopts `Heartbeat`. Source contains calls in 13 files,
but those files belong to **six** distinct Cell binaries: robot-dashboard, http-smoke,
input, net, net-broker, and init. The docket's approximate "13 cells" conflates call
sites with independently supervised binaries.

Heartbeat remains opt-in; broad adoption does not make it a fleet-wide mandatory policy
or a substitute for a hardware watchdog.

## Recommended ruling [FINAL]

**Approve recommendation A: correct the stale adoption claim without freezing a count.**

1. Say that multiple service, tool, demo, and application Cells opt in today.
2. Do not place an exact adopter count in the normative spec; generated status may report
   the current six binaries and 13 source files.
3. Replace the stale "app-level heartbeat remaining" item with the real gaps: explicit
   fleet policy, required-service enrollment, negative coverage, and hardware watchdog.
4. Preserve the existing opt-in syscall contract and timeout semantics.
