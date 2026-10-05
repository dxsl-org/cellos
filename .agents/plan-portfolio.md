# Cellos plan portfolio

**Status:** Canonical scheduling index
**Updated:** 2026-09-27 (kernel-repair/C2C concurrency boundary recorded; C2C implementation remains queued)

This file owns scheduling intent. Source/tests own implementation truth; individual plan
files preserve detailed scope and provenance. Untouched checkboxes are not proof that code
is absent. A plan directory not listed as active or queued is historical until explicitly
promoted through this index.

## Active

- `260916-1200-tier1-rust-std-pal-implementation` — [completed 2026-09-16] Tier 1 Rust `std` PAL in-tree implementation (custom target specs, sysroot overlay, PAL primitives, workload parity PASS).
- `260913-2002-g2-level-a-ai-inference` — Spec 24 CPU inference path (CP-1..CP-3); phases
  01-04 complete at the host/QEMU ceilings, NPU/GPU/Tier 2 checkpoints remain gated.
- `260727-2101-midori-lessons-cellos` — complete convergence program (D39).
- `260925-2214-beam-parity-b0-actor-supervisor` — B0 of the BEAM/OTP backend roadmap
  (`docs/roadmap/beam-parity-backend-roadmap.md` §5): a userspace actor + supervisor library
  (`ostd::actor`, `ostd::actor::supervisor`) so an application declares its own supervision tree
  instead of editing `/bin/init`. No `libs/api` change; design in
  [ADR-0021](../docs/decisions/0021-actor-supervisor-library-in-userspace.md). Witnessed at the
  `qemu` ceiling (`scripts/qemu-actor-supervisor.sh`, evidence under `docs/evidence/`). B1 (in-cell
  concurrency and cancellation) and B2 (per-request cell cost/scale, which keeps D5 WIP-limited)
  stay queued behind their own triggers; this promotion opens no ABI and touches no
  capability-scheduling boundary.
- `261004-1957-x86-pc-lane` — x86_64 PC lane, controller-family bring-up with
  QEMU-first gates ([plan](261004-1957-x86-pc-lane/plan.md)).
  **Authorized scope: phases 01–06** — 01 `x86_64-pc` descriptor + HCL model
  (**completed 2026-10-04**), 02a/02b AHCI/SATA (**completed 2026-10-05**,
  two-boot persistence), 03 xHCI + HID (**completed 2026-10-05**, enumeration +
  in-cell decode) and 03b USB HID producer role (**completed 2026-10-05**;
  append-only syscall 423 + service id 17 sharing allowlist bit 50, approved by
  the owner), 04a igb NIC part A (**completed 2026-10-05**; append-only opcode 424
  `FindPcieDeviceByVendor` + 48-byte `PcieDeviceInfo`, approved by the owner; SKU
  claim narrowed to `10C9`+`1533`), 04b igb DHCP data plane + VT-d variant
  (**completed 2026-10-05**; no product change — the net service already owned
  DHCP), 05 ACPI DMAR → real IOMMU (**completed 2026-10-05**; DRHD base parsed
  from DMAR, `DmaIsolation` profile contract makes absent-DMAR fail-closed), 06
  multi-port COM/RS232-485 (**completed 2026-10-05**; ABI 425–428 + `serial_port`
  capability + policy blob v4, owner-approved; RS485 declared, not claimed).
  **Phase 07 (physical lane + first HCL rows)
  is not authorized**: it starts only after 02b/03/04b/05 are green on QEMU and
  hardware is bought. Validate completed 2026-10-04 (36/36 claims verified,
  0 failed; interview: igb first, 02/04 split into sub-phases, sequential
  execution, hardware after QEMU). WIP limit: **one family at a time**.
  Boundaries: Intel VMX stays in `260711-1917-tier3b-x86-vtx` (P09); x86 AVX2
  kernels stay with `260914-cpu-engine-optimization`; Secure Boot belongs to the
  Security track. This promotion opens no ABI, claims no physical machine, and
  changes no evidence ceiling — QEMU results remain regression evidence only.
Allowed side work is limited to P0 security fixes, broken-build/CI repairs, and
verification-only closure that opens no new feature program.

## Queued / blocked

- `260712-0800-supervisory-cell-migration` — P-TRUST dependency satisfied; WIP-limited.
- `260712-1000-cell-package-distribution` — blocked on a capability-scoped installer
  redesign; ambient or name-authorized `/bin` writes are forbidden.
- **Trust & Identity program** (one portfolio group, separate child plans):
  - `260712-1900-manifest-v2` — P00-P02 complete; P03 deferred.
  - `260712-1902-dice-attestation-identity` — P00 complete; P01-P05 queued.
- `260927-1100-c2c-anywhere-tier-aware` — [queued implementation plan](260927-1100-c2c-anywhere-tier-aware/plan.md); Phase 01 contract ratification and Law-1 approvals before ABI work, with relay Phases 04–06 blocked on independent protected authority evidence. Native local/remote, async, Tier-1 fastpath and Tier-3 guest have distinct capability gates. WIP promotion requires this portfolio rule; no remote route is enabled by queuing the plan.

  Scheduling agreement (2026-09-27): C2C Phase 01 may proceed in parallel **only as contract inventory, drafting, review and non-activating evidence preparation**. Inventory and [Spec 20 Draft v3](../docs/specs/20-unified-ipc-contract.md) are prepared; Phase 01 waits for kernel-owner review of local binding and contract/ABI decisions. Its replay design also requires a protected nonrollback source incarnation or a separately ratified alternative: current beacon uptime is not such a source. This does not promote C2C implementation, ratify Spec 20, approve an ABI or enable remote operation. Kernel architecture repair retains sole ownership of overlapping kernel/syscall/domain/grant/scheduler files until its applicable gates pass and ownership is handed off. Promote C2C implementation through the rule below in the same portfolio change; serialize overlapping edits and verification.
