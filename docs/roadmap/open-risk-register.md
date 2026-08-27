# Open Risk Register

**Last updated**: 2026-08-27

This register tracks confirmed production-readiness gaps found while syncing
docs to code. It is not a bug-fix plan.

## Capability Scheduling Boundary

This register identifies gaps; it does not serialize unrelated work. Each risk
retains its security, hardware, ABI, and human-approval gates while its owning
lane advances only to the evidence ceiling it can prove. The authoritative
execution class, owner, and reopening event are in
[the roadmap capability table](../project-roadmap.md#capability-lanes).


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

- TLS raw fallback still trims at the last non-zero byte in
  `cells/services/net/src/handlers.rs:453-459` before the send path at
  `:608-613`, which can truncate valid binary payloads ending in zero bytes.
  The raw path needs an explicit length contract.
- Production cell admission is not signed-only by default. The 18-row catalog,
  33 stable `test-hooks` cases, and strict runtime parser are prequalification
  infrastructure only; local runs are explicitly non-admissible and the former
  local capture/writer was removed rather than accepted as evidence.
  `kernel/src/signing.rs` still uses the reproducible dev public key under
  `dev-signing-key`; without that feature the
  key is a zero placeholder, while `signing-required` remains opt-in. Phase 04
  stays blocked pending authenticated runner evidence, a qualified external
  floor and persistent recovery, production gate/task/audit integration,
  physical hostile evidence, provisioned anchors, both human approvals, and
  ledger/release closure.
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
 

## Medium

- **`CELLOS-HV-X86-TCG-001` — Medium, owner: Tier 3 x86 VM lane.** The
  qualified QEMU-TCG 10.2.0 runtime boots the pinned Alpine 3.21.7 guest at
  both 1 GiB and 2 GiB, reaches Linux 6.12.81, runs `/bin/sh`, and exposes the
  BusyBox `~ #` prompt. Ubuntu 24.04's QEMU-TCG 8.2.2 remains incompatible:
  the same ISO, VMCB/NPT code, CPU model, and memory sizes fetch zeros at the
  entry GPA and triple-fault (`rip=0x1127370 cr0=0x11`). The smoke runner
  accepts `QEMU_X86_BIN` so CI and WSL can select the qualified emulator and
  emits a specific diagnostic for the 8.2.2 signature. Hardware nested-SVM
  evidence and the precise upstream TCG regression range remain open.

- Net-broker is still partial wiring. `cells/services/net-broker/src/main.rs`
  marks K1 PSK loading, LAN beacon sockets, relay dispatch, lease renewal, and
  enrollment handling as TODOs; docs should not claim completed swarm routing.
- Net service polling still uses a 100 ms cadence in `cells/services/net/src/main.rs`,
  which is acceptable as a stopgap but remains latency debt for more interactive
  workloads.
- The HTTPS smoke path soft-skips when its TLS mock is unavailable
  (`tests/integration/tests/http-smoke.rs:170-175,211-219`), so CI can degrade
  to HTTP-only evidence unless that prerequisite becomes a hard CI gate.
- Native POSIX path handling remains incomplete: canonicalization, `chdir`,
  `rename`, `getcwd`, and `fstat` contain stubs or deferred behavior in
  `kernel/src/task.rs`; Tier 1 must not be documented as POSIX-complete.
- The hypervisor CI lane is documented as intermittent in
  `.github/workflows/ci.yml:512`. Hosted-runner dependency downloads have also
  exhausted the former 10-minute budget before tests; the affected apt steps
  now allow 20 minutes, but mirror failures remain infrastructure blockers, not
  runtime passes or functional failures.
- Several physical hardware lanes remain hardware-gated. Compile/QEMU evidence
  is useful regression evidence but must not be used as VF2/Pioneer/RPi4 physical
  qualification.
- AArch64 test-hooks runtime proof remains blocked where the host-side
  `qemu_exit::AArch64Semihosting` issue is still present.

## Low

- POSIX and Lua libc stubs are intentionally fail-closed or unsupported, but
  docs must avoid implying full POSIX compatibility for Tier 1 native cells.
