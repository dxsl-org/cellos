# P01 — Intel-direction measurement readiness and observed x86 baseline

Recorded 2026-10-12. Non-activating investigation of an existing development ISO; no compilation, source edits, network connection, persistent disk writes, public ABI approval or new profile admission. This is **x86 QEMU-TCG evidence**, not physical Intel, VMX, protected or production qualification. P01 remains pending.

## Actual scenario and artifact

- Artifact: `build/vicell-x86.iso`, SHA-256 `cd5fc9380e35dd8a55b2668ee7ae37ead7eafb6fdc8d16c6105c967d90915446`; ISO contains `boot/kernel.elf` and Limine configuration. Source revision/features/compiler-to-artifact provenance was not established; do not assert this preexisting artifact was built from the current checkout.
- Emulator observed: QEMU 8.2.2, Debian `1:8.2.2+ds-0ubuntu1.17`.
- Exact emulator arguments: `qemu-system-x86_64 -machine q35 -accel tcg -cpu qemu64,+pdpe1gb -m 256M -nographic -cdrom build/vicell-x86.iso -boot d -no-reboot -snapshot -nic none -monitor none`.
- First boot observed shell prompt and terminated at the intentional 30s `timeout` (exit124), not a boot failure. No shell command on that first boot.
- Second boot used the same arguments. A bounded subprocess driver waited for `Cellos >`, sent `free\r`, captured a `Mem (KiB)` row and terminated QEMU. Measurement completed about3.25s after launch; no claim of a settled long-duration idle baseline. Raw serial evidence: [evidence-2026-10-12-x86-free.log](evidence-2026-10-12-x86-free.log).

## Observed physical-frame allocator snapshot

| Metric | KiB |
|---|---:|
| Total tracked capacity | 247760 |
| Used committed frames | 17320 |
| Free tracked frames | 230440 |

Observed equality `total=used+free` holds; used amount is17,735,680 bytes. The existing source `cells/tools/shell/src/cmd_sys.rs:57-86` calls `sys_mem_info` and validates frame arithmetic. `kernel/src/task/syscall.rs:6726-6746` obtains frame allocator totals; `libs/api/src/abi/syscall.rs:1452-1462` states committed frames include kernel heap reservation. This source describes the metric's intended meaning, **not a verified source-to-ISO provenance chain**. It is neither RSS nor per-Cell attribution nor a maximum/mean/marginal cost. No target budget is selected from one snapshot.

## Observed profile limitations

Serial output identifies the development profile: Tier2 admission disabled, PKU/CET unavailable, PCID disabled, VT-d absent and DMA untranslated. It reports SVM root operation, not Intel VMX. No NIC, persistent SATA/NVMe disk, VFS persistent mounts or Tier3 guest was admitted in this scenario; VFS mount warnings and missing test executables appear. Baseline services reach shell despite those explicit gaps. Built-in selftest markers are incidental output of this historical image, not current-source regression evidence or Spec25 criterion closure.

The installed QEMU8.2.2 does not meet the exact10.2.0 prerequisite in existing Tier3 scripts `scripts/qemu-x86-virtio-e2e.sh:43-46` / `qemu-x86-python-gate.sh:33-34`. Those guest scripts were not executed, and this planning investigation does not install/upgrade or provision anything.

## Reachable measurement mechanisms and unsuitable substitutes

| Mechanism / source | What it supplies | Missing P01 evidence |
|---|---|---|
| Shell `free`; benchmark `cells/tests/bench/src/scenarios/memory_footprint.rs:1-4,28-37` | Aggregate committed frames, includes heap reservation | Source/artifact pin, stable baseline, marginal Cell/thread/metadata/DMA costs, application attribution and high-water workload |
| `kernel/src/task.rs:658-746` test-hooks stack watermark | Named init/shell/VFS/VFS-test/NET/virtio-net/thread stacks, emits allocation and used bytes after1500ticks or VFS-test exit | Suitable pinned x86 measurement image, complete scenario; first30s boot supplied no stack-baseline row. No conclusion that current source lacks hook or image has specific feature configuration |
| `scripts/qemu-cell-scale.sh:13-17,43-50,78-81` | Experimental parked-child memory exhaustion lane, defaultRISC-V/4096slots | Not an admitted Intel-direction runner; never promote4096slots or parked-child count to application/cluster throughput. No parkednonIntel lane executed |
| Existing x86 boot runner `scripts/qemu-x86_64-test.sh:86-105` | Boot/panic oracle and explicit device gates | Not marginal footprint or new-budget measurement; direct invocation avoided overwriting its shared root logs |

