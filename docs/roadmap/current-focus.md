# Current Focus

**Last updated**: 2026-09-22 (cell scale profile status projected; Spec 19 §3 amended)

## Development-first, solo-first execution boundary

[ADR-0007](../decisions/0007-development-first-hardware-constrained-execution.md)
keeps work lane-local and bounded by available hardware and truthful evidence
ceilings. [ADR-0013](../decisions/0013-solo-first-development-independent-promotion.md)
allows the sole accountable maintainer to perform all development roles.
AI agents, local subagents, and CI jobs provide automated assurance; none is an
independent accountable identity.

QEMU evidence is software-only. RPi3 and sensor evidence is development and
hardware-integration evidence for the exact exercised devices only. A missing
independent-member decision blocks only the independently ratified or
production promotion that requires it. When required, another repository
member must answer explicit `YES` or `NO` through the GitHub issue or pull
request bound to the exact proposal, commit, and evidence. It does not block
unrelated host, QEMU, exact-device development, or documentation work.

## Lab-first product workflow

[ADR-0014](../decisions/0014-lab-first-robot-workflows.md) selects LAB-01:
one identified closed inert dummy carrier transferred from indexed rack A to B,
with confirmed placement and traceable outcome. The
[SAS/LBI roadmap plan](../../.agents/260905-1139-sas-lbi-outcome-closure/plan.md)
has achieved full software/QEMU closure across its active roadmap:
- **Phase 04b (CellosFS Native & Vector DMA)**: Pure-Rust CoW extent engine (`libs/cellos-fs`) replaces RedoxFS and LittleFS; 10,000 power-cut tests pass and QEMU two-boot persistence is verified.
- **Phase 05 (Stateful Native Workload)**: 1,000 operations executed with real VFS checkpoints to `/srv/checkpoint.log`, v1->v2 live hotswap at op 300 via cached-TID witness (op 301), and VFS service kill & restart recovery at op 600 verified in QEMU.
- **Phase 06 (LAB-01)**: 06A pure contract and 06B native QEMU witness complete; verified with real VFS trace persistence to `/srv/lab_trace.log` and out-of-band operator reconciliation.
- **Phase 07 (BASE-01)**: 07A shared wrapper and 07B native QEMU witness complete; verified with arm-active/stationary exclusions and VFS trace logging.
- **Phase 08 (ASSEMBLY-01)**: 08A contract and 08B native QEMU witness complete; verified 3 operational modes (StandaloneUpper, StandaloneBase, AssembledStationary), stationary coupling lifecycle, loss-of-lock safe inhibition, and VFS trace logging.
- **Phase 01 & 04 (Evidence & Baseline)**: Immutable source-patch bound QEMU baseline collected and validated.

Physical milestones (**06C, 07C, 08C**) remain external-gated awaiting exact hardware, metrology, and safety packages; software closure alone never claims physical hardware actuation.

G2 is scoped to [organizational web/app/microservice servers and ordinary office PCs](../../.agents/260905-1139-sas-lbi-outcome-closure/organization-deployment-profiles.md),
not specialist devices. These profiles are not additional active implementation
programs and are not technically blocked on robot physical acceptance.

## Dual-Mode Hybrid Architecture baseline (ADR-0015)

[ADR-0015](../decisions/0015-dual-mode-hybrid-architecture.md) settles the architectural baseline:
- **Tier 1 (Real-Time SAS)**: 100% Safe Rust cells + audited Driver Cells (e1000, NVMe, VirtIO with IOMMU MMIO/DMA). Mandatory signing (`signing-required = ON`). Zero-trap SPSC lock-free ring buffer for same-hart cells.
- **Tier 2 (Paged Domain Engine)**: Hardware page-table isolation (`satp`/`CR3`/`TTBR0`). Mandatory home for all unsigned binaries, C-FFI, Lua, and POSIX shims. Memory faults trigger CPU page faults and safe cell termination without corrupting the SAS.
- **Tier 3 (Hardware VM Guest)**: Stage-2 paging for unmodified Linux guest OS.

The implementation roadmap is governed by [.agents/260906-dual-mode-kernel-evolution/plan.md](../../.agents/260906-dual-mode-kernel-evolution/plan.md).

## Native CPU AI inference (Spec 24)

[Spec 24](../specs/24-ai-inference-architecture.md) CP-1..CP-3 landed at the `host` and `qemu`
ceilings: `/bin/ai` (`service::AI = 15`) serves typed-IPC inference from a GGUF checkpoint with no
Linux guest, backed by `libs/ai-proto`, `libs/ai-sdk`, `libs/gguf-rs`, `libs/ai-tokenizer`,
`libs/tensor-math`, and `libs/ai-engine`
([plan](../../.agents/260913-2002-g2-level-a-ai-inference/plan.md)). The QEMU RV64 oracle
(`scripts/run-ai-inference-oracle-qemu.sh`) reproduces the reference token ids and embedding over
typed IPC and prints one `[ai-test] PASS`; a 30-layer Q8_0 checkpoint generates text on the host at
3.97 tok/s. Tier 2 GGML (CP-2) stays blocked on the Tier 2 admission route, NPU/GPU backends
(CP-4/CP-5) stay behind the G3 accelerator envelope, and CP-6/CP-7 remain future work. The AI
interface is frozen under Law 1 (2 of 2 confirmations recorded 2026-09-14; changing it now requires the
ABI process).