- `260605-1406-phase28-wasm-cells-epmp` — partial/suspect: WASM crates are present but
  retain-vs-remove and runtime qualification are unresolved; ePMP is M-mode-blocked.
- Per-request server scale (D5) — accepted goal, WIP-limited behind Midori. Promotion requires
  N=64/128/256/512 memory/spawn/isolation baselines **measured with M heavy cells resident**
  (mixed occupancy, not a homogeneous light sweep) before image sharing, demand stacks, profile
  quotas, or dynamic cell tables are implemented. Firmware memory discovery landed
  (`kernel/src/boot/dtb_memory.rs`), so the hardcoded 190 MiB map that capped the 2026-07-31
  measurement (n=8–9) no longer binds; those baselines have not been re-run. A variable VA budget
  (fixed 32 MiB stride today) is an additional named prerequisite — Spec 19 §3 amendment,
  2026-09-22.

## Explicitly deferred

- ViUI GPU acceleration — reopen as its own hardware/benchmark-gated plan.
- Manifest v2 `cap_args` — concrete parameterized-capability consumer required.
- DICE Veraison/COSE adapter — external verifier/consumer required.
- Hardware-gated product programs remain deferred until their plan-specific trigger is met.

## Completed / closed records

- `260712-1901-cap-revocation` (P00-P05) — closed 2026-09-25 at the `qemu` ceiling.
  `sys_cap_revoke` no longer label-changes: MMIO windows lose user accessibility
  (`unmap_mmio_user_x86` / `clear_mmio_user`), owned grants are reclaimed with in-flight
  pins quarantined rather than freed, `iommu::unmap_dma` is real (leaf cleared + IOTLB /
  IOFENCE acknowledged) and the whole-domain teardown is shared with cell death, and
  `pcie_driver`/`platform`/`supervisor` became revocable via three additive `cap_mask`
  bits (Law-1 confirmed twice) with their DMA/BDF/BAR/ECAM teardown. The victim is told
  with `AppEvent::CapRevoked` on the newly registered `0xF2` envelope. Witnessed by one
  RV64 QEMU run (`qemu-native-domain-test --harts 1`, kernel `e1ec27a0…`): IOMMU-TEARDOWN,
  GRANT-RECLAIM and MMIO-REVOKE markers plus the `thread-cap` revoke aggregate. Raw log:
  `docs/evidence/cap-revoke-qemu.{log,txt}`. Residual recorded, not hidden: no Cell issues
  `CapRevoke` yet (the end-to-end path is witnessed in-kernel), the DMA-fault oracle needs
  real IOMMU hardware, and the x86/aarch64 MMIO legs are compile-verified only.
- `260922-1549-cell-native-portability-program` (7 phases, ADR-0018/ADR-0019) — closed
  2026-09-25 at the `qemu` ceiling. Tier 2 admission on the path, `cpp-freestanding`, per-task TLS,
  futex ABI + wait queues, the kernel pipe object, the generated porting kit, and the three
  reference-port classes are all witnessed by RV64 QEMU runners
  (`qemu-native-domain-test`, `qemu-cpp-smoke`, `qemu-tls-test`, `qemu-futex-test`,
  `qemu-pipe-test`, `qemu-c-pthread`, `qemu-c-spawn`); the follow-on C thread/process
  proposal closed with it. Raw logs: `docs/evidence/`; closure record:
  `.agents/260922-1549-cell-native-portability-program/phase-07-reference-ports-and-cost.md`.
  Residual recorded, not hidden: a third-party port needing a `fork`/`exec` process tree stays
  class D, and class C is witnessed by an in-tree workload rather than vendored third-party code.
- `260616-0755-viui-completion` — canonical ViUI v2 implementation record.
- `260712-1100-loader-trust-repair` — P-TRUST landed in `721e1f6f`.
- `260712-1900-manifest-v2` implementation P00-P02 — landed in `c25f3185`.
- `260712-1903-thread-cellid-quota-fix` — kernel-side closure recorded.
- `260801-d12-hardware-supplement-ruling`, `260801-d13-tier1-signature-admission-ruling`,
  `260801-d15-d17-rulings`, `260801-d18-d25-rulings`, and
  `260801-d26-d33-rulings` — decision/documentation records complete.

## Superseded / retired

- `260624-cell-to-cell-anywhere` — superseded 2026-09-27; P00–P03 historical partial foundation only, integration not delivered.
- `260819-1409-cell-to-cell-anywhere-core` — superseded 2026-09-27; completed local-only evidence retained, remaining phases replaced by `260927-1100-c2c-anywhere-tier-aware`.

- `260608-1451-viui-next-phases`, `260609-0601-viui-g2`, and
  `260608-1227-viui-embedded-robot-readiness` — superseded by the closed ViUI record and
  current Spec 14.

## Promotion rule

A queued/deferred plan becomes active only when its dependency/trigger is evidenced, file
ownership does not collide with the active program, Law-1 confirmations are satisfied,
and this index is updated in the same change. Do not advertise aggregate COMPLETE/OPEN
counts until a generated inventory can reconcile code evidence with plan metadata.