## Exact prerequisites before finite profile selection

1. Pin current-source build, compiler/features, embedded service list and artifact digests to an admitted x86 local measurement lane. Existing unknown-provenance ISO may seed a baseline only; cannot certify current changed contract.
2. Capture stable pre-workload and incremental/high-water Cell/thread/stack/CSpace/lineage/operation/completion/queue/DMA allocation attribution, including kernel-owned storage and retained unknown/terminal charge. Avoid using whole reserved heap divided by Cell count.
3. Record aggregate CPU execution unit/common period/burst/priority/tick-switch charging and overshoot tolerance before evaluating quota. Existing watchdog/tick fields cannot implement this new accounting by documentary implication.
4. Budget Authority/controller/router target services do not yet supply actual bootstrap/genesis/liveclaim/control-envelope costs. Exact bounded layouts and reviewed design are needed before instrumented implementation; P08 real producers and P10 operational high-water remain independent gates.
5. Obtain exact Law-1 checkpoint1 for public/persistent delta before edits, checkpoint2 before release; preserve protected Build/activation, relay-first/directLAN and separately approved Intel VMX/physical/production lanes. Runtime capacity and200overlapping-client qualification remain P10, not discharged by shell `free`.

Result: a real existing-x86 allocator snapshot is available; **full measured profile, exhaustive consumer closure, exact byte approval and new-contract qualification remain open**. No lock/phase/AC status changes.

## Current-kernel pinned continuation — 2026-10-12

The earlier unknown-provenance ISO observation above remains historical. This continuation built the **current kernel source** into isolated `build/p01-pinned-x86-target`, packaged `build/p01-pinned-x86.iso`, and booted that exact path. No source/ABI change or old ISO overwrite. [Machine-readable provenance](pinned-x86-provenance.json) records 798 local source/config/embedded/boot inputs with SHA-256, compiler, exact build/assembly/run commands and final kernel/ISO digests. All recorded inputs matched before and after the build. This closes the current-kernel build-input association for this scenario, **not** exhaustive toolchain/registry provenance or source provenance for the reused embedded service binaries.

- Compiler: `rustc 1.97.0-nightly (f53b654a8 2026-04-30)`, LLVM22.1.4; target `x86_64-unknown-none`, release, default development kernel features plus `test-hooks`, locked/offline dependencies, `-Z build-std=core,alloc`.
- Kernel SHA-256: `fb9172279fab2d862070a731ebcfe696d387262b0d521f261fabca60580b31ed`.
- ISO SHA-256: `b182259920e87bce5263b1ccb982d087576fa26157e1ac2e3cf54e8ab1e46258`.
- Embedded init and ramdisk were hashed existing binary inputs, not rebuilt service-source outputs. No claim of complete current-source application image.
- Build finished successfully; assembly produced 10,782,720-byte ISO. Boot used the earlier q35/TCG/256M/network-disabled/snapshot arguments with the new ISO; one CPU, no physical/VMX qualification.
- Shell accepted `free` twice: first after about4.94s wall time, second requested after20s; capture ended about22.18s. Both rows were identical and passed total=used+free arithmetic. This is stability at two sampled instants, not a continuously flat high-water or steady-state guarantee.

| Pinned allocator metric | KiB at both snapshots |
|---|---:|
| Total tracked capacity | 246460 |
| Used committed frames | 17328 |
| Free tracked frames | 229132 |

Stack hooks emitted these sampled watermarks after the existing1500tick gate. Allocated bytes include guards; used bytes cover this boot path only, not a worst-case service workload or safe new stack size:

| Cell name | Stack | Used bytes | Allocated bytes | Usable bytes |
|---|---|---:|---:|---:|
| init | kernel | 19976 | 73728 | 65536 |
| init | user | 1784 | 73728 | 65536 |
| vfs | kernel | 14312 | 270336 | 262144 |
| vfs | user | 47016 | 270336 | 262144 |
| net | kernel | 14032 | 73728 | 65536 |
| net | user | 19120 | 73728 | 65536 |
| shell | kernel | 14032 | 73728 | 65536 |
| shell | user | 15232 | 73728 | 65536 |

**Observed failure, not suppressed:** serial contains `X86-VMM-SMOKE: FAIL e1=Preempted e2=Preempted`. The shell measurement succeeds, but the image is **not an overall selftest/virtualization qualification PASS**. This investigation did not rerun or repair that failure; it retains the full [pinned serial log](evidence-2026-10-12-pinned-x86.log). PKU skip, development posture, absent DMA remapper and persistent mount/test-artifact gaps remain explicit; selftest success strings do not close new-contract ACs.