The next consumer slice is now shipped at the same ceiling: `service-httpd` accepts a prompt in
`POST /api/infer`, calls the frozen `AiClient` over typed IPC, and returns bounded JSON; the canonical
RV64 image passes the hostfwd QEMU gate. This proves wiring, not latency, streaming, or model quality.

Hypha is the second consumer: its `llm-gateway` Cell answers a turn from `/bin/ai` (local-first) and
only falls back to the OpenAI-compatible network endpoint when no local inference is registered or
loaded (`.agents/260621-1433-hypha-ai-agent/phase-07a-local-inference-backend.md`). The
`hypha-local-ai` gate types two turns in QEMU and requires the local backend line, a reply line, and
no network marker. That gate also reproduced and fixed a real defect: `/bin/hypha` was unreachable
from the console, because the reviewed shell launch edge carries `spawn`, the ELF route refuses
capability-bearing targets, and the documented raw-path fallback resolves only through the kernel
loader's VIFS1 — where the app was not staged.

Engine throughput was measured and fixed next (`.agents/260914-cpu-engine-optimization/`): the Q8_0
matvec was the whole decode, and it was 7× slower per MAC than the dense kernel in the same crate
because the workspace's size profile (`-Oz`) leaves the per-block helpers out of line. The two hot
crates now build at `-O2`: **5.1× on the host** (135M checkpoint 4.7 → 24.0 tokens/s; 4.7 ms/token
on `stories15M`), **13% faster in the QEMU cell** (7.89 → 6.85 s for 24 tokens), and the cell image
is 6 KB smaller. `-O3` is faster on the host but 11% slower in the cell (TCG softfloat dominates), so
it was measured and rejected; both numbers live in that phase doc. A bench
(`libs/ai-engine/benches/cpu_engine.rs`) is the instrument, and `tensor-math`/`ai-engine` now run
their numerics tests in CI.

