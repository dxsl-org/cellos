# G0 instrumentation reconciliation

Recorded 2026-10-14. Evidence reconciliation, not new implementation authorization or producer qualification. Governing [dependency amendment](dependency-checkpoint-amendment.md), [source-bound entry](instrumentation-implementation-entry.md) and [measured-profile input](measured-profile-candidate.md).

## Already implemented versus missing

The historical entry's proposed build gate is now implemented in `kernel/build.rs:12–31`: checked private cfg, environment invalidation, absent/0 exclusion, x86_64 plus test-hooks admission and rejection of incompatible values. Existing private measurement exporter emits VT-d source/node/edge groups from `p01_vtd_topology`. Do not propose another gate or duplicate instrumentation convention.

The retained descriptor-tree producer predates widened VT-d coverage. Current readiness replay preserves the complete requested 15-capture matrix but refuses profile selection and reports VT-d software structure completeness false. Historical supported-family completeness cannot qualify this newer conjunction. Hardware publication/invalidation is a separate claim even when software descriptor geometry is complete.

The `build/p01-vtd-structures` directory has no completed `build.json` producer receipt in the inspected listing; displayed kernel/image and several command logs are zero bytes. Its presence cannot count as a successful build, capture or immutable producer. Preserve it as incomplete history; do not hash an empty ISO into a success-shaped artifact.

## Exercised current smoke

Actual command `python3 -m unittest discover -s scripts/x86 -p test_p01_trace.py` returned exit0,98 tests, OK. [Source-pinned receipt](../../build/cp1-prerequisites/g0-reconciliation.json) identifies the current gate/exporter/decoder/readiness bytes. This exercises decoder scenarios, not guest production, all-path storage fit, hardware fences or production admission.

## Exact remaining G0 obligations

1. Produce a complete current-source isolated measurement build receipt: commands/exits, immutable input denominator, compiler/features, source/image/config/probe hashes and actual nonempty scenario artifacts. Preserve disabled-image exclusion separately.
2. Capture all expected normal/denied scenarios from that producer; bind raw capture hashes and exact guest/CPU/device parameters. Require supported VT-d root/context/SLPT identities and full descriptor geometry. Refused/partial captures remain failures rather than replacement runs.
3. Supply source-owned per-incarnation event-high-water and independent all-path instrumentation capacity/history/retention proof. The existing 1024/8192/2048 diagnostic bounds do not prove their own fit from sample maxima.
4. Observe hardware root/context publication, acknowledged invalidation and safe DMA retirement on the admitted hardware lane. QEMU translated-DMA smoke cannot qualify physical hardware.
5. Review actual source/lock/unsafe/caller coverage and scoped handoff before changing private producer code. Existing observer access and normal-image enforcement remain intact; no public token or production parameter is introduced.

## Observable acceptance

- G0-AC1: WHEN a producer is reported complete, its receipt SHALL contain successful actual commands and nonempty hash-verified artifacts for every selected scenario; incomplete directories SHALL refuse acceptance.
- G0-AC2: IF a current decoder requires VT-d structures absent from an older capture, the report SHALL preserve refusal, not reuse the historical narrower completeness flag.
- G0-AC3: WHEN a profile uses high-water or capacity, its evidence SHALL distinguish event-complete disjoint charge from polling maxima and supply all-path producer/storage bounds.
- G0-AC4: IF only software geometry or emulator DMA is observed, the receipt SHALL NOT claim physical hardware publication or production budget qualification.

G0 remains partial. Its decoder smoke can feed candidate preparation but grants neither CP1 nor release of any scoped lock.

## Fresh current-source producer

Actual `python3 scripts/x86/p01_capture.py build --out build/p01-g0-vtd-20261014 --counts 0 1 4 8 denied` completed successfully. Its [producer receipt](../../build/p01-g0-vtd-20261014/build.json) has SHA-256 `d627a21e32f0ced15b54ebfbad77a7f89c0774355a833a2a4a1abee943493285`; all53 recorded commands returned0, including packaged-byte readbacks. Input SHA-256 is `166f3e7a51ba2ec445f62db7af39b4c12865bb34aa2c3cda1af710808d363824`. Each of the five scenario ISOs is26912768bytes and each kernel ELF23041280bytes. These are actual complete producer artifacts, not the earlier incomplete directory.

The source-built private producer closes the missing current producer receipt only. Its development signing, local input denominator and QEMU measurement lane do not authorize production or protected authority. Fresh capture/decoding and readiness are separate observations.

## Fresh runtime matrix and exact coverage ceiling

Actual capture command used the new producer, all0/1/4/8/denied counts, one guest CPU and `--intel-iommu`; [matrix](../../build/p01-g0-vtd-20261014-captures/matrix.json) contains15fresh QEMU boots. All15are structurally complete and allthree denied controls are accepted. The current readiness analyzer reports complete source/provenance matrix, CPU graphs, heap-stage partitions, root lifetimes and widened VT-d/software-tree completeness. This supersedes absent VT-d groups in the older captures, not their historical raw evidence.

**Narrow scope:** all78VT-d cutoffs contain exactly one root node and zero edges/incoming requester references. This lane observes the empty requester/context census; it does not exercise a populated context table, ownership transfer, invalidation or translated DMA. `software_structure_observed_complete=true` means this observed census is complete, not that every descriptor state was exercised.

The [runtime receipt](../../build/cp1-prerequisites/g0-vtd-runtime-receipt.json) pins producer/matrix/readiness hashes and coverage counts. Actual readiness command with `--require-budget-ready` returned exit2 with15blocker classes, null candidates and per-owner high-water incomplete; allcaptures remain `fully_settled=false`. The [full report](../../build/cp1-prerequisites/g0-vtd-readiness-20261014.json) explicitly preserves hardware publication and physical semantic coverage false. No two-CPU qualification, production values, physical remapper witness or CP1 approval follows.