Remaining measurement gate: rebuilt/pinned service artifacts and image inventory; complete marginal/retained/high-water allocations; aggregate CPU quota and SMP accounting; workload/control-envelope costs; exact limits/tolerances and approvals. No resource ceiling is chosen from these samples, and P01/38 locks/all new-contract ACs remain pending.

## Source-rebuilt service image — 2026-10-12

This next scenario closes the reused-service-binary gap **for the explicitly selected image**, not all repository services/profiles. Rebuilt init, shell, VFS, config, platform, NET, input, x86 serial, e1000, bench and bench-probe from current source into `build/p01-service-x86-target`; all11 executable binaries passed the existing F1/F5 signing gate and were dev-signed. Services use package defaults, not service test-hooks; kernel uses development defaults + test-hooks. These are development artifacts, never production signing/admission.

Initial signing correctly refused a nonexistent `driver-serial` executable: that package produces a library. The actual `driver-serial-x86` executable was then rebuilt and the corrected11-binary set signed through the same gate. No signer bypass, fabricated binary or weakened gate. Cell compilation succeeded with four NET dead-code warnings; no clean-lint claim.

[Service provenance](service-x86-provenance.json) records3174 stable local source/config/sign/packaging inputs, compiler, build environment, exact command chain, binary/image mapping and hashes. This is recorded-scope provenance, not exhaustive compiler/registry/vendor/macro-input certification. Independent `mtype` readback confirmed each of10 ramdisk executable files equals its signed build output, plus policy equality; separate embedded init equals signed rebuilt init. Included config/input/probes are not all observed running, and no complete-application readiness is inferred from kernel/init registration.

Input hashes were captured during the initial asynchronous Cell build and checked again after packaging/run, not an atomic pre-build filesystem snapshot. They establish equality at those observed instants; transient earlier edits are not excluded by that claim. Full byte-sequence checks additionally confirmed rebuilt signed init and readback-verified ramdisk appear in kernel, and that kernel appears in the ISO; this is artifact containment, not independent loader/parser security qualification.

- New ISO: `build/p01-service-x86.iso`,11143168bytes; SHA-256 `eb09fc3019ba90bd042ed854333231f189942366118e1a5b0c1f123922e7a4e2`.
- Kernel SHA-256: `972d52c570d0e1f20e41e7aacb88a7e4deeb28a84eea0ed9b31f12fd4e06afa8`.
- New ramdisk/init/ISO directories are isolated; no old embedded artifact or ISO overwritten. FAT16 ramdisk is built by existing `tools/mkfat32.py`; policy by existing dev policy signer.
- Boot arguments retain q35/TCG/qemu64/256M/oneCPU/network-disabled/snapshot mode with the new ISO. Shell `free` succeeded twice; capture ended about24.13s, second request after22s. [Full serial](evidence-2026-10-12-service-x86.log) preserves both rows, boot paths and warnings.

| Rebuilt-service allocator metric | KiB at both snapshots |
|---|---:|
| Total tracked capacity | 245756 |
| Used committed frames | 25488 |
| Free tracked frames | 220268 |

Both satisfy total=used+free. Image contents/service implementations differ from earlier samples, so differences cannot be treated as marginal per-Cell cost or performance regression. This scenario supplies source-rebuilt baseline, not a workload high-water, successful network path or new-budget enforcement.

| Cell name | Stack | Boot used bytes | Allocated bytes | Usable bytes |
|---|---|---:|---:|---:|
| init | kernel | 19976 | 73728 | 65536 |
| init | user | 18408 | 73728 | 65536 |
| vfs | kernel | 14680 | 270336 | 262144 |
| vfs | user | 47032 | 270336 | 262144 |
| net | kernel | 14032 | 73728 | 65536 |
| net | user | 21096 | 73728 | 65536 |
| shell | kernel | 14032 | 73728 | 65536 |
| shell | user | 15136 | 73728 | 65536 |

The known `X86-VMM-SMOKE: FAIL e1=Preempted e2=Preempted` appears incidentally in this newly packaged image too; its preservation is not a separate failure-confirmation rerun or overall qualification PASS. Serial now reports local development Tier2 admission ENABLED after existing oneCPU test-hooks gates; this is observed current baseline behavior, **not approval of the new-contract Tier2/remote/production profile**. PKU unavailable, no DMA remapper, no NIC traffic, persistent mounts unavailable, missing optional block/NVMe/AHCI/test Cells and absent protected/Authority/controller/router implementations remain explicit. Do not repeat the historical Tier2-disabled claim as this artifact's posture.

