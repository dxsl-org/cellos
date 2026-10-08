# Cellos Project Roadmap

**Project**: Cellos (Jarvis Hybrid OS)
**Current version**: 0.2.1-dev (Mycelium Era)
**Current direction**: C2C Anywhere on Intel x86-64 — sole active program
**Last updated**: 2026-10-08 (ADR-0022 direction cutover)
This file is the roadmap entrypoint. The previous all-in-one roadmap is
preserved as a read-only content snapshot at
[project-roadmap-legacy.md](project-roadmap-legacy.md). Use it only when a
historical decision is not represented by the current topic pages.

## How to Read the Roadmap

| Need | File |
|---|---|
| What is active now | [roadmap/current-focus.md](roadmap/current-focus.md) |
| Hardware qualification lanes | [roadmap/hardware-tracks.md](roadmap/hardware-tracks.md) |
| Historical product-stage definitions G1-G5 (parked) | [roadmap/product-stages.md](roadmap/product-stages.md) |
| Runtime assets and C2C dependency boundaries | [roadmap/runtime-and-platform-tracks.md](roadmap/runtime-and-platform-tracks.md) |
| Technical milestones and historical status | [roadmap/technical-milestones.md](roadmap/technical-milestones.md) |
| BEAM/OTP parity and backend roadmap (analysis) | [roadmap/beam-parity-backend-roadmap.md](roadmap/beam-parity-backend-roadmap.md) |
| Completed history ledger | [roadmap/completed-history.md](roadmap/completed-history.md) |
| Known open risks and deferred gates | [roadmap/open-risk-register.md](roadmap/open-risk-register.md) |
| Immutable pre-split snapshot | [project-roadmap-legacy.md](project-roadmap-legacy.md) |

## Execution Model

[ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md) makes **Intel x86-64
C2C Anywhere the only development direction**. All tasks must identify the C2C
deliverable, required dependency, measured bottleneck or regression they address,
with an observable acceptance scenario and evidence ceiling. No direct connection
means parked. Older G1–G5 labels and plan-local `ready` statuses cannot authorize
independent robotics, GUI, AI, runtime or hardware programs.

The canonical evidence ladder is `none → contract → host → QEMU → physical →
service → production`. `execution_class` describes whether a lane may run now:
`ready`, `scope-gated`, `contract-gated`, `governance-gated`, or
`external-gated`. `evidence_ceiling` describes the strongest result the next
slice may truthfully claim. QEMU and host evidence never promote a lane to
physical, service, or production qualification.

The [current focus](roadmap/current-focus.md) projects the outcomes; the
[portfolio](../.agents/plan-portfolio.md) selects child work with one implementation
slice at a time. LAB-01 / BASE-01 / ASSEMBLY-01 and general organizational
server/office replacement are parked. Earlier results and contracts are retained,
not prerequisites for the Intel C2C program.

[ADR-0015](decisions/0015-dual-mode-hybrid-architecture.md) settles the
Dual-Mode Hybrid Architecture (Real-time SAS Tier 1 + Paged Domain Tier 2 + VM Guest Tier 3),
balancing pure-Rust zero-copy performance with hardware memory isolation for C-FFI
and unsigned code, governed by the [evolution plan](../.agents/260906-dual-mode-kernel-evolution/plan.md).

### Development-first hardware-constrained decision

[ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md) supersedes
[ADR-0007](decisions/0007-development-first-hardware-constrained-execution.md)
only for hardware focus and independent-lane scheduling. Target one exact Intel
headless machine, then a separately qualified second Intel node after first-node
qualification; matching configuration is preferred, not mandatory. Existing x86 controller QEMU evidence is not a qualified physical
machine; Intel VMX and x86 Tier 2/C++ gaps remain required work.

ARM/RPi3/RISC-V/AMD code and evidence stay in-tree, but new platform/peripheral
work is parked. Existing non-Intel regressions may protect shared C2C changes;
they do not reopen a platform program. Software work can proceed before hardware
procurement, and local work need not wait for unrelated production gates.

No acquisition, paid service, irreversible provisioning, ABI change, remote
enablement or production admission is approved by the direction change.
Protected authority, identity/time/replay, signing, secure/measured boot,
qualified-floor, independent-human and release-ledger gates remain mandatory
for their applicable milestones. No insecure demonstration fallback is allowed.

### Solo-first development and independent promotion

[ADR-0013](decisions/0013-solo-first-development-independent-promotion.md)
allows the sole accountable maintainer to perform every development role,
including planning, implementation, testing, self-review, documentation, and
development release. AI agents and CI jobs provide automated assurance only;
they are not independent accountable identities. A missing independent-member
approval blocks only the independently ratified or production claim that names
it, never otherwise-admitted C2C work below that evidence ceiling. When such approval is
required, a repository member distinct from the maintainer must answer an
explicit `YES` or `NO` on the GitHub issue or pull request bound to the exact
proposal, commit, and evidence.

### Planning classes

- **Current executable work**: an admitted Intel C2C slice supported by present
  software/assets and its technical, ownership and approval gates.
- **Current-scope technical debt**: a defect or maintainability gap in that path
  or its shared baseline, not every unfinished OS capability.
- **Parked / future capability**: unrelated product, platform or runtime work;
  an old ready checkbox or newly available board does not authorize resumption.
- **Completed / regression-only**: a delivered lane whose stated evidence
  ceiling has passed and has no active implementation slice; reopen only for a
  regression or a separately governed higher evidence class.
- **External-gated prerequisite**: work that cannot cross its next evidence
  boundary until a named external asset, product, account, or vendor package
  exists.
- **Production release gate**: a mandatory production-admission or
  production-release invariant. It does not serialize non-production
  development.

## Capability Lanes

