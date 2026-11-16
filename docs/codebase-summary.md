# Cellos Codebase Summary

**Project**: Cellos (Jarvis Hybrid OS)
**Version**: 0.2.1-dev (Mycelium Era)
**Language**: Rust (nightly, `no_std`)
**Crates**: 214 workspace members (`cargo metadata --no-deps`): the 211 entries of the root
`Cargo.toml` `members` list plus three path dependencies (`hal/traits/paging`,
`third_party/sunset`, `third_party/sunset/sshwire-derive`)
**Last Updated**: 2026-11-16 (layout after the architecture remediation plan,
`.agents/261110-1703-architecture-remediation/`)

---

## Quick Stats

Counts are entries of the root `Cargo.toml` `members` list. Kernel size is tracked in the
[generated metrics](code-metrics.generated.md), and per-architecture status in the
[generated per-architecture table](generated/arch-status.md).

| Area | Members | Key highlights |
|------|--------:|---------------|
| Kernel | 1 | `kernel/`; every file listed with its Spec 15 category in `kernel/residents.toml` |
| HAL | 20 | `hal/core`, 4 `hal/soc/*`, 12 listed `hal/traits/*` (+ `paging` as a path dependency), 3 `hal/arch/*`; HAL↔kernel Rust ABI signatures are single-sourced in `hal/traits/arch/src/kernel_abi.rs` |
| Boards | 1 | `cellos-boards` no_std descriptor crate (`boards/`) |
| Libraries | 40 | `libs/abi`, `libs/types`, `libs/wire`, `libs/ostd`, `libs/nic-driver`, `libs/posix-shim`, plus AI, HTTP, text, ViUI, SSH, terminal, storage and authority crates |
| Tools (cells) | 6 | `init`, `cellbox`, `kms-ssh-admin`, `sys-tools`, `net-tools`, `wasm` |
| Apps | 15 | desktop, files, terminal, ocel, chrome, Hypha cells and other user-facing apps |
| Demos | 23 | hello, ViUI, hotswap, HTTP(S), DOOM/Tetris, audio, robot and peripheral demos |
| Drivers | 38 | block (AHCI, NVMe, virtio-blk, SCSI), nine NIC cells over `libs/nic-driver`, xHCI, GPU, serial, GPIO/I2C/SPI and board variants |
| Services | 20 | vfs, net, net-broker, compositor, input, config, power, platform, supervisor, activator, kms, machine-pin, sshd, silo, hypervisor, httpd, ai and the ocel script services |
| Tests | 45 | disposable guest-side test and benchmark cells |
| Runtimes | 2 | Lua 5.4; MicroPython is historical and not a workspace member |

Excluded from the workspace (root `Cargo.toml` `exclude`): host tools under `tools/`, vendored
`third_party/redoxfs` and `third_party/mlibc`, `libs/cell-build` (build-script helper),
`cells/apps/vove` (private submodule), `cells/demos/std-smoke`, and the three Spec 26 design
skeletons `cells/services/{budget-authority,workload-router,placement-controller}` (owner
decision D1, 2026-11-10).

### Removed crates and layouts

- `libs/api` is gone (remediation Phase 13). Its frozen kernel ABI is `libs/abi`, its service
  contracts are `libs/wire`, and its libc surface is `libs/posix-shim`. The kernel depends on
  `types` and `abi` only; `wire` is a dev-dependency for one byte test.
- `kernel/src/task/drivers/` and the single 13,924-line `task/syscall.rs` are gone (Phases 08–09):
  device code is in `kernel/src/dev/`, scheduling in `sched/`, IPC objects in `ipc/`, and the
  dispatcher is `kernel/src/task/syscall/` (18 files).
- The HAL `Timer`/`InterruptController`/`BootController` traits and the HAL IOMMU types
  (`ArmIommu`, `NoIommu`, `AppleDartPt`) are deleted (Phases 14 and 18).
- `libs/ostd/src/dispatch.rs` and the kernel's `fast_ipc` module are gone (Phases 11–12).

---

## Directory Structure

