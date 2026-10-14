# 2026-10-12 — P01 pinned kernel baseline

## What happened
Built current x86 kernel offline/locked in isolated target dir, release/defaultdev+testhooks; assembled new ISO and exercised two shell free snapshots plus boot stack watermarks. No source/ABI edits or old ISO overwrite, no commit.

## Decisions
- Record798 stable local source/config/binaryboot inputs, compiler, commands and final digests. Embedded init/ramdisk remain reused binary inputs, not claimed service-source rebuild.
- Two allocator snapshots and eight bootwatermarks are baseline samples, not marginal/highwater/control-envelope cost or safe new limits.
- Preserve observed X86-VMM-SMOKE FAIL e1=Preempted e2=Preempted; do not repair/rerun or call overall image qualification PASS. Shell measurement succeeds independently.

## Evidence
- Build release finished21.48s; newISO10782720bytes; exacthashes in pinned-x86-provenance.json.
- Full serial retained evidence-2026-10-12-pinned-x86.log. Networkdisabled/TCG/oneCPU/snapshot; no physicalIntel/VMX/protected claim.
-25docs/281local links/0missingpaths;10pendingphases/acyclic;38locks;3negativecontrols;798input hashes stable;2allocatorrows and8watermarks verified.

## Next steps
Pin/rebuild service packaging and measure incremental/retained/highwater resource attribution before finiteprofile/exactABI checkpoint1. P01/allACs remainpending,38scopedlocks active.
