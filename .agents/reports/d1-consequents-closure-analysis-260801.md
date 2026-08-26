# D1 consequents — retire unrunnable fast-IPC claims

**Status:** approved for Part 6 application.

## Finding

The D1 ruling is final, but its mechanical consequences were not applied. The normative docs
still advertise a 2–3-cycle direct path. `resolve_export` and `R_RISCV_JUMP_SLOT` have no caller,
and the separately linked `ostd` handler table still guarantees fallback for current cells.

The useful measured fact is the opposite of the stale headline: postcard encode plus decode is
about 0.64 us, while the typed message round trip is 48.5 us p50 on the measured QEMU run. A
future Tier-1 rewrite has a large prize, but no shipped direct path currently earns the claim.

## Recommended ruling [FINAL]

**Apply the already-approved D1 consequences without designing the replacement.**

1. Spec 17 remains the model of record; direct dispatch is a future Tier-1 transport under it.
2. Replace current 2–3-cycle claims in normative/living docs with the measured message-path
   evidence and an explicit unmeasured direct-dispatch target.
3. Delete only the proven-unreferenced `resolve_export` and `R_RISCV_JUMP_SLOT` scaffold; mark
   the retained handler tables as inactive for current separately linked cells.
4. Do not change the Law-1 ABI, caller-attestation contract, or message fallback.
