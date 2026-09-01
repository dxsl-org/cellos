# Open Risk Register

**Last updated**: 2026-08-31

This register tracks confirmed readiness gaps found while syncing docs to code.
It is not a global bug-fix queue, and it does not turn all future or
production-only capability into technical debt.

## Capability scheduling boundary

Risks do not serialize unrelated work. Under
[ADR-0007](../decisions/0007-development-first-hardware-constrained-execution.md),
QEMU, the two owner-reported Raspberry Pi 3 Model B+ boards, incoming sensors,
and local-runtime work remain executable to their lane-specific evidence
ceilings without additional procurement. QEMU evidence is software-only.
RPi3/sensor evidence is development and exact-device hardware-integration
evidence only; RPi3 cannot qualify production security or the independent
external floor.

Primary planning classification for the open entries:

| Planning class | Entries |
|---|---|
| Current executable work | Narrow fixes, bounded fixtures, and software evidence that an owning lane can perform now |
| Current-scope technical debt | Bounded net idle IPC dispatch latency, the pinned-QEMU compatibility gap, and AArch64 semihosting ledger closure |
| Future capability | Remote/public net-broker completion and native POSIX completeness beyond currently supported contracts |
| External-gated prerequisite | Unavailable exact board qualification and exact product/vendor evidence for protected relay or production-root milestones |
| Production release gate | Fleet signing/provenance, production admission, protected identity/root, secure/measured boot, qualified floor and persistent recovery, physical hostile evidence, authenticated evidence runner, human approvals, and governed ledger/release closure |

