# Cellos Codebase Summary

**Project**: Cellos (Jarvis Hybrid OS)
**Version**: 0.2.1-dev (Mycelium Era)
**Language**: Rust (nightly, `no_std`)
**Crates**: 111 active workspace members (`cargo metadata --no-deps`)
**Last Updated**: 2026-09-02 (capability-registry semantics refreshed)

---

## Quick Stats

| Area | Crates | Key Highlights |
|------|--------|---------------|
| Kernel | 1 | Size tracked in [generated metrics](code-metrics.generated.md); shared completion queue (`NET_RX` + finite `TIMER`), supervisor hotswap, exact launch profiles |
| HAL | 23 | `hal/core`, 4 SoC crates, 15 trait crates, 3 arch crates; HAL↔kernel Rust ABI signatures are single-sourced in `hal/traits/arch/src/kernel_abi.rs` |
| Boards | 1 | `cellos-boards` no_std descriptor crate; 7 active integration descriptors plus placeholder-only docs for `q35-x86_32`, `virt-riscv32`, `virt-aarch32` |
| Libraries | 10 | `types`, `api`, `ostd`, plus attestation, HTTP, text, mlibc shim, agent protocol, and ViUI crates |
| Tools | 5 | `init`, `shell`, `sys-tools`, `net-tools`, `wasm` |
| Apps | 7 | `fb-console`, `robot-dashboard`, Hypha core/gateway/tool cells |
| Demos | 21 | hello, ViUI, hotswap, HTTP(S), DOOM/Tetris, audio, robot, and peripheral demos |
| Drivers | 17 | disk, serial, e1000, nvme, virtio, GPIO/I2C/SPI/PWM/ADC/CAN, BCM/SiFive variants, wasm |
| Services | 12 | platform, vfs, net, net-broker, compositor, input, config, power, hypervisor, silo, supervisor, httpd |
| Tests | 13 | disposable bench, VFS, srv, hypervisor, peripheral, mlibc/posix, input, silo, W^X, isolation lanes |
| Runtimes | 1 | Lua 5.4 remains in-tree; MicroPython is historical and no longer an active workspace member |

---

## Directory Structure

```
Cellos/
├── kernel/                 Boot, scheduler, completion queue, loader, memory, policy
├── hal/
│   ├── core/               Feature-selected integration facade
│   ├── soc/                Immutable SoC facts (`riscv`, `arm-virt`, `bcm27xx`, `x86`)
│   ├── traits/             Shared HAL contracts, including `arch::kernel_abi`
│   └── arch/               RISC-V, ARM, and x86 mechanism code
├── boards/                 Root board descriptors and audited fallback DTS assets
├── libs/                   API/types/ostd plus attestation, HTTP, text, ViUI, shims
├── cells/
│   ├── tools/              Always-running bootstrap and CLI cells (`init`, `shell`, `sys-tools`, `net-tools`)
│   ├── apps/               User-facing apps and Hypha cells
│   ├── demos/              Hardware + feature demos, games, and smoke workloads
│   ├── drivers/            Shared device-driver cells (virtio, e1000, nvme, gpio, serial, wasm, ...)
│   ├── services/           Long-lived IPC services (`vfs`, `net`, `net-broker`, `supervisor`, ...)
│   ├── tests/              Disposable guest-side test and benchmark cells
│   ├── runtimes/           Lua runtime (current in-tree runtime)
│   └── guests/             Hypervisor guest images and guest-side support assets
├── tests/integration/      Host-driven QEMU and image-level integration suites
├── scripts/                Guard rails, disk/image builders, CI helpers, metrics scripts
└── docs/                   Living docs, specs, research, patterns, and bring-up notes
```

---

## Key Design Principles

1. **Single Address Space (SAS)** — all Cells share one virtual address space; no TLB flush on IPC
2. **Cellular isolation** — most Cell crates forbid `unsafe`; exceptions are documented, audited driver/service boundaries where hardware or FFI requires it
3. **Capability model** — capability entries enforce single ownership, permission bits, optional lease expiry, close, and owner-exit revocation; spawn-time `CapSet` intersection remains the separate monotonic-downgrade boundary
4. **Hot-swap** — `ViStateTransfer` on shell/config/vfs; 5-step live Cell replacement
5. **Law 2 (Owned Buffers)** — no `&mut [u8]` across `async` boundaries
6. **Law 5 (No mod.rs)** — `foo.rs` parallel to `foo/` everywhere

---

## Syscall Surface (selected)

| ID | Name | Description |
|----|------|-------------|
| 0–3 | Send/Recv/Call/Reply | Core IPC |
| 12 | SpawnFromPath | Load cell ELF from /bin/ |
| 13–15 | OpenCap/ReadCap/CloseCap | Capability-based file I/O |
| 201 | RecvTimeout | IPC with monotonic-tick deadline |
| 202–203 | SendGather/RecvScatter | Scatter/gather IPC (Phase 20) |
| 300 | GpuFlush | Blit pixel rect to VirtIO GPU |
| 400 | HotSwap (retired/reserved) | Legacy whole-sequence opcode; decodes `Unknown` |
| 401 | HotSwapReady | Ready signal for state transfer (bit 32) |
| 413–415 | Freeze/Resume/Kill | SupervisorCap-gated cutover primitives |
| 419 | QueryHotswapReady | SupervisorCap-gated readiness query (bit 32) |
| 421 | SpawnReplacement | SupervisorCap-gated replacement launch |
| 422 | PauseService | SupervisorCap-gated quiesce point before Snapshot |
