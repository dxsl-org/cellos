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

### ⚪ Environmental
7. ~~`bench_all_pass`~~ **DONE 2026-07-11** — bench now prints an unconditional `BENCHMARK SUITE COMPLETE` marker; the QEMU CI gate is machinery-completion (green), while `ALL BENCHMARKS PASS` (latency thresholds) remains the real-hardware acceptance gate (RK3588 / VF2 / Pioneer — run `bench` from the shell). bench+bench-probe live in VIFS1 (children spawn via kernel `sys_spawn_pinned`).
8. ~~littlefs2 on x86/aarch64~~ **DONE 2026-07-11** — no cross-gcc needed: plain clang + `third_party/freestanding-include` (string.h/inttypes.h declarations; implementations from compiler_builtins + POSIX shim; x86_64 gets a local str* shim in vfs since the api POSIX module is mlibc-gated off there). aarch64 suite 7/7 with littlefs vfs. x86_64 builds+links; runtime verify blocked by #9.

### 🔴 x86_64 boot-to-shell (pre-existing; PARTIALLY fixed 2026-07-11)
9. **Part A — FIXED**: the `#PF kernel va=0x0` that killed init during `/bin/vfs` load was `virt_to_phys` extracting the PTE address with `& !0xFFF`, keeping bit 63 (**PTE_NX** — set on every user RW/RO page) → "phys" `0x8000…` → +HHDM wrapped to a NON-CANONICAL pointer → #GP (ec=0, stale CR2) masquerading as #PF va=0. Fixed with `PTE_ADDR_MASK` (bits 51:12) at every entry-extraction site (hal walkers + kernel `virt_to_phys`). The #PF handler now prints rip/cs/rsp + kernel-text return-address candidates. **VFS now loads and runs on x86** (RamFS up, mounts graceful-fail on the diskless ISO boot).
   **Part B — OPEN (x86 q35 P02 scope)**: suite still 3/7. After its mount sequence, vfs dies at `[#PF user] va=0xffffffffffffffff rip=0x10202d1c7` and the kernel logs `SetTimer (bit 11) denied for tid 2` — **vfs never calls SetTimer**: instrumentation showed `SetTimer(ticks=4328748342 ≈ a vfs-image POINTER) pc=0x102000000(stale)` — i.e. an x86 **syscall re-dispatch reads the syscall number as user CS (0x23 = 35 = SetTimer)** with a pointer as arg (matches the known "x86 ViTrapFrame no free slots / scratch regs" fragility). The vfs allowlist deliberately does NOT include SetTimer so this denial stays a CANARY (allowing it turns the corruption into an unbounded sleep that hangs boot). Investigate the x86 syscall entry/resume path (`hal/arch/x86` syscall asm + yield-in-syscall context save) — x86 q35 plan P02 (`.agents/260616-1639`).

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