Remaining P01 measurement: complete incremental/retained/high-water attribution, aggregate CPU/SMP accounting, approved finite dimensions and tolerance, and exact public/persistent checkpoint proposals. The selected service-source packaging prerequisite is now evidenced; no AC/phase/38-lock status changes and no standalone shell-to-probe grant invented to obtain a misleading marginal measurement.

## Two-CPU launch-readiness scenario — 2026-10-12

Booted the same source-rebuilt ISO/digest with q35/TCG/qemu64+pdpe1gb/256M, two CPUs, network disabled and snapshot mode. [Serial evidence](evidence-2026-10-12-service-x86-smp.log) and [exact scenario metadata](service-x86-smp-provenance.json) retain commands, timings and outcome. Existing boot markers report topology online=2, context handoff, remote shootdown, SAS fastpath and pin-dying PASS. These are existing development gates, not proof of the proposed contract's SMP correctness, aggregate CPU accounting or cluster execution.

Attempted `exec /bin/bench async-lifecycle` after the first `free`. Loader refused the shell→bench launch edge (`spawn_cap=false supervisor_cap=false`). The scenario produced neither START nor PASS: **workload NOT RUN**, not a successful lifecycle or failed lifecycle implementation. Second `free` measures the rejected launch path, not reclamation after completed workload.

| Two-CPU allocator metric | KiB before and after rejected launch |
|---|---:|
| Total tracked capacity | 245680 |
| Used committed frames | 25752 |
| Free tracked frames | 219928 |

Both rows satisfy allocator arithmetic. Do not infer per-CPU marginal cost from comparison with the one-CPU sample: timing/service scheduling differ and no isolated attribution exists. Known VMM failure again occurs incidentally during this distinct two-CPU boot; no dedicated confirmation rerun, no overall PASS.

### Required measurement launcher before workload qualification

The older `scripts/x86/run-c2c-concurrent-input-smoke.py` issues `bench async-lifecycle`/`bench local-service-lifecycle`, but current shell sources contain no `bench` command. Treat that script as stale for this selected image, not an available authorized alternative. Do not widen shell authority, edit policy to conceal denial, or rerun the same refused edge.

Planning-only prerequisite: use the existing init-owned opt-in fixture pattern in `cells/tools/init/src/boot.rs::spawn_test_fixtures` for a dedicated measurement image. Its exact new fixture edge, bench/probe child edges, feature scope, package manifest/policy ceilings and signing must be reviewed before implementation; no existing permission is implied. Keep normal images unchanged. Launch with explicit bounded scenario arguments, collect START/terminal/result and before/peak/after counters, attribute child termination and resources, and fail closed on a refused launch or missing terminal marker. First scenario is bounded async lifecycle; throughput/marginal/high-water measurements need separate scoped workloads and accounting. This prerequisite may be collapsed into P01 measurement harness work; it does not create an eleventh phase or release any lock.

### Superseding launcher decision — source reconciliation

The preceding new-feature proposal is superseded by the [existing-launcher contract](measurement-launcher-contract.md). Current init supports structured args in autoload.toml, republishes argv on ELF/raw-path attempts and uses reviewed init-autoload ceilings. Existing init→bench and bench→bench-probe edges provide the selected route without widening authority or adding a fixture feature. This corrects planning, not the denied shell launch evidence.

Use isolated config/ramdisk/kernel/ISO and restart=never; runtime configuration acceptance is still pending. Async probes are pinnedCPU0, so a twoCPU run is not aggregate-CPU, multiworker or multithread proof. Host parser tests passed18tests with explicit Linux target; bare-metal default initially failed for missing test crate. Exact proposed config/workload has not booted under this route; no launch or resource-success claim.

## Configured bounded lifecycle runtime — 2026-10-12

The existing autoload route has now run, superseding its earlier NOT_RUN status for this specific baseline. A new isolated minimal image rebuilds init/VFS/bench/probe, signs all four through F1/F5 and uses services.toml with no configured services plus the private bench autoload selection. Boot VFS remains; no shell/NET/platform or cluster services are packaged. No source, normal policy semantics, ABI or feature edits. Do not compare its resource totals with the selected-service image.