| Current C2C workstream | Required result | Entry boundary |
|---|---|---|
| Contract and local service lifecycle | Typed calls, authority, bounded waits, restart and stale-reference behavior | Tier-aware C2C Phase 01 review/ABI gates |
| Bounded Intel hardware | One qualified physical node, then two independently qualified Intel nodes with stable wired networking and persistence; matching preferred, not mandatory | Existing PC plan/HCL; procurement and exact-device qualification remain separate |
| Tier 2 C/C++ | Qualified x86 domain admission and runtime/adapters | Test-only admission and C++ runtime gap are not closed by this decision |
| Tier 3 Intel | VMX/EPT guest lifecycle, storage/network and explicit C2C bridge | VMX plan P09; existing SVM/TCG evidence is not Intel qualification |
| LAN and remote/relay | Two-node behavior, authority enforcement, restart/reconnect and explicit uncertain outcomes | Each transport's identity/security contract; no automatic remote activation |
| Useful measured workload | All-tier participation and same-hardware Linux comparison | Reproducible workload/semantics before optimization or advantage claims |

The detailed next slice is in the portfolio, not all rows running at once.

### Historical capability inventory — not scheduling authority

The table below retains pre-ADR-0022 planning/status evidence. Its old planning
classes, owner/next slices and reopening events are historical. They cannot
authorize work outside the current C2C workstreams; source and exercised evidence
still determine implementation truth.