An entry's primary class does not erase its nested dependencies. In particular,
production release gates remain mandatory, disabled, and fail-closed, but block
only the production-admission or production-release milestone that owns them.
Mitigation and prequalification work may continue without promoting local,
QEMU, or RPi3 results. The authoritative execution class, owner, and reopening
event are in
[the roadmap capability table](../project-roadmap.md#capability-lanes).

The former single-guest local Cell-to-Cell oracle CI-coverage gap is closed by
the required [workflow job](../../.github/workflows/ci.yml)
`c2c-broker-oracle-single-guest-local-runtime`. Its result remains QEMU evidence
only and does not close two-node, relay, remote/public, or production risks.

The software-evidence origin/integrity and replay-control gap is closed at the
`host` ceiling by GitHub-hosted run `33251921677:1` and durable external
sequence consumption. This does not close the distinct production-admission
runner, physical-hostile, secure-root, human-approval, or release-ledger risks;
those remain fail-closed production gates.


## Critical

- **`CELLOS-LOADER-SIG-001` — Critical, owner: Phase 03
  provenance/signature boundary.** ADR-0004 binds every final ELF byte except
  the 64-byte `__ViCell_sig` payload, so post-sign mutation of section metadata
  or `.rela.dyn` invalidates the signature. Relocation writes are confined to
  pages owned by the new Cell: `apply_relocations` receives the `LoadedPage`
  set and rejects any word-sized write not wholly inside one owned page
  (`kernel/src/loader/reloc.rs`, `kernel/src/loader/reloc_target.rs`). The
  finding remains open because fleet key provisioning, the production
  provenance/signature gate, and `signing-required` enforcement are still
  unfinished; development builds still admit unsigned cells by default.
- **`CELLOS-RUSTSTD-PTR-004` — Critical historical GetRandom output-provenance
  defect; technical mitigation complete, owner: PAL/target/runtime governance.**
  GetRandom now validates the original descriptor, caps the write to its frozen
  ABI bound, proves complete caller-owned writable provenance, and retains final
  authorization through the bounded write (`kernel/src/task/syscall.rs`).
  Isolated RV64 QEMU evidence covers direct hostile descriptors and races
  against root retirement, grant revocation, and exact backing-frame reuse.
  `PAL-031` remains `Deferred` in the authoritative support map pending all
  named approvals of the now-rebound governed security manifest.
  This local QEMU evidence grants no PAL support, real production entropy,
  implementation-checkpoint, or umbrella Phase 03 production transition.

## High

- **Production release gate — production cell admission is not signed-only by
  default.** The 18-row catalog,
  33 stable `test-hooks` cases, and strict runtime parser are prequalification
  infrastructure only; local runs are explicitly non-admissible and the former
  local capture/writer was removed rather than accepted as evidence.
  `kernel/src/signing.rs` still uses the reproducible dev public key under
  `dev-signing-key`; without that feature the
  key is a zero placeholder, while `signing-required` remains opt-in. Phase 04
  stays blocked pending authenticated runner evidence, a qualified external
  floor and persistent recovery, production gate/task/audit integration,
  physical hostile evidence, provisioned anchors, both human approvals, and
  ledger/release closure. These are production-admission requirements, not
  global blockers for QEMU, RPi3, sensor, or local-runtime development.
- **`CELLOS-RUSTSTD-ENTROPY-005` — High technical mitigation complete; approval
  pending, owner: kernel entropy and Rust `std`/PAL owners.** The default
  development tuple still enables `dev-weak-rng` and remains non-qualifying.
  The governed production release tuple builds with
  `--no-default-features --features production-relay-image`; a source-equivalent
  no-default QEMU companion proves unavailable entropy returns zero without
  synthetic or partial success. `PAL-019` remains Deferred pending named
  approval of the governed manifest. Drift in any exact kernel backing path,
  release tuple, or zero/error behavior invalidates this evidence. The later
  child remains unauthorized behind the six human approvals, implementation
  checkpoint, and umbrella Phase 03 production gates.
 
- Protected relay mTLS remains unimplemented and fail-closed. The service-net
  Build and
  [Cell-to-Cell Anywhere Phase 05](../../.agents/260819-1409-cell-to-cell-anywhere-core/phase-05-relay-first-remote-correctness-oracle.md)
  relay path may reopen only after real protected persistence and authenticated
  time exist, a distinct reviewed pending-key binding is approved under the
  frozen KMS ABI, and the `DEV_REFERENCE`
  [authority plan](../../.agents/260826-1605-phase4-dev-reference-authority/plan.md)
  Phase 8 returns GO. This is a lane-local governance gate, not a blocker for
  the CI-gated single-guest local runtime or unrelated QEMU, RPi3, sensor, and
  local-runtime development. The `boot-suite` now runs the independently
  supported plain-HTTP smoke and requires `HTTP PASS`. Its guest still attempts
  HTTPS, but the harness deliberately makes no claim from the generic connect
  failure: default `service-net` has no authenticated certificate time, so the
  request cannot reach certificate verification, while the same output could
  also represent an unrelated transport failure. Direct clock tests prove
  authenticated time remains unavailable. Dedicated handler and socket
  preflight tests now pin the zero-capability and `InvalidCertificate` mappings;
  their production call sites invoke those preflights before `make_tcp` and
  record-buffer allocation. Positive HTTPS requires admitted authenticated time
  or a separately approved, explicitly test-only clock/provider harness that
  cannot weaken the default image.

## Medium

- **`CELLOS-HV-X86-TCG-001` — Medium, owner: Tier 3 x86 VM lane.** The
  qualified QEMU-TCG 10.2.0 runtime boots the pinned Alpine 3.21.7 guest at
  both 1 GiB and 2 GiB, reaches Linux 6.12.81, runs `/bin/sh`, and exposes the
  BusyBox `~ #` prompt. Ubuntu 24.04's QEMU-TCG 8.2.2 and upstream QEMU commits
  through `4a75c8c7d6` remain incompatible: the same ISO, VMCB/NPT code, CPU
  model, and memory sizes fetch zeros at the nested entry GPA and triple-fault
  (`rip=0x1127370 cr0=0x11`). The smoke runner accepts `QEMU_X86_BIN` so CI and
  WSL can select the qualified emulator and emits a specific diagnostic for the
  8.2.2 signature. Regression is isolated upstream to `b56617bbcb`
  (`target/i386: Walk NPT in guest real mode`) which restores real-mode NPT
  walking behavior in nested paging paths. Hardware nested-SVM evidence remains
  open.

- Net-broker is still partial wiring. `cells/services/net-broker/src/main.rs`
  marks K1 PSK loading, LAN beacon sockets, relay dispatch, lease renewal, and
  enrollment handling as TODOs; docs should not claim completed swarm routing.
- The former nearly 100 ms service-net idle IPC blind spot is narrowed at the
  software/QEMU ceiling. `WaitCompletion` still does not wake for IPC, so the
  service-local policy now caps its NET_RX wait at one scheduler tick plus
  normal dispatch while preserving interrupt-driven NET_RX wakeup. The
  independent smoltcp maintenance interval remains 100 ms; this is not a hard
  wall-clock or physical-hardware latency bound, and no unrelated completion
  ABI limitation is closed. The C2C QEMU oracle passed 1/1 with
  `[net-rx-producer] irq->completion PASS`, 1,000/1,000 calibration calls,
  10,000/10,000 soak calls, positive network progress, zero
  heartbeat/watchdog deltas, and no heartbeat/watchdog termination markers.
- Native POSIX path handling remains incomplete: canonicalization, `chdir`,
  `rename`, `getcwd`, and `fstat` contain stubs or deferred behavior in
  `kernel/src/task.rs`; Tier 1 must not be documented as POSIX-complete.
- The historical ARM64 hypervisor machinery intermittency was fixed at the
  EL2 IRQ/preemption boundary. Hosted run `33486590595:1` passes the retained
  TCG machinery oracle; full logs remain diagnostic evidence for distinguishing
  the narrowly tolerated nested-walk signature from functional failures.
  Hosted-runner mirror failures remain infrastructure blockers, not runtime
  passes or functional failures.
- Several additional physical hardware lanes remain external-gated. Compile/
  QEMU evidence is useful regression evidence but must not be used as VF2,
  Pioneer, RPi4, or physical x86 qualification. The two owner-reported
  Raspberry Pi 3 Model B+ boards remain available for development integration
  only and cannot qualify a production-security floor.

- AArch64 test-hooks semihosting code and a dedicated runner exist, but
  `B-AARCH64-SEMHOSTING` remains `BLOCKED` in the authoritative acceptance
  ledger with the stale `qemu-rv64` subject and no schema-valid, fresh,
  independently owned raw resolution evidence. A local runner or completed plan
  checkbox cannot close that governed ledger record.

## Low

- POSIX and Lua libc stubs are intentionally fail-closed or unsupported, but
  docs must avoid implying full POSIX compatibility for Tier 1 native cells.