- ISO `build/p01-configured-x86.iso`, SHA-256 `d45de50bcc24751c50c832a44a4073bd2fb3b2a5043b7c69f49608920c1f6fda`.
- Six config/policy/executable ramdisk inputs matched independent readback; init/ramdisk→kernel→ISO byte containment passed.
- [Provenance](configured-async-provenance.json) retains hashes, profile, commands, results and limits. The3174retained source inputs matched before/after; this is not an atomic or exhaustive toolchain provenance claim.
- Actual q35/TCG/qemu64+pdpe1gb/256M boots: [oneCPU serial](evidence-2026-10-12-configured-async.log), [twoCPU serial](evidence-2026-10-12-configured-async-smp.log). Both ended after terminal plus a ten-second capture window, within150seconds.

| Observed lifecycle result | OneCPU | TwoCPU |
|---|---:|---:|
| Outstanding calls completed | 8 | 8 |
| Lost / wrong sequence | 0 / 0 | 0 / 0 |
| Death-leg admitted / submit refused | 4 / 0 | 4 / 0 |
| PeerGone terminals | 4 | 4 |
| Unterminated / lost on death | 0 / 0 | 0 / 0 |

Both runs show exactly one START/PASS, reverse echo, successful provider release, TrySend parked accepted/busy refused, provider second queued=false and task3exit0 after scenario PASS. **[INFERENCE]** task3is the bench from ordered launch/exit records; logs do not explicitly map the spawned bench TID/incarnation. Strict generation-attributed termination proof remains open; generic clean-child exit is not a replacement. No workload resource-reclamation claim.

Probes are still pinnedCPU0: twoCPU baseline completion is not distinct-CPU throughput, shared-CSpace multithread, cancellation, aggregate CPU quota or distributed proof. Raw latency samples/frequency are retained, not production percentiles or regression thresholds. Known VMM failure appears incidentally in both required boots; neither image is an overall qualification PASS. P01, new-contract ACs and38locks remain pending/unchanged.

## All-count charged-ownership measurement basis