The kernel *shape* was fixed next (`.agents/260914-q8-integer-activations/`): activations are now
quantized to Q8_0 once per projection and the inner loop is an exact integer dot product per 32-weight
block scaled by `d_w·d_a`, instead of decoding every block to f32. That is **1.9× per kernel** on the
host (135M decode 41.6 → 23.6 ms/token, 42.5 tokens/s; the integer kernel now beats the *dense* f32
kernel in the same crate), and it removes ~2/3 of the inner loop's f32 operations, which is what the
cell actually pays for. Cost, recorded rather than hidden: the engine is no longer bit-exact against
f32 accumulation over dequantized weights — the integer sum is exact and the only error is the
activation's half-step, pinned by a derived-bound test — and the golden reference in
`scripts/gen-ai-test-model.py` now mirrors the shipped arithmetic while keeping the same eight token
ids (weakest greedy margin 0.38 vs the fixture's 0.05 floor).

Spec 24 CP-3's gate asks for "QEMU RV64/ARM64 and RPi3 memory budget validation", and only the RV64
half had ever run. The oracle is now parameterized by architecture (`--arch riscv64|aarch64`) and the
CI job is a two-leg matrix (`.agents/260914-ai-oracle-arm64/`): the golden ids reproduce on aarch64 —
whose cell target is *softfloat*, so its float arithmetic is the compiler's software routines — which
makes that leg a numerics cross-check on a second ISA rather than a second boot test, and the same
image generates coherent prose from a real 26.7 MB checkpoint on both. CP-3's RPi3 memory-budget leg
stays open: it needs the board, and a QEMU result is not a substitute for it.

The board leg is now running over the existing static-TFTP netboot lane
(`tools/rpi3-netboot/serve-ai-oracle.ps1`): U-Boot pulls the payload, init starts `/bin/ai`, and the
oracle is driven from the board's console. **The fixture passes on real Cortex-A53 silicon**
(golden ids and embedding reproduced, 8 tokens in 59 ms, `.agents/260914-ai-oracle-arm64/`), which is
the lane's first exact-device evidence. Two board-only findings are recorded there: a 43.6 MB payload
(model embedded in VIFS1) panics the kernel at compositor setup where the same image passes on QEMU
aarch64 virt — reported to the board lane — and the shell mangles a path argument into
`/bin//bin/<name>`, so the console command is the bare `ai-test`. The checkpoint therefore lives on
the card (`/mnt/sd/ai-model.gguf`, the service's second candidate path) instead of inside the image —
and a third board run with a real 1.1 MB checkpoint (`stories260K`) reproduced the whole path at the
board ceiling: `[ai] model bytes: 1185376 read in 70 ms`, `model ready: 512 vocab, context 2048,
resident bytes 1220116`, `[ai-test] PASS`. That is the RPi3 memory-budget datapoint for a real
checkpoint, and the 25.6 MiB case followed: the large-payload panic turned out to be the board's
*static* fallback memory map declaring a fixed 16 MiB kernel region (proven by printing the map on
the board), fixed by sizing that region from the linker end symbol — after which the 43.6 MB payload
boots and reports `[ai] model ready: 32000 vocab, context 128, resident bytes 28087038` with
`[ai-test] PASS`.

## Cell-native portability program (completed)

[ADR-0018](../decisions/0018-cell-native-portability-and-runtime-profiles.md) settles how Linux
applications become native Cellos applications without a Linux personality: POSIX is translated in
userspace, porting has three lanes (relink / embed the library / Tier 3 guest) selected by the
application's POSIX profile, a language is admitted as a runtime profile under five conditions
(`cpp-freestanding` is next), and the kernel gains exactly three primitives — per-task TLS, a futex
ABI, and a pipe object. [ADR-0019](../decisions/0019-tier2-admission-control-on-path.md) settles the
control that gates Tier 2: one on-path policy, feature-selected capability only, an explicit default
posture per build profile, and predicates that cover the architectures the route supports. It also
records the shipped truth: the policy module is not on the path today, while the class-based route
runs on the default `native-domains` feature.

Execution is queued in
[.agents/260922-1549-cell-native-portability-program/plan.md](../../.agents/260922-1549-cell-native-portability-program/plan.md)
(7 phases). **Phase 01 is complete** at the `qemu` ceiling: the admission control now sits at the
single publication point (`task::launch::publish_prepared`) with a held-and-rechecked lease, a
denial that never falls back to SAS, architecture coverage that matches the route, a
MMIO/DMA/device-authority ceiling, and an explicit per-profile posture (development enables;
`policy-required`/`production-relay-image` leave admission disabled and deny domain-class
artifacts). Evidence: `S22-RV64-ADMISSION-{ENABLED,DENY,DRAIN,PUBLICATION-DENY,CEILING}: PASS`
across five cases of `scripts/qemu-native-domain-test.sh`, and `tier2-fault-isolation` 5/5 PASS
against a rebuilt kernel. The operator-facing `DRAINING` trigger is still absent (the transition
is in-kernel and selftest-exercised). **Phase 02 is complete**: `cpp-freestanding` ships as a
language subset whose C++ ABI runtime is the POSIX shim itself, with reference cell
`cells/tests/cpp-smoke` and runner `scripts/qemu-cpp-smoke.sh` (`CPP-SMOKE-QEMU: PASS`, 9/9
assertions on RV64; AArch64 build clean; x86_64 refused by design because the shim's C++ ABI layer
is complete**. **Phase 03 is complete**: `SetTlsBase` gives each task its own user thread pointer
(RV64 trap-frame `tp`, AArch64 `TPIDR_EL0`, x86_64 `FS_BASE`), and a thread inherits its creator's
base. The scheduler publishes it under lock; AArch64/x86 install it before fresh `__trap_exit`
entry and after switched-back resume; RV64 writes the child trap-frame carrier before scheduling.
`scripts/qemu-tls-test.sh` verifies the RV64 one- and two-hart cases; its domain switch,
migration, and fast-path regression suite stays green. C `__thread` remains a documented gap: it
needs loader `PT_TLS` exposure plus per-thread block placement, which the kernel primitive unblocks.
**Phase 04 is complete**: `FutexWait`/`FutexWake` (17/18) park and
wake by `(address space, generation, address)` — a Tier 2 peer cannot wake a wait it does not
share — with the deciding read under `SCHEDULER` through a validated copy view, deadlines on the
existing sweep, and waiter cleanup on exit. Verified on RV64 with one and two harts
(`scripts/qemu-futex-test.sh`: 10 000 serialised increments and 2 000 ping-pong rounds with zero
deadline expiries, plus timeout/mismatch/invalid-address cases), and the `futex-key` boot selftest
proves the key's domain discrimination. Two self-inflicted bugs were found and fixed during the
phase (a copy-view self-deadlock under `SCHEDULER`, and the shared timeout block clobbering the
futex outcome). The shim-level pthread surface stays with the porting kit (phase 06). **Phase 05
is complete**: fixed-capacity kernel pipes and endpoint ownership cross two independently admitted
Tier 2 domains through `/bin/pipe-test` and `/bin/pipe-peer`; RV64 QEMU harts 1/2 pass 1,024
ordered bytes through a 256-byte ring, exact EOF, `BrokenPipe`, unauthorized denial, and measured
drain/wake markers. **Phase 06 is complete**: the generated 200-symbol shim contract has
injected-drift checks; `port-platform` safely owns VFS/TCP/compositor/input/time clients; and the
external CMake/Meson smoke archive runs through `posix-shim-test` in RV64 QEMU. **Phase 07 is
complete**: Tetris-C is the Class-A witness, `c-pthread` is the Class-B witness
(`scripts/qemu-c-pthread.sh`: `C-PTHREAD-QEMU: PASS` after a two-worker condition-variable handoff
and 32 immediate create/join reuse cycles), and the follow-on C child-process adapter is the
Class-C witness (`libs/port-platform/{include/,}cellos_spawn.{h,c}`; `scripts/qemu-c-spawn.sh`:
`C-SPAWN-QEMU: PASS` on harts 1 and 2). The adapter composes an exact reviewed launch edge with the
staged command line, explicit `PipeShare` endpoint grants, and `Wait`, and proves argv delivery, an
ordered endpoint payload, the child's status, a denied unreviewed target, and the refusal of an
over-long command line; it is not `fork`/`exec`/`posix_spawn`. The reference-port closure re-ran
the class-B and class-C selections and published raw logs under `docs/evidence/`
(`c-pthread-qemu.*`, `c-spawn-harts{1,2}-qemu.*`) with Tier-2 admission markers, artifact sizes, a
1,307-line phase-authored surface, and a 3.29 h measured artifact window. A third-party port that
needs a `fork`/`exec` process tree remains a recorded class-D blocker. No phase carries a physical,
fleet-secure, or production claim.

## Runtime capability revocation (completed)

`sys_cap_revoke` (219) used to be a label change: it cleared TCB fields and left every
authority already handed out live — exactly the stale-authority retention Spec 16 cites the
J-Kernel proof for (LBI prevents forgery, not revocation). The program in
[.agents/260712-1901-cap-revocation/plan.md](../../.agents/260712-1901-cap-revocation/plan.md)
is closed at the `qemu` ceiling. `iommu::unmap_dma` is real (lookup-only leaf clear plus
IOTLB/IOFENCE acknowledgement, with `iommu::revoke_dma_for_cell` shared by cell death and
runtime revoke), `reclaim_owned_grants` reclaims a live Cell's owned grants while
quarantining frames an in-flight pin still holds, MMIO revocation releases the window in the
resource registry and removes its *user* accessibility (`paging::revoke_mmio_user`: user PTE
cleared on x86, permission lowered on riscv64/aarch64 so the kernel keeps its identity
mapping), and `pcie_driver`/`platform`/`supervisor` became revocable through three additive
`cap_mask` bits (Law-1 confirmed twice) with their DMA-domain/BDF/BAR/ECAM teardown. The
victim is told with `AppEvent::CapRevoked` on the newly registered `0xF2` envelope.
`HYPERVISOR` stays refused — the one ambient authority with no teardown path. Evidence: one
RV64 QEMU run (`scripts/qemu-native-domain-test.sh --harts 1`, kernel `76832e05…`) carrying
`IOMMU-TEARDOWN-*`, `GRANT-RECLAIM-*`, `MMIO-REVOKE-*` and the `thread-cap` revoke aggregate;
raw log `docs/evidence/cap-revoke-qemu.{log,txt}`. Recorded, not hidden: no Cell issues
`CapRevoke` yet (the end-to-end path is witnessed in-kernel), the DMA-fault oracle needs real
IOMMU hardware, and the x86/aarch64 MMIO legs are compile-verified only.

## Cell scale profiles (D5)

The per-request server profile (Spec 19 §3) is an accepted goal, not current capacity, and the
large-app profile remains the default with `MAX_CELLS = 64`. The 2026-07-31 measurement stopped
at **n = 8–9** parked cells with `MAX_CELLS` already raised to 512 and all 512 VA slots free; the
binding ceiling was a **hardcoded 190 MiB RAM map**, not per-cell cost — so the profile "cannot be
reached by raising constants". That prerequisite has since landed: RISC-V builds its memory map
from the firmware DTB (`kernel/src/boot/dtb_memory.rs`) and `MemInfo = 243` returns
`ViMemInfoV1 { total, used, free }`, so capacity is measurable from userspace for the first time.

Still open, in the order the measurement established: shared immutable `.text`/`.rodata` frames
(the loader still copies the whole ELF per spawn), demand-paged stacks, and — last — raised
`MAX_CELLS`/`MAX_SLOTS`. The staged N = 64/128/256/512 gate has not been re-run since the memory
map landed, and it must be measured **with heavy cells resident** to describe a mixed deployment;
a variable VA budget (fixed 32 MiB stride today, `kernel/src/loader/va_alloc.rs:47`) is an
additional prerequisite for data cells. Runnable instrument: `cells/tests/bench/src/capacity-probe.rs`
(built only with `CELLOS_INCLUDE_CAPACITY_PROBE=1`) plus
`tests/integration/tests/capacity-observability.rs`; no committed N-sweep scenario exists.
This remains `qemu`-ceiling work.

## Current executable work

- Continue useful QEMU software and integration work to the `qemu` ceiling.
  Host/QEMU results never qualify a board, secure root, cloud authority,
  physical-hostile posture, or production release.
- Use both owner-reported Raspberry Pi 3 Model B+ boards for G1 boot and
  peripheral integration work.
  The HDMI external-display lane has completed its software and exact-device
  development gates and is regression-only. Its `lungmat8` approval and strict
  software checks do not promote the result to production qualification or
  globally block other RPi3 work.
- Defer camera and other sensor integration until the sensor lane is resumed.
  The camera's exact identity and interface must be recorded before it is
  exercised or used as physical-behavior evidence.
- The x86 Tier 3 hostile path now passes 27 bounded, origin-separated scenarios
  under pinned QEMU-TCG 10.2.0, including transport/queue/descriptor rejection,
  reset, independent pause-less vCPU preemption, and VFS/Net supervisor restart
  with backend recovery. ARM64 hostile execution remains blocked by the known
  synchronous TCG fault before the guest probe; rerun that corpus only in an
  environment that reaches the probe.
- The Tier 3 wide-guest Ubuntu/glibc substrate is implemented: stable service ID
  14 (`HYPERVISOR_SERVICE_ID` in `libs/api/src/abi/hypervisor.rs`) registers the hypervisor on spawn, the kernel
  auto-clears dead registrations on task exit, VFS grants the live provider
  preallocated fixed-capacity write access to `/mnt/sd/guest_disk.img` without
  quota charging while forbidding file growth, whole-file write, and recursive tree
  deletion across `/`, `/mnt`, `/mnt/`, `/mnt/sd`, `/mnt/sd/`, and
  `/mnt/sd/guest_disk.img`, and `ubuntu-wide-guest` enables 512 MiB RAM and
  root-on-blk `/dev/vda` ext4 systemd multi-user boot. The reproducible
  Canonical Noble 24.04 image builder and two-boot persistence runner are pinned
  and fail-closed. Execution of the two-boot apt-persistence and full systemd
  multi-user assertions remains blocked on the remaining external prerequisite:
  host root for rootfs creation.
- The AArch64 test-hooks semihosting ledger closure is complete. Blocker
  `B-AARCH64-SEMHOSTING` was corrected to subject `qemu-arm64` and resolved to
  PASS under schema v4 following independent ratification on Issue
  [#47](https://github.com/dxsl-org/cellos/issues/47) by repository collaborator
  @datgausaigon (`DECISION: YES`). The resolution binds fresh QEMU runtime
  artifacts (`docs/evidence/aarch64-semihosting-20260903-03-raw.txt` and
  `docs/evidence/aarch64-semihosting-20260903-03-runner.txt`). Acceptance-ledger
  production Phase 3 remains `PLANNED`.
- The caller-scoped shell `cd`/`pwd`, bounded truthful `fstat`, Phase 05 atomic `rename`
  backend gate, and Phase 06 pinned-QEMU x86 compatibility lanes are complete;
  POSIX documentation repair is complete and ARM64 hostile execution remains isolated.
- Single-guest local Cell-to-Cell evidence is now required through the
  [CI workflow](../../.github/workflows/ci.yml) job
  `c2c-broker-oracle-single-guest-local-runtime`, displayed as
  `C2C Broker Oracle (single-guest local-runtime QEMU)`. The job allows
  60 minutes, limits the oracle step to 40 minutes, and uses an `if: always()`
  upload for the runner log on ordinary success or failure.
  `cell_main` loads K1 through a bounded path, initializes authenticated beacon
  state, and uses deadline- and cancellation-bounded service-net admission
  whose start/finish is linearized with shutdown. Queued IPC now interrupts a
  parked `WaitCompletion` through the existing raw return `0`, with no
  completion record. The public completion ABI and source vocabulary remain
  `NET_RX` and `TIMER`; no IPC completion source was added. Kernel
  park/publication linearization remains under `SCHEDULER`, outgoing-context
  handoff is armed before wait-state publication across `Send`, post, and
  `TrySend`, and NET_RX `Completing` ownership is preserved.
  Service-net retains its finite 10-tick (about 100 ms) smoltcp maintenance
  wake and exactly one production grace yield (`grace=1`). The canonical gate
  now requires the exact kernel `IPC-PENDING` completion-wake and
  `NET-RX-RESERVATION` IPC-safe PASS markers with no corresponding FAIL before
  launching every post-command benchmark gate; it does not wait for a runtime
  timing PASS before launch. Final whole-run parsing nevertheless requires at
  least one exact raw-zero same-cycle PASS below the exclusive `900000`
  ceiling. A drain at or above the ceiling is neutral INCONCLUSIVE: it neither
  satisfies nor itself fails the gate, while INCONCLUSIVE-only output cannot
  pass. Clean-source commit `59501e2b` passed one canonical
  `scripts/run-c2c-broker-oracle-qemu.sh` invocation (exit 0, 1/1), including
  the exact `[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe,
  completion-wake)` and `[selftest] NET-RX-RESERVATION: PASS (fills, remembers,
  releases, IPC-safe)` markers with no corresponding FAIL. Runtime cycle 36
  reported `start_ticks=144911300`, `raw_ret=0`, `elapsed_ticks=586804`,
  `proof_ceiling_ticks=900000`, `budget_ticks=1000000`, and `status=PASS`;
  no INCONCLUSIVE marker appeared. This mandatory runtime observation is
  supplemental and non-causal. The measured baseline completed 1000/1000, the
  1/2/4/8/16 sweeps passed, the soak completed 10000/10000 with positive
  network progress and zero heartbeat/watchdog deltas, overflow and restart
  passed, and no forbidden oracle or runtime marker appeared. These remain
  local/QEMU classifier and benchmark results, not a physical timing bound or
  evidence that the timing observation caused the benchmark result. This work
  proves no two-node direct LAN, relay, remote session cleanup, remote/public
  operation, service deployment, physical execution, protected relay identity,
  or production completion.
- The broker's stable-identity consumer now uses the existing opaque KMS
  static-DH seam. It accepts only matching ready register/status/acquire
  snapshots and gives Clatter handle/epoch/public metadata; the private scalar
  never enters broker or VFS state. Plaintext VFS `machine-id` is not a C2C
  identity root. KMS absence, non-ready provider state, or any mixed snapshot
  selects an ephemeral local-only identity and keeps remote disabled.
  Operator recovery is now fixed to a live-supervisor, exact nonzero-revision
  compare-and-swap contract; clone/lost-key states cannot auto-rotate or restore
  plaintext identity. Qualified provider execution and physical recovery
  evidence remain open.
- API tests pass 91/91, service-net host tests pass 30/30, the deterministic
  kernel completion-wake boot gate passes 1/1, and fresh RV64 builds pass;
  Candidate B local ingress remains complete at its source/host boundary. The
  clean-source combined QEMU result is recorded above. The focused `ostd`
  completion decoder and bounded `read_file` regression pass. The package-wide
  host command now passes with 24/24 unit tests, 5/5 `cluster-endpoint` tests,
  and 19 doctests passed plus 2 intentionally ignored: 48 passed, 0 failed.
  Enrollment, lease renewal, and routing remain unwired or unreachable from the
  broker dispatch path.
  This does not prove two-node, relay, direct-LAN, remote restart/failover,
  service deployment, physical execution, provider qualification, or
  production readiness.
- Phase 04 local protocol contract is complete; remote dispatch stays disabled.
  The allocation-free V1 envelope uses a 112-byte header and a
  3,712-byte end-to-end payload cap across local ingress, Noise, and net-cell IPC.
  Its fixed 16-entry, 30-second dedup cache never evicts or redispatches in-flight
  work. Sixteen authenticated source/boot replay floors keep stale boots and
  evicted old ids `Indeterminate`. Boot-local server epochs and explicit typed
  local/remote endpoints are now defined; a shared validated nonzero relative
  deadline is mandatory in both envelope and remote-call API, whose disabled
  boundary returns `NotSupported` without broker contact. Hostile canonical
  decoder properties, monotonic deadline semantics, and epoch-before-dedup
  receive ordering pass; strictly increasing replacement retires dead response
  entries while preserving replay floors, and same/lower epochs fail without
  mutation.
  Focused broker tests pass 92/92, endpoint integration tests pass 5/5, and
  RV64 broker/`ostd` builds pass. Phase 04 is complete at the disabled
  local-only ceiling; provider qualification and authenticated cross-broker
  incarnation binding gate Phase 05 remote dispatch, relay, and direct LAN.
- Phase 05's bounded local contract portion is complete without relay
  enablement. The four-session Noise pool preserves occupied sessions and
  returns `WouldBlock` before `TcpConnect`; paired prologue, relay-endpoint,
  admission, reconnect, and server-framing regressions pass. Authority-owned
  client framing and correlation integration remain blocked until the exact
  Phase 4 entry GO. After that Build work, AC-012 gates relay enablement,
  relay receive, the two-node oracle, and phase completion.
- Project each completed lane immediately into the roadmap and acceptance views
  at its exact evidence ceiling.
- The managed-surface child is complete at the QEMU ceiling. Its dedicated
  RV64 oracle passes generated Counter repaint, pointer interaction,
  maximize/restore geometry, accepted close, and pointer-established Enter
  activation after restore; the separate compositor `window-policy` QEMU
  regression also passes. The signed image gate passes F1/F5 after removing
  three unapproved unsafe islands. No physical, production, or additional
  desktop contract is authorized.
- The authenticated software-evidence pipeline is complete and regression-only
  at the `host` ceiling. GitHub-hosted `main` run `33251921677:1` at revision
  `d951d7dbf191133e94061ded7f0a8d17bfcf07c8` completed. Its manifest digest
  was independently verified, the run-id/attempt sequence was consumed once
  through explicitly provisioned durable operator-owned external state, and
  exact replay was rejected. This authenticates bundle origin and integrity
  only. Every bundled
  result retains its own evidence ceiling, and no physical, secure-root, cloud,
  approval, admission, or production status changes.

## Work classification

- **Current executable work:** the QEMU, two-Model-B+ non-HDMI peripheral,
  local Cell-to-Cell, evidence-projection, sensor, and separately reopened
  governed lanes above. Camera and other sensor integration retains this
  classification but is deferred in the current session order.
- **Completed / regression-only:** the RPi3 HDMI software and exact-device
  development lane, the x86 Tier 3 VirtIO software lane at its QEMU ceiling,
  and the authenticated software-evidence pipeline at its host ceiling. Reopen
  them only for a regression or separately governed higher evidence.
- **Current-scope technical debt:** confirmed defects and maintainability gaps
  in supported paths, including kernel signature, pointer, and entropy
  remediation tracked by the [open risk register](open-risk-register.md). This
  label does not apply to completed/regression-only lanes or all advanced work.
- **Future capability:** remote/public Cell-to-Cell operation, additional
  desktop and x86 platform depth, per-request cell scale (D5, Spec 19 §3),
  G3 accelerators, G4 `rust-std`, and G5
  virtualization expansion.
- **External-gated prerequisite:** unavailable exact boards, protected relay
  assets/cloud identity, and an exact production-root vendor evidence package.
  No stock TPM or generic secure-element counter is selected as the production
  floor.
- **Production release gate:** remote C2C identity where applicable, protected
  relay identity, production KMS/root, secure/measured boot, a qualified
  rollback-resistant external floor, persistent recovery, physical hostile
  evidence, an authenticated runner, required human approvals, and governed
  release-ledger closure.

Production admission and release remain disabled and fail-closed until every
applicable production release gate is satisfied. Those gates block only the
production-admission or production-release milestone that owns them; they do
not block the current executable work above. Precise owners and reopening
events are maintained in
[the roadmap capability table](../project-roadmap.md#capability-lanes).

## Recent State

- Current inventory comprises `2 × Raspberry Pi 3 Model B+` as reported by the
  owner. No provisional board labels are assigned. The exact serial, revision,
  and condition of both boards—and their relationship to prior captures—remain
  unresolved pending reconciliation.
- A prior exact-device run reported board revision `a22082` / `RPI 3 Model B`
  and unique serial `000000003d042795`; it is not assigned to either current
  Model B+ board. On 2026-08-28, `lungmat8` approved that run's exact BCM
  mailbox unsafe island and strict F1/F5 passed. The independent repository
  TFTP log records the final 9,642,048-byte reviewed-image transfer at
  2026-08-28 11:14:54. Separately,
  `.agents/debug/rpi3-b-hdmi-reviewed-20260828.raw` contains an earlier boot at
  lines 37–210 and a later reviewed-image boot beginning around line 253. The
  later block records one 4,096-byte mailbox page, successful cache begin/exact
  completion, framebuffer base `0x3e876000`, size 3,686,400, 1280x720, pitch
  5,120, driver registration, fb-console damage, and a completed first scanout
  flush without a cell fault. The UART file has no host timestamp or image hash
  and does not itself prove the 2026-08-28 11:14:54 TFTP event. The user
  separately observed the cold-connected display remain lit for more than
  10 minutes with fb-console and cursor movement. This closes the HDMI visual
  gate for that exact captured device at development evidence only; it is not
  production qualification. The earlier late-connect black / `No Signal`
  observation remains only a reproduction condition, not a root-cause finding.
- The historical shell/BCM-scanout capture remains unassigned because it
  contains no unique serial and therefore cannot be mapped to either current
  Model B+ board.
- RPi3 post-HAL-split smoke work has landed in `main`.
- HAL to kernel Rust ABI signatures are centralized in
  `hal/traits/arch/src/kernel_abi.rs`.
- Root `boards/` is the owner for board descriptors and fallback assets.
- SoC immutable facts live under `hal/soc/*`.
- Shared drivers remain single-copy in kernel integration paths or
  `cells/drivers/*`; boards do not fork UART, SDHCI, GIC/PLIC, PCIe, or
  DesignWare-style mechanisms.
- Cell-to-Cell Anywhere has landed its bounded local broker and fail-closed KMS
  foundation. Remote/public operation remains disabled while the production
  hardware-backed root and trusted monotonic epoch are unavailable.
- Tier 1 admission prequalification now has its canonical 18-row catalog, all
  33 stable `test-hooks` IDs, and a strict runtime parser. This is test
  infrastructure only: local runs are non-admissible, production admission is
  disabled, and Phase 04 remains blocked.
- Manifest-v2 tooling Phase 05 is complete. The loader now classifies a unique
  manifest section as `Absent`, `Valid` (v1 or v2), or `Malformed` before task
  creation; only genuine absence selects the explicit legacy path policy.
  Rust v2 remains exactly 16 bytes and Zig v1 exactly 8 bytes, with compatible
  upcast behavior and protection-class terminology separated from application
  execution tiers.
- The Phase 07 atomic-publication prerequisite is verified, not full Phase 07
  completion: a fresh `test-hooks` build/sign, a populated-fixture one-hart VFS
  run (1/1; AP-00–11 and AP-15; AP-13 explicitly `SKIP`), and an SMP atomic
  run (1/1; AP-00–15) passed. The SMP proof includes AP-02 live-PTE/TLB
  restoration evidence, an AP-13 remote-hart scheduler witness, and the
  terminal/aggregate markers. Its terminal state remains
  `ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED`.
- Phase 08 Manifest-v3 ABI predesign is validated (20/20), with pinned consumer
  inventory and content digests. Its state is
  `PREDESIGN_COMPLETE / PHASE08_BLOCKED`: it depends directly on Phases 03, 05,
  and 07 and adds no Manifest-v3 code, readiness claim, or approval.
- Full Phase 07 and Phase 08 remain blocked by the Phase 03
  provenance/signature boundary, the Phase 04 production-admission gate, and
  the Tier 2 native-domain gate. The verified atomic prerequisite does not
  clear those release conditions.
- `CELLOS-VFS-SMP-006` is closed after the owner-lifetime lifecycle
  implementation passed API90, an RV32 release compile, fresh `test-hooks`,
  one-hart VFS 2/2, and two-hart VFS 7/7. Final quality and security closure
  both passed. RV32 runtime remains unavailable on this host because OpenSBI
  firmware is missing; that compile-only evidence gap is non-blocking and is
  not a runtime claim.
- RV64 native-domain substrate and scheduler transitions (Spec 22 Items 2–3)
  have passed one-hart (`switch`, `sas-fastpath`) and two-hart (`migration`)
  QEMU evidence runners. AP-13 pre-ready quota drain race, release-build supervisor
  unregistration, and SMP UART timing were resolved. Production admission remains
  disabled, SAS remains default, and no Manifest v3 or ledger qualification claims
  are made.
- RV64 QEMU desktop has an implemented bounded window-policy scenario.
  Interactive surfaces set bounded titles and poll typed lifecycle events beside
  their captured pointer and selected-owner keyboard input. The compositor owns
  clipped frame/title/control decoration, titlebar drag, edge/corner resize,
  minimize/maximize/restore controls, and explicit close negotiation; client
  content coordinates remain unchanged.
- Resize, maximize, and restore commit only after the owner applies a
  replacement Grant and acknowledges the matching configure serial. Minimized
  surfaces are not paintable or hit-testable until restored; an accepted close
  is removed when its owner destroys the surface. `SurfaceRole::Background`
  remains visible but cannot hit-test, raise, or use decoration controls.
  The `window-policy` scenario retains QMP/PPM background, capture, and
  keyboard-focus coverage while adding lifecycle paths; the separate
  compositor-cursor scenario retains cursor coverage. This is still not a
  desktop shell or G2 qualification: taskbar, snapping, persistence, and live
  resize preview remain absent.

## Current Documentation Corrections

- Spec 21's Layer-3 mechanism is live: `scripts/check-spec-anchors.py` resolves every
  `Anchor:` line under `docs/specs/` against the tree and generates
  `docs/spec-status.generated.md`, which is now the status home specs link to instead of
  carrying status prose. The `lint` CI job runs it warn-only plus a staleness gate;
  `--strict` turns on enforcement once the rollout step-2 backfill reaches zero coverage
  gaps. At 2026-09-22: 19 sections anchored (`19-` and `21-`), 0 anchor violations, 256
  ratified sections still unanchored, 81 status-prose hits remaining.
- Spec 19's §2 previously named `task::spawn_from_mem`, which does not exist; the
  enforcement path is `kernel/src/loader/wx.rs::enforce`, called from `task::elf_prepare`
  with the gated entry `loader::mem_spawn_gate::spawn_from_mem_gated`. The section now
  carries `impl` and `test` anchors so a rename cannot land silently.
- MicroPython is historical, not an active workspace runtime.
- Cargo workspace count is generated/discovered data; avoid hardcoding old
  counts except in generated metrics.
- `docs/TODO.md` is no longer project documentation. Personal task tracking
  belongs in `.agents/`.

## Next-session work order

1. AArch64 semihosting ledger closure is complete: Issue #47 ratified
   `B-AARCH64-SEMHOSTING` resolution to PASS under schema v4 with bound QEMU
   evidence. Acceptance-ledger production Phase 3 remains PLANNED.
2. Independently continue any ready lane, beginning with both available
   Raspberry Pi 3 Model B+ boards: record each exact serial, revision, and
   current condition, then reconcile whether either corresponds to the prior
   `a22082` / Model B / serial `000000003d042795` run. Record the available
   camera's exact identity and interface without starting sensor integration.
   Buy no additional hardware.
3. Exercise the existing RPi3 boot/peripheral path on the reconciled current
   boards and retain development-only logs tied to the exact board. Do not
   infer a production-security or external-floor result.
4. Preserve the completed bounded HDMI path: cold-connect and power the display
   before firmware startup, retain separate exact-board UART and TFTP evidence
   records for future regressions, and do not promote the prior exact-device
   development result to production qualification.
5. Publish each observed result at its evidence ceiling with the remaining
   lane-local gate. Continue local Cell-to-Cell baselines if the hardware lane
   is waiting on physical access or a named review.
6. Resume camera and other sensor protocol, board-interface, driver, fixture,
   QEMU, and exact-device RPi3 work only in a later sensor session. Keep QEMU
   results software-only and physical results development/hardware-integration-
   only.
7. Keep protected relay assets, other physical boards, G3 acceleration, and the
   ADR-0006 production root external-gated. Keep every production-admission and
   release invariant mandatory without making it a global development blocker.
8. Keep HAL/board boundary checks in CI whenever board descriptors, SoC facts,
   or HAL ABI hook declarations change.
9. Use [project-roadmap.md](../project-roadmap.md#capability-lanes) for
   cross-lane routing and the topic pages for evidence details.
