# TODO - Quick references for dev

## Tasks (2026-07-08 — post G2 loader redesign + boot-suite recovery)

### 🔴 High value — real bugs, QEMU-debuggable
*(none — input virtqueue-poll, multi-device claiming + mouse→compositor routing, and virtio-gpu registration all fixed 2026-07-10/11; see changelog)*

### 🟡 Medium — single/self-contained
*(none — aarch64 boot-to-shell regression fixed 2026-07-11: RPi3 debug probes in shared exception vectors wrote to BCM UART 0x3F215040 → recursive abort on QEMU virt; aarch64-boot suite 7/7. See changelog.)*

### 🟢 Architectural follow-ups (now unblocked)
5. ~~Shrink kernel_fs → cell-store~~ **DONE 2026-07-11** (kernel_fs 40→36MB, kernel 43.3→39.1MB; VIFS1 = bootstrap cells + 2 documented exceptions). Remaining fat, tracked as follow-ups:
   - **doom1.wad 28.8MB still in VIFS1** — DOOM reads it via mlibc → kernel Open/Read, which resolve against VIFS1 only (`kernel/src/fs.rs::file_open`). Moving it out needs mlibc/kernel-FD file I/O → VFS routing (Tier-1b C story). That is ~74% of the remaining kernel_fs.
   - **hotswap-demo-v1/v2 in VIFS1** — kernel-side hotswap (`cell/hotswap.rs`) loads the new ELF via `loader::spawn_from_path` (VIFS1/P2, no VFS from kernel). Resolves itself when hotswap orchestration migrates to the Supervisory Cell (tracked kernel tech debt).
6. Phase 07 — scoped-SUM `[deferred, NO-GO for G1]` — census + decision recorded in `.agents/260707-1726-g2-loader-redesign/phase-07-scoped-sum.md`; split into its own G2 hardening plan if revisited (RAII `SumGuard` + `copy_from/to_user` helpers).

### ⚪ Environmental — need hardware/harness, not code
7. `bench_all_pass` — RT/WCET thresholds fail under QEMU TCG; only meaningful on real hardware (RK3588 / VF2 / Pioneer).
8. littlefs2 on x86/aarch64 — currently feature-gated off (no bare-metal cross-gcc). Only needed for `/data` on those arches; requires provisioning `aarch64-none-elf-gcc` / `x86_64-elf-gcc` or a clang sysroot.

### G1 Active
- Hypha AI agent P3 boot verify → P4 tool-peripheral (robot NL control = G1 showcase)

### G2 / Deferred
- TLS server-side accept `[G2-parked]` — plan at `.agents/260623-1500-tls-server-accept/`; GATE: only for edge nodes without LB/VM
- PKU PTE key tagging `[G2]` — PTE-level key assignment; ARM64 MTE/x86 MPK base done
- DICE/RIoT attestation `[G3/hardware-gated]` — OpenTitan-backed Silo; needs real hardware
- Compositor full desktop + mouse `[G2]` — mouse path done; full desktop shell (Terminal Cell VT100) is G2
- App Platform J: L2/L3/L4 `[G2]` — Middleware, tooling, observability (see roadmap §J)
- Cell-to-Cell Anywhere G2 `[G2]` — HyParView gossip, Pkarr DNS discovery, K2 per-node key; G1 P00-P09 complete
- VirtIO-GPU PCI transport for x86 `[G2]` — P03 of GPU backend plan; requires PciRoot/ECAM adapter



