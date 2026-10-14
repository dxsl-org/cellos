# 2026-10-12 — Configured async baseline

## What happened
Built fresh init/VFS/bench/probe in isolated target; signed4executables through F1/F5. Packed private minimal services/autoload config and existing dev policy; six ramdisk inputs read back identically. Kernel and isolated ISO built; exact init/ramdisk→kernel→ISO containment passed. No source/ABI or policy-semantics edits.

## Observed
Actual oneCPU/twoCPU boots of build/p01-configured-x86.iso reached existing async START/PASS. Both completed8calls with0lost/wrongsequence, reverseecho/releasedprovider, parked TrySend accepted/busy refused/secondqueued=false. Deathleg4admitted/0refused/4PeerGone/0unterminated/0lost. Recorded task3exit0 after PASS; bench association is inferred, not explicit TID/incarnation mapping.

## Limits
Probe pinnedCPU0: no distinct-CPU throughput or aggregatequota proof. No resource/settledreclamation measurement, cancellation or new-contractACqualification. Known VMM failure occurred incidentally in required boots; no overallimagePASS. Minimalimage omits normalservice stack and must not be compared as marginal cost with earlierimages.

## Evidence
configured-async-provenance.json and oneCPU/twoCPU serial logs in plan directory; readiness/launcher/overview updated.26docs/301locallinks/0missingpaths,10pendingphases/38retainedlocks. OldISO hash unchanged; newISO and durable leg counters verified.3174retainedsource hashes matched before/after, not atomic/exhaustive provenance.

## Next
Explicit generation attribution and bounded resource instrumentation design before incremental/highwater/CPU measurements. Exact checkpoint approvals still pending; no commit.
