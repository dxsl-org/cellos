# TODO - Quick references for dev

## Tasks (2026-07-08 — post G2 loader redesign + boot-suite recovery)

### 🔴 High value — real bugs, QEMU-debuggable
1. Input virtqueue-poll regression (`input_bare_cell`, `input_keyboard_e2e`) — input service claims virtio-keyboard + polls virtqueue directly (Phase-03 kernel-push→userspace migration), but QMP-injected keys never surface in that poll. Debug `cells/services/input` virtqueue setup / used-ring / IRQ-notify. Input-test cell already spawns + focus-grants (cell-store lfn fix works) — isolated to key delivery.
2. virtio-gpu Driver Cell not registering (`gpu_framebuffer_initialises`) — cell doesn't spawn/claim its device (compositor falls back to software cursor); also update test's retired `"Framebuffer setup success"` marker. Likely same Driver-Cell device-claim class as #1.

### 🟡 Medium — single/self-contained
3. `mqtt_subscribe` SUBACK receive — reaches "connected"; mock broker sends SUBACK but client `mqtt_recv(5000)` times out. Investigate `mqtt_recv` timing / smoltcp RX buffering (`mqtt_publish` passes).
4. aarch64 userspace boot-to-shell regression — kernel boots + spawns init + scheduler runs, then init produces no output. Bisect Jun-12→HEAD; check aarch64 U-mode entry / crt0 `__init_array` PC-relative. See `project-arm64-peripheral-test-status.md` memory.

### 🟢 Architectural follow-ups (now unblocked)
5. Shrink kernel_fs → cell-store — now that the cell-store read works (fatfs `lfn` fix), migrate more non-bootstrap cells off `kernel_fs` into the disk cell-store to reclaim G2's kernel-size goal (keep only true bootstrap cells in VIFS1).
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



