# Open Risk Register

**Last updated**: 2026-08-24

This register tracks confirmed production-readiness gaps found while syncing
docs to code. It is not a bug-fix plan.

## Critical

- Net socket caps are still keyed only by predictable `CapId` values in
  `cells/services/net/src/socket_table.rs:20,51`, and the handlers at
  `cells/services/net/src/handlers.rs:157,177,202` do not owner-bind resolution
  to the caller. Another network-capable Cell can therefore guess or reuse a
  live cap and operate on a peer's socket.
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
- **`CELLOS-RUSTSTD-PTR-004` — Critical, owner: later authorized
  PAL/target/runtime implementation child with the kernel syscall-security and
  Rust `std`/PAL owners.** `GetRandom` currently constructs a mutable slice
  from the Cell-supplied output pointer without first applying the available
  user-buffer validator or otherwise proving bounded, complete, caller-owned
  writable provenance (`kernel/src/task/syscall.rs`). A caller granted the
  syscall can therefore present null, overflowed, oversized, unmapped, kernel,
  or peer-cell ranges to an unsafe write boundary. `PAL-031` remains Deferred.
  Qualification requires rejection of every hostile class before access,
  evidenced by direct-syscall tests, while preserving allowlist enforcement and
  the frozen ABI. This later child is not authorized until all six human
  approvals, its implementation checkpoint, and umbrella Phase 03 production
  gates are granted.

## High

- TLS raw fallback still trims at the last non-zero byte in
  `cells/services/net/src/handlers.rs:453-459` before the send path at
  `:608-613`, which can truncate valid binary payloads ending in zero bytes.
  The raw path needs an explicit length contract.
- Lua and WASM host cells do not declare the `StateRestore` / `LookupService`
  syscall pair required by their argv/VFS paths
  (`cells/runtimes/lua/src/main.rs:11`, `cells/tools/wasm/src/main.rs:16-20`).
  Those paths can fail under the enforced syscall allowlist.
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
- **`CELLOS-RUSTSTD-ENTROPY-005` — High, owner: later authorized
  PAL/target/runtime implementation child with the kernel entropy and Rust
  `std`/PAL owners.** The default development kernel feature tuple enables
  `dev-weak-rng`; the current VirtIO RNG source returns zero bytes, after which
  `GetRandom` emits predictable xorshift bytes and reports success
  (`kernel/Cargo.toml`, `kernel/src/task/drivers/virtio_rng.rs`,
  `kernel/src/task/syscall.rs`). This may support disposable development/QEMU
  identities with an explicit warning, but it is not production entropy,
  cryptographic evidence, PAL support, or qualification. `PAL-019` remains
  Deferred until a production tuple omits `dev-weak-rng` and proves real
  admitted entropy or observable zero/error without synthetic or partial
  success. Drift in any of the exact six kernel security-backing paths
  invalidates the feasibility approval input. The later child remains
  unauthorized behind the six human approvals, implementation checkpoint, and
  umbrella Phase 03 production gates.
 
- **`CELLOS-HV-X86-TCG-001` — High, owner: Tier 3 x86 VM lane.** The SVM
  personality cannot boot the pinned Alpine guest on QEMU-TCG 8.2.2
  (Ubuntu 24.04/WSL2): the cell writes and reads back correct vmlinux bytes at
  the entry GPA, yet the vCPU fetches zeros there and triple-faults
  (`rip=0x1127370 cr0=0x11`, deterministic across `-cpu qemu64,+svm` and
  `-cpu max`, Alpine 3.21.3 and 3.21.7). The recorded known-good x86 boot
  (commit `1827b8f3`, Linux 6.12.81, `-m 2G`) ran on a different (Windows)
  QEMU build, so the mismatch is between Cellos' VMCB/NPT setup and this
  host's TCG `vmrun`, not the guest artifacts. Resolution requires either a
  NPT/VMCB investigation against the failing TCG build or runtime evidence
  from KVM/nested-SVM hardware; until then no x86 guest-boot PASS may be
  recorded from this host.

## Medium

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