```
Cellos/
├── kernel/
│   ├── residents.toml      Every kernel file and its Spec 15 category (check-kernel-residents.py)
│   └── src/
│       ├── arch/           The only place architecture code lives: x86_64/, riscv64/, aarch64/,
│       │                   legacy/ (32-bit, quarantined, does not build)
│       ├── dev/            Device ownership (DeviceRecord), NIC/block routes, DmaRemapper,
│       │                   PCIe ECAM, IRQ wait, UART, MMC, VirtIO common code
│       ├── sched/          Scheduler, SMP, hart-local state, domain switch, CPU accounting
│       ├── ipc/            Pipes, futex, cell sessions, pending mailboxes, IPC wire
│       ├── memory/         FrameRun, frame allocator, paging, regions, cell quotas, tag pool,
│       │                   TLB shootdown, Stage-2
│       ├── task/           Task/TCB, capabilities, user copy, and syscall/ (dispatcher by domain)
│       ├── cell/           CSpace (CellAuthority), service registry, hot-swap, retirement verdicts
│       ├── loader/         ELF loader, launch profiles, boot ceilings, domain admission, VA slots
│       ├── admission/      Owner-key admission gate and A/B slot format
│       ├── sync/           Lock-rank table (rank.rs)
│       ├── acpi/, boot/, fs/, hypervisor/, platform/
│       ├── catalog.rs      One authority row per install path (exported to scripts/catalog.generated.json)
│       ├── posture.rs      dev / test / release posture derivation, shared with build.rs
│       ├── policy.rs, signing.rs, audit.rs, measurement_log.rs, fsapi.rs
│       └── snapshot_engine.rs / snapshot_disabled.rs   (engine only with `snapshot-qualified`)
├── hal/
│   ├── core/               Feature-selected integration facade
│   ├── soc/                Immutable SoC facts (`riscv`, `arm-virt`, `bcm27xx`, `x86`)
│   ├── traits/             Shared HAL contracts, including `arch::kernel_abi`
│   └── arch/               RISC-V, ARM and x86 mechanism code
├── boards/                 Root board descriptors and audited fallback DTS assets
├── libs/
│   ├── abi/                Frozen kernel ABI; abi/syscalls.toml is the one syscall table
│   ├── types/              Shared primitive types
│   ├── wire/               Service contracts; proto/ holds the per-service wire modules
│   ├── ostd/               Cell runtime: service::serve loop, IPC, grants, heap, startup
│   ├── nic-driver/         Shared skeleton of the Ethernet Driver Cells
│   ├── posix-shim/         Optional libc/POSIX adapter
│   └── …                   AI, HTTP, text, ViUI, SSH, terminal, storage, authority crates
├── cells/
│   ├── tools/              init, cellbox, kms-ssh-admin, sys-tools, net-tools, wasm
│   ├── apps/               User-facing apps and Hypha cells
│   ├── demos/              Hardware + feature demos, games and smoke workloads
│   ├── drivers/            Device-driver cells
│   ├── services/           Long-lived IPC services
│   ├── tests/              Disposable guest-side test and benchmark cells
│   ├── runtimes/           Lua runtime
│   └── guests/             Hypervisor guest images and guest-side support assets
├── tools/                  Host tools (mutable-globals, vi-compiler, viui-build, …)
├── tests/integration/      Host-driven QEMU and image-level integration suites
├── scripts/                Guards, signer (cellos-sign), image builders, CI helpers, generators
└── docs/                   Living docs, specs, generated tables, research and bring-up notes
```

---

## Key Design Principles

1. **Single Address Space (SAS)** — Tier 1 Cells share one virtual address space; no TLB flush on IPC.
   Tier 2 Cells run in private page-table domains; Tier 3 runs whole guests under a hypervisor.
2. **Cellular isolation** — Cell crates forbid `unsafe`; exceptions are reviewed allowlist entries,
   and a Tier-1 crate with a `c-ffi`/`legacy-raw-ptr` entry needs a signed tier exception
   ([security model](security-model.md#tier-1-perimeter)).
3. **Capability model** — one rights record per Cell incarnation, ceilings per install path from
   `kernel/src/catalog.rs`, runtime revoke with `CapRevoke`; launch edges key on the
   kernel-measured program name ([security model](security-model.md#authority-one-rights-record-per-cell-incarnation)).
4. **Hot-swap** — supervisor-orchestrated live replacement with state through the kernel stash;
   proven for `config` only (`config-hotswap-x86`).
5. **Law 2 (Owned Buffers)** — no `&mut [u8]` across `async` boundaries.
6. **Module files** — both `foo.rs` + `foo/` and `foo/mod.rs` occur; see
   [code standards Law 5](code-standards.md#law-5-module-structure).

---

## Syscall Surface

The one source is `libs/abi/abi/syscalls.toml`: names, numbers, groups, allowlist bits, raw
kernel-only opcodes and retired numbers. `libs/abi/build.rs` generates `ViSyscall` and the
allowlist from it, `scripts/gen-syscall-headers.py` generates the C, Zig and mlibc headers
(`--check` in CI), and `libs/abi/abi/syscalls.golden.json` pins numbers and bits (Law 1). The
kernel dispatcher is `kernel/src/task/syscall/`, one handler file per domain.
