# D35 — Group manifest, revocation, and DICE plans

**Status:** approved/applied 2026-08-01. Documentation/portfolio only.

## Finding

The premise that all three plans are unstarted is stale:

- Manifest v2 P00-P02 landed (`c25f3185`); only parameterized `cap_args` is deferred.
- Capability-revocation P00's honest narrowing landed (`4b8f1543`); eager Class-2
  teardown P01-P05 remains open.
- DICE P00's CDI/token library landed (`aebc092a`); measurement syscall, Silo/KMS,
  K2/K3, and the deferred interop adapter remain separate gates.

They form one trust-chain program but have distinct security invariants, ABI gates,
hardware dependencies, and file ownership. A monolithic implementation plan would make
ordering and confirmation boundaries harder to audit.

## Recommended ruling [FINAL]

**Reject a physical merge; approve one portfolio group with three child plans.**

1. Correct each child plan's phase status from source/commit evidence.
2. Add a Trust & Identity program entry that orders remaining work without moving phase files.
3. Keep Law-1 confirmations local to the child phase that changes the ABI.
4. Defer hardware/consumer-dependent DICE phases and manifest `cap_args` until their
   concrete trigger exists.
