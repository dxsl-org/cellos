# W^X Cross-Hart TLB Shootdown Plan

## Goal

Produce a narrow implementation plan that closes stale W^X/page-permission translations across harts without expanding Cellos ABI or unrelated SMP features.

## Decisions

- Preserve `HANDOFF-260731.md` section 8 ordering; treat this as a P0 security exception, not a Midori scope expansion.
- Use synchronous SBI RFENCE on RV64, with an online-hart mask that subtracts the actual caller and with explicit PTE-store ordering.
- Hold frame/VA reuse until post-unmap invalidation completes; fail-stop after an unrecoverable RFENCE error.
- Treat AArch64 `TLBI ...IS` as the architectural broadcast mechanism, but require two-active-PE evidence before closure.
- Keep x86_64 blocked because current Cellos has only local `INVLPG` and no runnable SMP/LAPIC shootdown path.

## Evidence Gates

- RV64 QEMU must run with literal `-smp 2`, prove hart 1 online, pass the physical-content oracle, and fail the negative control.
- Hardware evidence is mandatory for each architecture eventually claimed closed.
- Unavailable non-RV lanes remain `RUNTIME-GATED` or `HOST-GATED`, never `PASS`.

## Next Step

Execute `/home/dmin/cellos/.agents/260808-1544-wx-cross-hart-tlb-shootdown` with `$hc-cook`; rollback RFENCE and teardown/reuse ordering together if evidence fails.