The private-enabled producer `build/p01-budget-stage-ownership/build.json` and five three-boot matrices now exercise0/1/4/8children and denied control on the same pinned kernel source. Exact provenance, observed ranges and remaining selection blockers are in [profile budgets](profile-budgets.md#observed-all-count-budget-measurement-basis). Guest scope remains root-only, QEMU-TCG and one CPU; parallel host launches and scenario-specific ISOs do not establish SMP, a byte-identical same-image control or production capacity.

Actual runtime produces all required live/settled heap sidecars and93 source-coherent heap cutoffs with no unclassified/unmatched charged bytes. All15 raw captures reparse under the corrected decoder; their original sealed metadata/evidence remain unchanged. `build/p01-budget-stage-ownership-evidence-final.json` separately pins the current analyzer and decoder, records all replicates and refuses numerical budget selection with exit2. The earlier `build/p01-budget-measurement-captures` job was cancelled before instrumentation edits and is superseded; its partial artifacts are retained, not a completed matrix.

Verification: enabled matrix build succeeds; source-including ledger39/39 and corrected parser59/59 behavioral tests pass; seven actual-artifact negative controls reject cohort/boot/build reuse and cached qualification promotion while preserving conservative control/absent-metric handling. A decoded surviving-incarnation transfer smoke retains exact before/after provenance. The same kernel source builds with `CELLOS_P01_TRACE=0` in the isolated disabled target. AP-RF/RM/RW/FA rollback markers pass on all15 required boots; marker scan finds no MISMATCH/kernel panic. The known `X86-VMM-SMOKE: FAIL e1=Preempted e2=Preempted` appears incidentally and is retained; these are not overall hardware/VM qualification PASSes.

Observed heap partition closure is complete for these cutoffs; every capture remains `fully_settled=false`. Missing capacity/coverage, physical/reference topology, CPU/thread/operation saturation, actual distributed producers and approval prerequisites are not made into a cleanup tolerance or guessed margin. P01, new-contract ACs,23 exact-delta approvals, CP1/CP2 and38locks remain pending/unchanged; no ABI/quota/signing change, activation or commit.

## Current CPU topology and capacity proof disposition

The next15 fresh one-CPU boots use `build/p01-topology-runs-exact/build.json` and all five `build/p01-topology-runs-{0,1,4,8,denied}-captures/matrix.json` files. `build/p01-topology-runs-evidence.json` proves observed CPU graph completeness and exact snapshot-storage fit at78/78cutoffs. Full task-root identities, deduplicated table/incarnation joins, current shared references and physical leaf aliases replace cumulative path touches only for this current CPU graph scope. See [capacity disposition and observed rows](profile-budgets.md#capacity-refusal-and-observed-current-cpu-graph).

All measured root rows are class1(kernel/SAS). The enabled native-Domain discovery branch also compiles with `test-hooks,native-domains`; this is compile coverage, not native-Domain runtime or SMP qualification.

The initial15-boot uncompressed graph's capacity refusals remain preserved, as does the cancelled partial pre-correction capture directory. Exact leaf runs fix representational redundancy without raising ceilings or weakening graph validation. Version2 excludes hardware accessed/dirty; capture refuses SMP rather than inferring remote quiescence.

Verification: parser73/73 and source-including ledger48/48 behavioral tests pass; enabled all-count packaging and same-kernel-source trace-disabled compilation pass. Five mutations of an actual eight-child raw capture reject zero spans, table runs, invented accessed bits, wrong geometry and managed leaves mislabeled external. `build/p01-topology-runs-smoke.json` preserves their reasons and all15 raw marker results: AP-RF/RM/RW/FA present, no MISMATCH/kernel panic, known VMM Preempted failure retained in every boot.

Heap cutoff ownership remains complete, but every capture remains `fully_settled=false`. The structural allocator proof is a coarse bound above ledger capacity, not a1024fit certificate. Global NPT/EPT/IOMMU/unpublished/retired topology and physical ownership, all-path capacity/coverage and the other15 budget blocker classes remain open; the actual readiness CLI exits2 with no candidates. Lexical verifier/negative controls pass with3715inputs,23pending exact deltas and38locks. P01, issuer/CP1/CP2, ABI/quota/signing and activation remain unchanged; no commit.

## Allocation-lifetime non-CPU root verification

The five fresh three-boot `build/p01-noncpu-roots-{0,1,4,8,denied}-captures/matrix.json` files use the same corrected producer pinned in [root-lifetime evidence](profile-budgets.md#observed-non-cpu-root-lifetimes). All78root cutoffs have complete birth-lifetime and current registry censuses; CPU graph and heap partition checks remain complete. The two still-allocated IOMMU self-test roots, zero registry references and earlier nested-root retirement are preserved rather than inferred away from an empty registry.

Parser83/83 and source-including ledger52/52 pass. Enabled all-count packaging, exact-kernel-source trace-disabled compilation and enabled native-Domain compilation pass; native-Domain compilation is not runtime certification. `build/p01-noncpu-root-lifetimes-smoke.json` records five actual-capture rejections and all15raw boot scans: AP-RF/RM/RW/FA present, no MISMATCH/kernel panic, known VMM Preempted failure retained. The initial failed build's u64/usize compiler diagnostic remains sealed; the corrected image is a new artifact, not an overwritten capture.

Root constructor closure and genuine allocator structural premises are separately pinned. No non-CPU descriptor-tree, DMA/VM authority, global physical ownership, all-path fit, production budget or overall qualification PASS is claimed. Readiness still exits2 with15blocker classes and null candidates; all15captures remain `fully_settled=false`. P01,23exact-delta approvals/CP1/CP2 and38locks remain pending/unchanged.

## Format-aware non-CPU software-tree verification

The [fresh software-tree matrix](profile-budgets.md#observed-non-cpu-software-trees-and-aliases) covers15boots and78complete cutoffs. Every cutoff has six exact table identities and six compressed edges; the two surviving Sv39/VT-d self-test trees share three leaf targets, not their table allocations. Root-lifetime census, CPU graph and observed heap partition checks remain complete.

Parser86/86 and actual-source ledger54/54 pass, including format-specific geometry, middle-of-run count/fill changes and software cross-plane alias extents. Enabled all-count packaging, trace-disabled compilation and native-Domain compilation pass; native runtime is not certified. `build/p01-xtree-smoke.json` records actual-capture rejection of stale physical incarnations, wrong child levels, zero run spans, mismatched Sv39 PPN/target, unnormalized hardware A bits and false exact preflight counts. All15boots preserve AP-RF/RM/RW/FA, contain no MISMATCH/kernel panic and retain the known VMM Preempted failure.

Resource static overhead is14014608bytes, reported without production subtraction. Readiness still exits2 with15blocker classes, null candidates and all captures `fully_settled=false`. Supported software trees are not hardware publication, global physical ownership or a future capacity certificate; the pinned262144allocator bound still exceeds1024slots. P01 and approval/lock prerequisites remain unchanged.

