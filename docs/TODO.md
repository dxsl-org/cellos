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

### 🔴 New — x86_64 boot-to-shell broken since G2 loader redesign (pre-existing, found 2026-07-11)
9. `x86_64-boot` suite = 3/7: init spawns `/bin/block` (not in x86 VIFS1) → kernel loader fallback → **`#PF kernel va=0x0` in `paging.rs:718`** → init killed → no shell. Bisect-verified pre-existing at `d505b7e0` (before this session): the "x86 7/7 (Jul-8)" ran on a stale pre-G2 ISO. Also: x86 `service-vfs` had been silently ABSENT from the image for weeks (littlefs link failure → script Warning → shipped without vfs). Investigate the x86 spawn-fallback NULL deref (kernel-mode read of page 0 during the `/bin/block`→`/bin/vfs` spawn misses); belongs to the x86 q35 completion plan (`.agents/260616-1639`, P02-P05 pending). The #PF handler prints no RIP — add it first.

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



