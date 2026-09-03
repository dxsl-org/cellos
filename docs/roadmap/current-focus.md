# Current Focus

**Last updated**: 2026-09-03

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
  multi-user assertions remains blocked on its external prerequisites: host root
  for rootfs creation and qualified QEMU-TCG 10.2.0.
- The AArch64 test-hooks semihosting ledger closure is complete. Blocker
  `B-AARCH64-SEMHOSTING` was corrected to subject `qemu-arm64` and resolved to
  PASS under schema v4 following independent ratification on Issue
  [#47](https://github.com/dxsl-org/cellos/issues/47) by repository collaborator
  @datgausaigon (`DECISION: YES`). The resolution binds fresh QEMU runtime
  artifacts (`docs/evidence/aarch64-semihosting-20260903-03-raw.txt` and
  `docs/evidence/aarch64-semihosting-20260903-03-runner.txt`). Acceptance-ledger
  production Phase 3 remains PLANNED. The caller-scoped shell `cd`/`pwd` and
  bounded truthful `fstat` lanes are complete; POSIX documentation, atomic
  `rename`, and pinned-QEMU x86 compatibility retain their independent gates.
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
  in supported paths, including pinned-QEMU compatibility and AArch64
  semihosting ledger closure tracked by the
  [open risk register](open-risk-register.md). This label does not apply to
  all advanced work.
- **Future capability:** remote/public Cell-to-Cell operation, additional
  desktop and x86 platform depth, G3 accelerators, G4 `rust-std`, and G5
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