| Capability | Planning class | Execution class | Evidence ceiling | Owner / next slice | Reopening event |
|---|---|---|---|---|---|
| Roadmap projection | Current executable work | `ready` | `contract` | Hardware-independent roadmap Phase 01/08 | A lane emits bounded evidence/status |
| General QEMU software and integration | Current executable work | `ready` | `qemu` | Owning runtime/platform lane | Exercise the supported software path; do not promote the result to physical, service, or production evidence |
| RV64 performance evidence | Current executable work | `scope-gated` | `qemu` | SAS/LBI closure Phase 04 retains three structurally valid but dirty-source-unbound generic captures and three unprofiled local C2C oracle repetitions | Add immutable dirty-source provenance, rebuild canonical compatible history, then recapture before acting on the diagnostic 76.08 MiB commitment and 9/s SMP spawn-rate misses; real grant-backed VFS rows and named-board evidence remain separate |
| LAB-01 / BASE-01 / ASSEMBLY-01 software contracts | Current executable work | `ready` | `contract`; host/QEMU only after the named milestone evidence | SAS/LBI 06A LAB-01 and 07A BASE-01 bounded private contracts pass independent host plants at the model-only ceiling; ASSEMBLY-01 is the next slice | Consume the reviewed shared identity/dispatch/observation/reconciliation contract for 08A; native QEMU roles wait for the real Phase05 backend/oracles |
| Robot physical workflow acceptance | External-gated prerequisite | `external-gated` | `physical` development target, unexercised | SAS/LBI plan 06C/07C/08C | Exact mechanism/controller/fixture/observation/metrology/safety package and applicable activation approvals; no procurement or motion authorization from the plan alone |
| Organizational server and office profiles | Future capability | `scope-gated` | `contract` | ORG-SRV-01 / ORG-PC-01 profile document | Actual application/hardware inventory, compatibility/disposition matrix and separately activated implementation/qualification lane; no prerequisite on physical robot completion |
| Intel x86-64 C2C physical substrate | Required dependency | `external-gated` | `qemu` controller evidence; no physical row | PC phases 01–06 have QEMU evidence; first exact Intel node needs separately authorized phase 07 capture | First Intel qualification then separately authorized second Intel node with independent capture; matching preferred, not mandatory; VMX remains separate; former Intel/AMD pair superseded |
| RPi3 HDMI software and exact-device boundary | Completed / regression-only | `scope-gated` | `physical` development evidence on the prior captured revision `a22082` / Model B / serial `000000003d042795` device; mapping to current inventory unresolved | Phases 04 and 05 completed; no active HDMI slice | Reopen only for a regression: the exact mailbox unsafe island is approved by `lungmat8`, strict F1/F5 passes, and the separately recorded TFTP deployment, later UART boot block, and user visual observation close the reviewed exact-device gate |
| RPi3 peripheral hardware integration | Current executable work | `ready` | `host` now; `physical` development evidence after exact-device exercise | G1 board/peripheral lane using the two available Raspberry Pi 3 Model B+ boards; HDMI external-display work is completed and regression-only | Reconcile each current board's exact serial, revision, and condition before attributing evidence; stop before any production-security qualification claim |
| Camera and other sensor integration | Current executable work | `deferred` | `contract` until resumed; then exact-device `physical` development evidence | Deferred in the current session order; the available camera must be identified before use | Resume the sensor lane in a later session and record the exact sensor/interface before exercise |
| Tier 3 hostile QEMU evidence | Current executable work | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 06; x86 bounds, descriptors, backend faults, reset, restart, and independent vCPU preemption pass in 27 bounded scenarios | Rerun the ARM64 hostile corpus only in an environment that reaches the guest probe past the known synchronous TCG fault |
| ARM64 Tier 3 persistent storage | Future capability | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 09 | Supported Phase 06 hostile scenarios; policy is fixed to `build/tier3-arm64-persistent.img` at 8 MiB with explicit cleanup |
| Tier 3 wide-guest Ubuntu/glibc substrate | Future capability | `scope-gated` | `host` / `qemu` | Stable service ID 14 (`HYPERVISOR_SERVICE_ID` in `libs/api/src/abi/hypervisor.rs`), live-provider VFS fixed-capacity `/mnt/sd/guest_disk.img` overwrite contract, 512 MiB PVH boot profile, reproducible Canonical 24.04 ext4 image builder, and two-boot runner implemented | Host-root rootfs build environment to execute the two-boot apt-persistence and full systemd multi-user assertions |
| Desktop, ViUI, and SDK | Future capability | `scope-gated` | `qemu` | Managed-surface child is complete at the QEMU ceiling | Resume the owning desktop lane only under separate approval; the dedicated RV64 oracle proves generated Counter repaint, maximize/restore, accepted close, and pointer-established keyboard activation, while `window-policy` remains a separate passing regression |
| Local Cell-to-Cell runtime | Current executable work | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 04; bounded Candidate B ingress and opaque KMS identity consumer wired; supervisor-only exact-revision recovery fixed; core Phase 04 local protocol contract complete at its disabled ceiling; Phase 05 local bounded portion complete with bounded K1 loading, authenticated beacon setup, cancellation-aware restart-safe admission, and IPC-aware `WaitCompletion`; qualified provider execution still gated | Required [CI job](../.github/workflows/ci.yml) `c2c-broker-oracle-single-guest-local-runtime` gates the runner. Queued IPC interrupts a parked wait through existing raw `0` with no completion record; the public completion ABI/source vocabulary remains `NET_RX`/`TIMER`, with no IPC completion source, and NET_RX `Completing` ownership remains preserved. Service-net keeps the finite 10-tick/about-100-ms maintenance wake and exactly one production grace yield (`grace=1`). Clean-source commit `59501e2b` passed one canonical `scripts/run-c2c-broker-oracle-qemu.sh` invocation (exit 0, 1/1), including the exact `[selftest] IPC-PENDING: PASS (deferred, bounded, quota-safe, completion-wake)` and `[selftest] NET-RX-RESERVATION: PASS (fills, remembers, releases, IPC-safe)` markers with no corresponding FAIL. Runtime cycle 36 reported `start_ticks=144911300`, `raw_ret=0`, `elapsed_ticks=586804`, `proof_ceiling_ticks=900000`, `budget_ticks=1000000`, and `status=PASS`, with no INCONCLUSIVE marker. That required runtime observation is supplemental and non-causal. The measured baseline completed 1000/1000, the 1/2/4/8/16 sweeps passed, the soak completed 10000/10000 with positive network progress and zero heartbeat/watchdog deltas, overflow and restart passed, and no forbidden oracle or runtime marker appeared. This remains a local/QEMU classifier and benchmark result, not a physical timing bound or evidence that the timing observation caused the benchmark result. API passes 91/91, service-net 30/30, fresh RV64 builds pass, and the focused `ostd` decoder and bounded `read_file` regression pass. The package-wide `ostd` host command passes with 24/24 unit tests, 5/5 `cluster-endpoint` tests, and 19 doctests passed plus 2 intentionally ignored: 48 passed, 0 failed. Enrollment, lease renewal, routing, protected provider, cross-broker binding, two-node relay/direct-LAN, remote failover, service deployment, physical recovery, and production scope remain gated |
| Kernel signature, pointer, and entropy remediation | Current-scope technical debt | `governance-gated` | `host` | Separately approved security children | Named security/PAL approvals and implementation checkpoints |
| Authenticated software evidence | Completed / regression-only | `ready` | `host` | Hardware-independent roadmap Phase 07; trusted GitHub-hosted `main` run `33251921677:1` at revision `d951d7dbf191133e94061ded7f0a8d17bfcf07c8` completed, manifest digest `2263115d4f3f58b990074d0cb7489ec5f52523f23a2a9777a8685a8c09492abb` was independently verified, the run-id/attempt sequence was consumed once through explicitly provisioned durable operator-owned external state, and exact replay was rejected | Reopen only for a pipeline/schema/workflow-identity regression or a separately governed higher evidence class; authenticated carriage preserves each bundled result's existing ceiling and implies no physical, secure-root, cloud, approval, admissibility, or production claim |
| x86 Tier 3 VirtIO parity | Completed / regression-only | `scope-gated` | `qemu` | Hardware-independent roadmap Phase 10 completed with two-boot block/network and 27-scenario hostile QEMU evidence | Reopen for a software regression or a separately governed physical-x86 qualification run |
| x86_64 interrupt entry and dispatch | Completed / regression-only | `ready` | `qemu` | Per-vector IDT plan completed with 256 deterministic entries, exact normalized errors, vector/CPL routing, 15-GPR/DF preservation, saved-CS GS/PKRU transitions, corrected syscall/fresh-exit state restoration, and an isolated `x86-idt-cpl3-test` two-task Ring-3 status-plus-marker oracle; generic `test-hooks` and production are fixture-free, the production boot/integration gates passed, and the earlier bootstrap SysV stack-phase defect is corrected | Reopen for an x86 interrupt/transition regression or a separately governed physical-x86 qualification run |
| Native CWD/path substrate | Completed / regression-only | `ready` | `qemu` | Canonical caller-attributed CWD/path resolution, `chdir`, `getcwd`, and VIFS1 FAT stat are complete | Reopen only for a regression or a separately approved broader path contract |
| AArch64 semihosting ledger closure | Completed / regression-only | `ready` | `qemu` | Issue #47 ratified by repository collaborator @datgausaigon; blocker `B-AARCH64-SEMHOSTING` resolved to PASS under schema v4 with bound QEMU runtime evidence; acceptance Phase 3 remains PLANNED | Reopen only for an AArch64 QEMU test-hooks regression or physical AArch64 qualification |
| Native POSIX follow-up | Completed / regression-only | `ready` | `qemu` | Caller-scoped shell `cd`/`pwd`, truthful bounded `fstat`, typed VFS `stat`/`unlink`/`rename`/`mkdir`/`rmdir`, and POSIX documentation repair are complete; `aaf612f1` and `85e77756` bind the live `/srv` Rename and directory-lifecycle QEMU evidence (CellosFS Native), with [Phase 05 atomic-rename verification](evidence/atomic-rename-verification.txt) | Reopen only for a regression in these bounded contracts or a separately approved broader POSIX ABI; Tier 1 remains explicitly not POSIX-complete |
| Pinned QEMU-TCG x86 compatibility | Completed / regression-only | `scope-gated` | `qemu` | Phase 06 completed the exact official QEMU 10.2.0 runner boundary, clean-prefix provenance build, and unchanged smoke, end-to-end, and hostile-oracle execution; [installer evidence](evidence/qemu-x86-10.2.0-installer.txt) and [verification evidence](evidence/qemu-x86-10.2.0-verification.txt) bind the result; x86 Tier 3 VirtIO parity remains separate regression coverage | Reopen only for a runner/provenance or oracle regression, or a separately governed physical-x86 qualification |
| x86 hypervisor boot-to-shell CI gate | Completed / regression-only | `ready` | hosted `qemu` | Separate automated-assurance [CI job](../.github/workflows/ci.yml) `qemu-x86-hypervisor-boot` is wired with checksum-pinned official QEMU 10.2.0 source, a volatile-disk hypervisor image, the `/ #` shell oracle, hard timeouts, and always-uploaded evidence. The local Ubuntu 24.04-equivalent dependency path built and installed the source in 86.90 seconds with downloads disabled and slirp 4.7.0; its exact image/kernel/ISO flow and strict 1 GiB `BOOT_WINDOW=600` smoke passed in 600.09 seconds. Hosted pull-request run [`33474206901:1`](https://github.com/dxsl-org/cellos/actions/runs/33474206901) passed the dedicated job and uploaded `x86-hypervisor-boot-1` with successful gate/smoke status and the guest prompt. | Reopen for a software regression or a separately governed KVM or physical-x86 qualification run; QEMU-TCG evidence grants no KVM, persistence, physical-x86, admission, or production qualification |
| Remote/public Cell-to-Cell operation | Future capability | `contract-gated` | `host` / `service` | ADR-0009 and [tier-aware C2C plan](../.agents/260927-1100-c2c-anywhere-tier-aware/plan.md); correlated relay server codec verified 40/40 | Authority-owned client framing and bounded broker correlation integration wait for post-entry-GO Phase 4 Build; remote routing still waits for ADR-0008 AC-012, Phase-06 two-node relay-only oracle, and separately governed identity/export gates; public export remains separately gated |
| Protected relay identity | External-gated prerequisite | `external-gated` | `host` | ADR-0008, KMS/Silo protected relay plan, and [tier-aware C2C Phase 04](../.agents/260927-1100-c2c-anywhere-tier-aware/phase-04-protected-relay.md) | Real protected persistence, authenticated time, reviewed pending-key binding, and `DEV_REFERENCE` authority Phase 8 GO over AC-001..AC-011 open service-net Build. Build must then implement ADR-0008 and pass AC-012 before the guarded C2C Phases 05–06 relay route can be enabled; public KMS opcodes stay frozen and this lane-local gate does not block unrelated development |
| STM32 DEV_REFERENCE protected authority | External-gated prerequisite | `external-gated` | `host` `SOFTWARE_HARNESS` complete; later exact-device `physical` development evidence | Authority Phase 4 private v2; typed protocol, authenticated profile bank, full-record journal/recovery model, certificate/profile validator, promotion recovery, production rejection, and deterministic non-executing provisioning plan pass 52 authority-protocol, 38 journal/bank, 17 validator, 22 provisioning, and 8 production-rejection tests plus RV64 no_std checks | Admit exact STM32H573I-DK and private SLB9672 hardware; freeze its TPM handle/NV/template/policy map; obtain operator approval of the irreversible plan hash; prove locked-device isolation, lifecycle/debug protection, endurance, and the full physical failure matrix; and demonstrate a confidential, integrity-protected, purpose-bounded STM32-to-isolated-KMS capability handoff |
| AWS DEV_REFERENCE nonce-bound signed time | External-gated prerequisite | `external-gated` | `host` `SOFTWARE_HARNESS` runtime/package/provider/allocator-lineage cores complete; later authorized live-service evidence | Authority Phase 5; ADR-0011's strict draft-11 adapter and ADR-0012 lineage contract plus cold-start/package composition pass 303 signed-time and 8 production-rejection tests. Source history confirms generated Cloudflare vectors are synthetic, current source emits/requires root `NONC`, and neither draft 11 nor draft 8 permits the public endpoint's authenticated missing-`NONC` response; draft 11 also forbids its `RADI=1`. The deployed source/configuration is unpublished, so the adapter remains strict and sealed. | Obtain a conforming endpoint or approve a new provider/profile and source epoch; then obtain reviewed packaging inputs, name and authorize the AWS DEV account/region, implement signing/upload/deploy/rollback/evidence scripts, and prove old-key disable, table/head CAS, restore/fork rejection, outage, rollback, and principal isolation. No current local artifact or endpoint observation satisfies deployment or production admission. |
| G3 accelerator | Future capability | `external-gated` | `contract` | Accelerator evidence envelope | RK3588, accepted RKNN package/license, then X390 evidence |
| Native CPU AI inference service | Current executable work | `ready` | `qemu` | Spec 24 CP-1..CP-3 plus Phases 05–07 (SentencePiece, control tokens, scaled attention, zero-copy weights, a 15M-parameter checkpoint served inside a Cell, and `POST /api/infer` over `AiClient`): `libs/ai-proto`, `libs/ai-sdk`, `libs/gguf-rs`, `libs/ai-tokenizer`, `libs/tensor-math`, `libs/ai-engine`, `cells/services/ai` (`service::AI = 15`), and `cells/services/httpd` | HTTP → inference → JSON passes the canonical hostfwd QEMU gate; the Hypha gateway is a second consumer (local-first, see its own row); throughput is measured and fixed twice over — `-O2` for the two hot crates (5.1× host, 13% in-cell) and integer Q8_0 × Q8_0 activation quantization (1.9× per kernel: 135M decode 211.5 → 23.6 ms/token on the host), both in `.agents/260914-cpu-engine-optimization/` and `.agents/260914-q8-integer-activations/`. The oracle runs on both ISA legs of the CP-3 gate (`.agents/260914-ai-oracle-arm64/`): the CI job is a riscv64 + aarch64 matrix, and the golden ids reproduce under the softfloat aarch64 ABI as well. Next: the RPi3 memory-budget leg of that gate (needs the board), SIMD kernels (AVX2/NEON/RVV), and a Q4_K quantizer, while NPU/GPU backends stay behind the G3 accelerator envelope |
| Hypha agent (local-first LLM backend) | Current executable work | `ready` | `qemu` | [Hypha plan](../.agents/260621-1433-hypha-ai-agent/plan.md) P0–P3 plus [Phase 07a](../.agents/260621-1433-hypha-ai-agent/phase-07a-local-inference-backend.md): `cells/apps/hypha/llm-gateway` answers a turn from `/bin/ai` whenever a model is resident and the prompt fits one AI IPC message, and falls back to the network endpoint only when local inference is absent (`NoService`/`NoModel`) — a failed local attempt is reported, never retried remotely. Gate: `hypha-local-ai` on the canonical image (two typed turns, reply lines, no network marker) | Next: P4 `tool-peripheral` (G1 robot showcase), P5 persistence, P7 NPU backend (G3-gated); the deterministic fixture in the canonical image means the gate covers wiring, not text quality |
| VF2 DEV_REFERENCE root stream | External-gated prerequisite | `scope-gated` | `host` `SOFTWARE_HARNESS` complete; later exact-device `physical` development evidence | ADR-0010 and authority Phase 3; deterministic no_std CBOR/COSE core, bundler, independent verifier, bounded host I/O, initialized-DRAM-contained pre-receive window/quarantine validator, logical cleanup-order harness, and XMODEM codec pass 28 tests plus real host smoke | Exact VF2 v1.3B plus STM32 authority hardware must freeze BootROM/SRAM/DRAM/address/quarantine/coherency limits and prove sole-sender/reset/strap/media/cleanup negatives; host output never satisfies that gate |
| Additional physical-board qualification | External-gated prerequisite | `external-gated` | `physical` | Hardware tracks | Exact VF2, Pioneer, RPi4, or x86 board and board-specific logs; existing RPi3 work need not wait |
| Production root selection | External-gated prerequisite | `external-gated` | `production` | ADR-0006 | Vendor package satisfying ADR-0006 and a superseding GO ADR; no stock TPM or generic secure-element counter is selected as the floor |
| Production admission and release | Production release gate | `external-gated` | `production` | Tier 1 admission and governed release owners | Remote C2C identity where applicable, protected relay identity, production KMS/root, secure/measured boot, qualified external floor, physical hostile evidence, authenticated runner, required human approvals, and release-ledger closure |

Internal child plans own execution details subject to ADR-0022 and the portfolio.
The preceding historical table is not an executable queue.

## Current Direction

Intel x86-64 C2C Anywhere is the sole direction until an explicit owner decision
changes it. Tier 1 native, Tier 2 C/C++ and Tier 3 VM consumers are all part of
the target. Console/CLI and underlying OS facilities serve that target; they are
not independent products. Local native IPC or a guest boot alone is not completion.

All former G1–G5 product programs are parked except concrete slices admitted as
Intel C2C dependencies. Keep one physical configuration, one implementation slice
and bounded acceptance scenarios; see ADR-0022 and the current-focus milestone table.

## Retained Codebase Snapshot

The following snapshot and open-gate records predate this direction change.
They preserve provenance, not current scheduling or qualification; consult source
and the current-focus boundaries before using an old status as an implementation
claim. In particular, x86 guest evidence below is SVM/TCG, not Intel VMX.

- Cargo workspace members: 111, verified with `cargo metadata --no-deps`.
- HAL shape: `hal/core`, four `hal/soc/*` crates, fifteen `hal/traits/*`
  crates, and three `hal/arch/*` crates.
- HAL to kernel Rust ABI hook signatures are single-sourced in
  `hal/traits/arch/src/kernel_abi.rs`; `scripts/check-hal-boundaries.sh`
  rejects new local `extern "Rust"` declarations under `hal/arch`.
- Board descriptors live in root `boards/`; the earlier inventory recorded seven
  integration descriptors and placeholder documentation for `q35-x86_32`,
  `virt-riscv32`, and `virt-aarch32`. Only Intel x86-64 is now a development target.
- Active native scripting runtime: Lua. MicroPython is historical roadmap text
  and is not a current Cargo workspace member.
- Application execution uses Tier 1/2/3 terminology. `Tier 1b` and `Tier 3b`
  are legacy guide aliases for Tier 1 runtime profiles and Tier 3 Linux guests;
  SDK packaging uses named modules, not numbered tiers. The ratified [Spec 23
  Native SDK contract](specs/23-native-sdk-contract.md) keeps execution tier,
  runtime profile, stability, and availability as separate axes. Manifest v2 exposes
  canonical `PROTECTION_CLASS_*` aliases while retaining the ABI-stable `tier`
  byte and legacy `TIER_*` names.
- Tier 2 RV64 native-domain substrate and scheduler transitions (Spec 22 Items 2–3)
  are implemented behind `native-domains` and `test-hooks` with verified one-hart
  (`switch`, `sas-fastpath`) and two-hart (`migration`) QEMU evidence. Production
  admission remains disabled by default, SAS remains the default view, and no
  Manifest v3 bytes, installer UI, or qualification claims are exposed.
- Tier 3 x86 has aggregate QEMU evidence: the normal pinned Alpine 3.21.7
  Linux 6.12.81 path reaches `/bin/sh`, and the dedicated `/virtio-e2e` path
  passes two fresh outer boots under QEMU-TCG 10.2.0 with Intel VT-d ACTIVE,
  persistent 16 MiB block write/FLUSH/readback, IRQ5/IRQ6, and shared network
  TX/RX under a distinct nested MAC. The hostile path passes 27 bounded,
  origin-separated scenarios, including an independently observed pause-less
  vCPU preemption, supervisor-driven VFS/Net generation termination, bounded
  unavailable outcomes, persistent block reopen/readback, acknowledged network
  TX, and matching ARP RX after restart. Post-stimulus liveness and host-read
  persistence remain required. This is QEMU-only evidence: ARM64 hostile
  execution is blocked, QEMU-TCG 8.2.2 remains incompatible, and physical x86
  remains hardware-gated.
- Separate automated-assurance [CI job](../.github/workflows/ci.yml)
  `qemu-x86-hypervisor-boot` now wires the x86 hypervisor boot-to-shell path to
  checksum-pinned official QEMU 10.2.0 source, `HV_VOLATILE_DISK=1`, the `/ #`
  shell oracle within 600 seconds, hard step/job timeouts, and evidence
  collection/upload under `if: always()`. The local Ubuntu 24.04-equivalent
  dependency path built and installed the official source in 86.90 seconds
  with downloads disabled and slirp 4.7.0, then the exact image/kernel/ISO flow
  and strict 1 GiB `BOOT_WINDOW=600` `/ #` smoke passed in 600.09 seconds. Local
  static/adversarial validation and review also passed. Hosted pull-request run
  [`33474206901:1`](https://github.com/dxsl-org/cellos/actions/runs/33474206901)
  passed the dedicated job and uploaded `x86-hypervisor-boot-1`; its gate record
  reports `job_status=success` and `smoke_outcome=success`, while the serial
  log records the volatile-disk selection, vCPU run-loop entry, and guest `~ #`
  prompt. This hosted QEMU-TCG PASS grants no KVM, persistence, physical,
  admission, or production evidence. The preceding x86 QEMU evidence remains
  unchanged, and this independent gate does not alter blocked Cell-to-Cell
  Phase 05.
- RV64 QEMU desktop now has a bounded compositor-owned window-policy slice.
  Interactive surfaces carry bounded titles and receive typed lifecycle events
  without losing normal forwarded input. The compositor paints clipped
  frame/title/control decoration outside immutable client coordinates, consumes
  decoration input, and preserves content click-to-raise, capture, keyboard
  focus, and background exclusion.
- A titlebar drag relocates the window. Edge/corner resize, maximize, and
  restore use a staged `WindowConfigure` transaction: the owner applies a
  replacement Grant and acknowledges the matching serial before geometry
  commits. Minimize removes the surface from paint/hit testing until restore;
  close requests require an explicit owner rejection or acceptance.
- The `window-policy` QEMU scenario drives real tablet input and samples real
  scanout for frames, controls, titlebars, client pixels, drag relocation,
  resize commits, minimize/restore, maximize/restore, and close
  reject/accept. Its existing background, capture, and keyboard-focus coverage
  remains; the separate compositor-cursor scenario retains cursor coverage.
  This remains compositor policy, not a desktop shell: no taskbar, snapping,
  persistence, or live resize preview is supplied.
- The compositor's damage-driven scanout path now clips each intersecting
  surface copy to the dirty region before decoration, cursor composition, and
  GPU flush. This preserves the bounded window-policy contract while avoiding
  whole-surface copies for local damage.
- ViUI now has a bounded managed-surface integration: `ManagedSurfaceApp`
  handles configure/resize, minimized/restore, close accept/reject, and
  explicit shutdown for one compositor-owned `ViSurface`. The live generated
  Counter demo uses compositor-forwarded input and precise clipped damage.
  Its dedicated RV64 QEMU oracle passes generated-label repaint, pointer input,
  maximize/restore geometry, accepted close, and post-restore Enter activation;
  the separate `window-policy` QEMU regression also passes. This is QEMU
  software evidence only, not physical-board or production qualification.

## Retained Open-Gate Records

- Production signing is not fleet-enforced by default: `signing-required` is
  non-default and the non-dev public key path is still a `[0u8; 32]` placeholder.
- Physical hardware evidence remains separate from QEMU/compile evidence. RPi3
  boot/storage/UART and BCM GPIO/I2C/SPI gates pass; VF2, Pioneer, RPi4, and
  physical x86 remain hardware-gated.
- The q35 PCIe/ECAM/NVMe/e1000/VT-d software lane now covers admitted buses
  above bus 0 and the e1000 DHCP data plane. Checked inclusive MCFG mapping and
  the matching Platform claim normalize the bus-0-relative ECAM base by
  `bus_start`; enumeration and VT-d use canonical BDFs and distinct context
  tables per bus. The x86 image now requires a fresh `/bin/net` and invalidates
  stale service/image artifacts before that build. Strict q35 runtime passes
  multibus 2/2, e1000 DHCP 2/2, NVMe 3/3, and full x86 boot 7/7. Ordinary and
  VT-d DHCP gates order NIC registration, accepted Driver-Cell Tx, e1000 Rx,
  and the acquired IP; the VT-d case requires isolation first. This closes
  only the q35 software Tx/Rx/DHCP gate. Physical x86 NIC qualification and
  ACPI DMAR discovery remain hardware-gated.
- Physical x86 has no qualified target and no PC-class driver path yet. A
  **generic** `pc/x86_64-pc` descriptor plus the HCL model landed in phase 01
  (`--features board-x86-pc`, `docs/hardware-compatibility-list.md`), but there
  is still no machine-specific descriptor and no HCL row; AHCI has no
  implementation anywhere in the tree, xHCI was
  explicitly frozen out of the G1–G3 common-driver plan by decision, the e1000
  cell fail-closes every Ethernet binding other than 82540EM
  (`kernel/src/task/drivers/pcie_ecam.rs:894`), and the x86 IOMMU base is
  hardcoded to q35 (`kernel/src/task/drivers/iommu_x86.rs:59-60`). Intel Tier 3
  (VMX) guest execution is also unimplemented, so the x86 guest path remains AMD
  SVM-only. Prerequisites X86-PC-0..7, the COM1/16550 must-have (the only
  working x86 log and input path), and the requirement that firmware allow
  disabling Secure Boot (no signed/measured x86 boot path exists, so a board
  that locks it on is not compatible) are recorded in
  [hardware-tracks.md](roadmap/hardware-tracks.md); this bullet authorizes no
  implementation.
- AArch64 test-hooks runtime evidence remains host-gated where the existing
  `qemu_exit::AArch64Semihosting` issue blocks the lane.
- RV32 release compilation is verified, but RV32 runtime cannot run on this
  host without OpenSBI firmware. This is a non-blocking compile-only evidence
  gap, not runtime qualification.
- Tier 2 native domains have implemented RV64 QEMU substrate and cross-hart
  migration evidence, but remain unqualified for production release; physical
  hardware containment, DMA quarantine, and approval gates remain open.
- The Native SDK contract is ratified and the authoritative Phase 02
  [acceptance ledger](app-tier-acceptance-ledger.json) is recorded through
  `LEDGER_RECORDED`. The ratified revision is `798e8b04`; the implemented,
  verified, and attested lifecycle commits are `92340d05`, `635600c8`, and
  `c538df84`. The ledger is schema v5: `source` witnesses bind archived contract
  revisions by digest instead of the amendable working file, and the live binding
  was re-based onto the contract as amended at `81dbb81c` (C2-MID witness and gap
  prose only; the ratified matrix digest is unchanged). Phase 03 remains
  `PLANNED`; its production-admission work remains blocked. The qualification
  result remains `NOT_COMPLETE`: compile, test/runtime, delivery, hardware,
  admission, and hostile-test witnesses remain mandatory before any applicable
  SDK cell can be promoted to `USABLE`; FFI, `rust-std`, and Tier-2 scopes
  without ratified applicability remain non-qualifying.
- **PREQUALIFICATION INFRASTRUCTURE COMPLETE / ADMISSIBLE EVIDENCE
  BLOCKED:** the backend-neutral Tier 1 admission core remains test-only, while
  the canonical 18-row prequalification catalog now maps all 33 stable
  `C3-ADM-*` `test-hooks` cases and the strict parser/validator pins their
  runtime ordering. Verification is Python 13/13, RV64 33/33 plus its aggregate
  test PASS marker, QEMU integration 1/1, production-marker exclusion PASS,
  with the host baseline unchanged at 101 passed, 0 failed, and 4 ignored. The
  rejected local capture/writer and its generated bundle were removed; local
  runs are verification only, non-admissible, and retain no Phase 04 evidence.
  Production admission remains disabled: Phase 03 and Phase 04 are both
  `IMPLEMENTED` at their bounded ledger boundaries (2026-09-15/16), and their
  production gates remain blocked pending a signed CI or secure measured runner, a qualified
  authenticated rollback-resistant floor, persistent slot/evidence recovery,
  physical hostile evidence, provisioned owner/publisher anchors, production
  loader/task/audit wiring with no-task-on-denial evidence, both required human
  approvals, and governed ledger/release closure.
- **RUST `STD` FEASIBILITY PACKAGE VERIFIED / SECURITY BACKING AND HUMAN
  APPROVAL BLOCKED:** the pinned scope reconciles 27/27 sys modules and 36
  hooks (8 Supported, 10 Unsupported, 18 Deferred), selects a conditional
  content-addressed private source overlay, and verifies a fixture-only,
  non-promotional benchmark validator. PAL-019 technical backing now binds a
  production release tuple without default features and a source-equivalent
  no-default QEMU companion proving zero without synthetic success. PAL-031
  technical backing binds caller-owned GetRandom validation plus isolated RV64
  QEMU hostile/final-write race evidence. The authoritative support map keeps
  both hooks Deferred pending every named approval. This runtime evidence does
  not grant PAL approval, real entropy, or any remaining qualification gate.
  The implementation is in-tree: the private target specs
  (`targets/*-unknown-cellos.json`), the pure-Rust PAL, the sysroot overlay and
  the QEMU witness lane all exist, and the `rust-std-lane` CI job boots the std
  cell on riscv64, aarch64 and x86_64. The acceptance ledger records Phase 06 as
  `IMPLEMENTED` (2026-09-29) at exactly that bounded boundary; what remains open
  is qualification — no live capture, published triple, promotion or readiness,
  `PAL-019`/`PAL-031` still `Deferred`, and the qualification-blocking approval
  rows still `NOT GRANTED`. Maintained
  detail is in
  [runtime-and-platform-tracks.md](roadmap/runtime-and-platform-tracks.md).
- [Spec 18c Publisher Provenance Envelope](specs/18c-publisher-provenance-envelope.md)
  is **proposed**, pending security-owner and independent-reviewer approval. It
  introduces no producer, parser, production profile, or admission path; it
  does not alter the ledger's Phase 03 `PLANNED` status or unblock production
  work. Production admission remains disabled pending qualified and approved
  external-floor and owner-record gates.
- Local Cell-to-Cell recovery is explicitly relay-first. The current broker
  fails closed on bounded K1 loading, runs authenticated LAN beacons, and owns
  bounded local ingress/worker/reply roles, but neither direct Noise sessions
  nor relay routing are constructed from its dispatch loop. The external relay
  server requires TLS 1.3 mutual authentication, certificate-bound SPKI
  identities, pre-registration revocation checks, generation-safe duplicate
  handling, and bounded sessions, frames, and I/O from a strict mounted
  manifest. The [tier-aware staged plan](../.agents/260927-1100-c2c-anywhere-tier-aware/plan.md)
  replaces the unfinished [relay-first recovery record](../.agents/260819-1409-cell-to-cell-anywhere-core/plan.md)
  as execution owner; contract ratification and protected-authority entry/AC-012
  gate remote operation. Public export and distributed leases remain deferred
  until separately approved.
- KMS TLS-signing Phase 1 is verified as a fixture-backed, non-production
  vertical slice. The append-only KMS v1 ABI binds signing authority to the
  live service-net cell generation and TID, keeps C2C X25519 and Relay P-256 as
  independent capability leaves behind one provider boundary, rejects replayed
  request IDs monotonically, and returns only low-S signatures that KMS verifies
  against the exact TLS 1.3 client `CertificateVerify` input. No generic signing
  operation or private-key export was added. See
  [ADR-0005](decisions/0005-mutual-tls-relay-identity.md).
- A 2026-08-29 security review narrowed that Phase 1 evidence: untrusted
  service-net supplies an opaque transcript hash, so the protected authority
  cannot prove the configured relay server. ADR-0008 now assigns the entire
  relay TLS endpoint to the authority and makes service-net a bounded byte
  carrier. The fixture remains unreachable from a relay client; Phase 4 Build
  and all protected prerequisites remain blocked.
- KMS/Silo Phase 2 is complete as a clean-cutover `DEV_REFERENCE`,
  AArch64-virtualized-QEMU-only provider. The public/general Silo handle,
  initialization, signing, ECDH, and raw-command surfaces are removed. Only the
  live KMS instance may reach the private, TLS 1.3 client
  `CertificateVerify`-purpose protocol; direct and unbound callers fail closed.
- Silo publishes readiness only after admission, VM load, one-time development
  initialization, guest readiness, and public-metadata validation. Its exact
  `/bin/silo` self-registration authority is test-hooks-only,
  non-manifestable, non-delegable, and limited to `service::SILO`. The locked
  guest is digest-admitted before launch (33,888/61,440 bytes; SHA-256
  `fea5cd2b9c36bb158e1e74b9e2c60209c133e0057292f0b9b4bc5f3e830838e4`);
  faults or resets permanently fail the instance closed with no retry or
  fallback.
- The exact signed 12-cell AArch64 QEMU lane passed registered readiness, KMS
  self-verification, direct/unbound denials, VFS PAGE+REG grant lifecycle, and
  `vfs-test` 96/0. This proves software custody/containment only. Because the
  same Cellos EL2 host constructs and loads the guest and supplies the
  disposable development seed, Stage-2 is not an independent hardware root or
  production qualification.
- KMS/Silo Phase 3 is complete as a constrained, non-production certificate
  provisioning slice. The supervisor-only lifecycle admits one pending
  generation, publishes a canonical bounded CSR through an ordered one-shot
  handle, requires live service-net profile staging, and activates only the
  exact staged generation/policy/profile tuple.
- The fresh nonce-bound pending P-256 key remains inside Silo. Abort, invalid
  CSR access, proof failure, and commit failure require confirmed cleanup or
  leave a fail-closed tombstone; persistence or activation disagreement seals
  serving rather than exposing mixed state. Service-net validates only
  allowlisted mounted profiles and bounded client-only certificate chains.
- Phase 3 deliberately does not claim an end-to-end default-runtime enrollment:
  runtime KMS remains sealed until protected persistence and authenticated time
  exist, and frozen opcode 14 exposes only the active key, not the pending key
  needed for authenticated precommit certificate binding.
- **PRODUCTION RELAY SIGNING BLOCKED:** fixture and development Silo providers
  are non-production. Cargo rejects unsafe provider/downgrade combinations,
  production artifacts exclude the development Silo, and
  `hardware-relay-provider` remains compile-blocked because
  [ADR-0006](decisions/0006-block-production-root-pending-exact-product-evidence.md)
  selected no product. Production is `BLOCKED_BY_ADR_0006`; no Phase 1–3
  result is hardware-backed signing or a production relay artifact.
- Phase 6 closed NO-GO. No exact product, procurement path, OTP/provisioning
  plan, or board/AP integration is approved. Reopening requires one
  vendor-signed package binding all eight ADR-0006 criteria to the same proposed
  deployment; receipt permits review, not approval. Every item must pass without
  inference and a superseding GO ADR must select the exact product before
  production implementation resumes.
- The KMS/Silo protected relay identity plan remains blocked on hardware assets
  and a named AWS DEV account/region. Its authorized software track has
  completed the Phase 2 `SOFTWARE_HARNESS`: a closed `no_std`, no-allocation
  authority protocol and protected-state model, literal private/public wire
  fixtures, and production-marker rejection. These host results satisfy no
  physical or live-cloud acceptance criterion and do not unblock Phases 3–5
  while Phase 1 admission remains blocked.
- Phase 1 admission validates the locked hardware inventory and AWS
  identity/account/region evidence but stays fail-closed: its currently
  authorized AWS commands cannot prove read-only permissions. Revising that
  evidence contract requires operator approval. Phases 3–5 physical work,
  Phases 6–8 integration/evidence, and production use remain blocked; only a
  superseding GO ADR may authorize the exact production product and trust
  chain.

## Update Rule

Keep this file short. Put maintained detail in the matching
`docs/roadmap/*.md` topic file. Do not edit the legacy snapshot; historical
delivery evidence belongs in `project-changelog.md`.
