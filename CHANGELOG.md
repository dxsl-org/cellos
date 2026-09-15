# Cellos Changelog

All notable changes to Cellos are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

---

## [Unreleased]

### 🚀 Improvements
- cells/tetris-c: the Tetris-OS port no longer depends on an unfetchable gitlink. Its upstream source (Banaxi-Tech/Tetris-OS, MIT, commit `66c4466`) is vendored into `cells/demos/tetris-c/src/c/tetris-os/` with its licence, `build.rs` records the provenance, and the one change we carry — the idle loop's per-ISA wait instruction (`hlt` on x86, `wfi` on RISC-V, because upstream's x86 form does not assemble for riscv64) — is tracked instead of sitting as uncommitted dirt in a nested clone. `gen_disk.ps1` now probes the real path (`src/tetris.c`), so the cell is packaged rather than silently skipped, and the platform shim gained the two hooks upstream actually calls (`keyboard_set_layout`, `speaker_music_update`) so `cargo build -p tetris-c --target x86_64-unknown-none` links end to end. The riscv64 link still needs the xpack picolibc, which is why the CI exclusions keep the cell and their comments now say so.
- kernel/rv64: the trap boundary now holds the root invariant the intermittent `/srv` fault exposed. Trap entry parks the interrupted `satp` in the trap frame — RV64-only, 304 bytes including padding, so the x86_64/aarch64 layouts and their 288-byte frames are untouched — and installs the kernel root from the fixed-name `VI_KERNEL_SATP` symbol before any kernel code runs, so the timer tick's console poll and the PLIC claim never issue an MMIO access under a Cell root. `__trap_exit` restores the frame's root only when the live value differs (a task resumed through a context switch already has it; a primed first-entry frame carries 0), and `SwitchPlan::SameDomain` now programs its domain's root instead of the zero no-write sentinel — leaving it at `(0, 0)` would resume a Cell under the kernel root with the kernel's mappings visible to its S-mode code. Private-domain user copies already branch on the resident root, so they take their stricter PTE-walk alias path inside a handler, and `validate_kernel_range` now walks the kernel root and refuses user addresses passed as kernel buffers. Evidence: 10/10 consecutive clean boots on the phase-03 direct-drive instrument (`posix-shim-test` reaching `POSIX-RENAME: OK`, zero `[KERNEL PANIC]`), 10/10 green `cargo test --test srv-cellosfs` runs, and the RV64 domain suite green on one and two harts with the new `S22-RV64-RESUME-ROOT` fixture plus a `resume-root` case in `scripts/qemu-native-domain-test.sh`. Hosted: the `CellosFS /srv Integration Test` job is green on run 34933281918 (its first green push); the `C2C Broker Oracle` job's boot no longer prints the 487 PLIC-fault kernel exceptions that used to halt it (923 on a local control) — it now reaches Embedded Init and fails on a cell-side fault, which is the C2C lane's own remaining defect; and the `Network Data-Path Integration (riscv64)` suite goes from 4 panicking failures among 6 to 2 failures with zero kernel exceptions (`52 passed; 2 failed`), the remainder being the two HTTP-server tests that failed without a panic before the fix too. `.agents/260914-ci-gate-restoration/phase-04-trap-root-discipline.md`.
- rv64/test-runner: `scripts/qemu-native-domain-test.sh` no longer pins the `SMP-FAULT-RETIREMENT` canary to `generation 99`. That field is `NEXT_DOMAIN` — how many private AddressSpaces had been built when the synthetic trap published its record — and the unmodified kernel reads 133 on this workstation today against 99 in the phase-07 captures, so the pin failed a correct kernel at `--harts 2` (measured on both the pre-change and post-change builds). The canary is now matched by cell, task, injected cause, and null pc/addr, with the measurement recorded in the comment.
- vfs/srv: the RedoxFS retirement is complete end to end. P5 ships raw and the guest formats it as CellosFS Native (no host `redoxfs-ar` build in either image script), the suite, its CI job, its cache key and its labels are `srv-cellosfs`, ADR 0016 supersedes ADR 09b and marks ADR 0002's backend as historical while its G1/G2 policy survives, and the remaining RedoxFS labels in kernel/VFS comments plus the orphaned `backend_stub.rs` are gone. Nothing in the workspace depends on `third_party/redoxfs`; it stays vendored for reference. The job's red state at that point was the kernel trap-root fault, not the filesystem — closed by the trap-boundary fix above.
- kernel/rv64: the kernel-fault panic prints the live `satp` beside the kernel's own, and the kernel records that root when it activates its page table. This is the instrument that proved the intermittent `console_drv::poll` fault (UART line-status load at `0x10000005`, `scause=13`) runs with a private Cell root live — `satp` ASID 1 — instead of the kernel root's ASID 0, because the trap entry never restores the kernel root. `hal/arch/riscv/src/rv64/{trap,domain,paging}.rs`; the fix is designed in `.agents/260914-ci-gate-restoration/phase-04-trap-root-discipline.md`.
- ai/rpi3: the inference stack runs on the physical Raspberry Pi 3 over the existing static-TFTP netboot lane. The fixture's golden ids and embedding reproduce on real Cortex-A53 (`ai-test` PASS, 8 tokens in 59 ms), and `tools/rpi3-netboot/serve-ai-oracle.ps1` boots a chosen payload and captures the board's console. The checkpoint no longer has to live inside the kernel: `service-ai` reads `/bin/ai-model.gguf` and then `/mnt/sd/ai-model.gguf` (the card's own FAT volume) and logs the path it used, and `build-aarch64-cells.ps1 -AiCells` builds the AI cells with no checkpoint at all — the 43.6 MB embedded-payload variant panics the board's kernel at compositor setup while the same image passes on QEMU aarch64 virt.
- ai/oracle: the inference oracle now runs on **both** ISA legs of Spec 24 CP-3's gate. `scripts/run-ai-inference-oracle-qemu.sh --arch riscv64|aarch64` builds, signs, and boots the same isolated image on either architecture, the CI job is a two-leg matrix, and each evidence artifact names the architecture and model it ran against. On aarch64 (a softfloat target, so its float arithmetic is software) the fixture's golden ids and embedding reproduce exactly, and a real 26.7 MB checkpoint generates coherent prose — a numerics cross-check of the integer Q8_0 kernels on a second ISA, not just a second boot test.
- ai-engine/tensor-math: **4.3× faster inference in the Cell, 1.8× on the host.** Activations are now quantized to Q8_0 once per projection and each 32-weight block is an exact integer dot product scaled by `d_w·d_a`, instead of decoding every block to f32 and running 32 f32 multiply-accumulates. Measured: 24 tokens in 1595 ms in the QEMU RV64 cell (was 6.85 s), 23.6 ms/token on a 135M-parameter Q8_0 checkpoint on the host (was 41.6 ms; 211.5 ms before the profile slice), and the per-kernel rate goes 6.8 → 12.5 GFLOP/s — faster than the *dense* f32 kernel in the same crate. The integer sum is exact in `i32` (four orders of magnitude of headroom), so the only error is the activation's half-step quantization; the golden reference in `scripts/gen-ai-test-model.py` now mirrors the shipped arithmetic and still produces the same eight token ids.
- ai-engine/tensor-math: the activation quantizer (`quant::q8_0_row_from_f32`) and its f16 encoder (`quant::f32_to_f16`, round-to-nearest-even, checked against all 65 536 half patterns) are the new public kernels; `matvec_q8_0` stays as the f32 reference the integer kernel is bounded against and as the benchmark's comparison row.
- ai-engine/tensor-math: the inference engine now builds at `-O2` instead of the workspace's size default. The Q8_0 matvec kernel — which is the entire decode path — was spending its time in out-of-line helper calls per 32-weight block. Measured: **5.1× on the host** (135M-parameter Q8_0 checkpoint 4.7 → 24.0 tokens/s; `stories15M` 24.1 → 4.7 ms/token) and **13% faster in the QEMU RV64 cell** (24 tokens in 6.85 s vs 7.89 s), with the `service-ai` image 6 KB smaller. `-O3` was measured too: faster on the host, 11% slower in the cell (TCG softfloat dominates), so it was rejected.
- ai-engine: host benchmark `cargo bench -p ai-engine --target x86_64-unknown-linux-gnu --bench cpu_engine` reports load, prompt-processing, per-token decode rate, and per-kernel GFLOP/s at the model's own shapes — the instrument behind those numbers, never asserted in CI.
- ai-test: the in-cell oracle prints its own generation timing (`[ai-test] generate: N tokens in X ms`), so cell-side inference speed is visible on every oracle run.
- hypha: `llm-gateway` now asks the on-device inference Cell first (`/bin/ai`, Spec 24 `service::AI`) and names the backend and model it used per turn; the OpenAI-compatible network endpoint stays as the fallback for images with no local model, and only absence (`NoService`/`NoModel`) or a prompt above `ai_proto::MAX_PROMPT_BYTES` reaches it.
- ai/httpd: expose `POST /api/infer` as a bounded JSON consumer of the frozen `AiClient`; canonical RV64 hostfwd QEMU coverage proves prompt → inference Cell → response, `max_tokens`, and caller-error handling.
- httpd/build: wait for `Content-Length` bodies across TCP segments, reject prompts over the AI wire limit, and build/sign the current `service-httpd` binary into `gen_disk.ps1` instead of packaging a stale `/bin/httpd`.
- vfs: introduce CellosFS Native pure-Rust CoW extent engine (`libs/cellos-fs`), replacing external RedoxFS and LittleFS dependencies with power-loss-resilient dual-ring superblocks and vector block DMA
- bench: implement native stateful workload scenario (1,000 ops, checkpoints, v1->v2 hotswap, and VFS restart recovery) verified in QEMU
- robot: implement LAB-01 carrier transfer (06B), BASE-01 tray handoff (07B), and ASSEMBLY-01 stationary coupling (08A/08B) QEMU native witnesses with real CellosFS Native trace logging
- lab: add a bounded model-only LAB-01 carrier-transfer contract with independent host fault plants
- base: add a bounded model-only secured-tray handoff contract and independent host fault plants
- performance: add immutable profile-bound RV64 benchmark captures with separate validity, target, and regression verdicts
- platform: enumerate checked inclusive MCFG ECAM ranges across buses with canonical BDFs and per-bus VT-d contexts
- x86_64: install 256 vector/CPL-aware IDT entries and verify them with an isolated `x86-idt-cpl3-test` two-task Ring-3 QEMU oracle
- security: land non-admissible Tier 1 admission catalog/test infrastructure; Phase 04 production evidence remains blocked
- kernel: enforce post-relocation W^X page permissions
- signing: gate cells through F1/F5 admission
- ipc: add bounded per-cell completion queues
- vfs: add inherited directory capabilities
- security: attest caller identity across VFS IPC
- hypha: native AI agent Cell, LLM gateway and chat loop
- net: TLS server certificate verification via embedded-tls
- ostd: http-core crate with HTTP/1.1 and JSON client
- disk: package app-https-demo as /bin/https-demo binary
- security: signed operator policy with Ed25519 in-kernel verify
- security: spawn-time capability intersection and delegation
- shell: Phase 17 parser, pipes, redirects, history, aliases
- net: smoltcp 0.11 Cell with DHCP and VirtIO NIC driver
- compositor: software blending, z-order, damage, 30 FPS
- input: US QWERTY keymap, modifier tracking, focus dispatch
- scripting: Lua 5.4 multi-line REPL with VFS bindings
- scripting: MicroPython v1.24.1 for RISC-V bare-metal
- hot-migration: ViStateTransfer, HotSwap syscall, grant chains
- bench: /bin/bench with 4 scenarios and perf CI integration
- community: CODE_OF_CONDUCT.md and contributor dev tooling
- doom: Freedoom Phase 1 port, boots and renders first frame
- vfs: add OP_MKDIR, OP_RMDIR, OP_UNLINK to IPC protocol
- kernel: add TryRecv, NetTx, NetRx, StateStash, StateRestore syscalls
- performance: release builds for all bootstrap table entries

### 🐛 Fixes
- hypha: restore `/bin/hypha` launchability. The reviewed `(shell, Path, /bin/hypha)` launch edge carries `spawn`, but the shell's VFS+grant spawn takes the ELF route, which `launch_profile::authorize` refuses for capability-bearing targets, and the raw-path fallback resolves only through the kernel loader's VIFS1 (the block table is never probed on RV64). Both routes failed, so the app printed `command not found`; `gen_disk.ps1` now stages `/bin/hypha` and `/bin/tool-spawn` into VIFS1, the same class as `/bin/bench`.
- lab/base: reject reactivation of retired configuration epochs so rollback cannot revive stale readiness observations
- base: revoke stale active dispatch authority when newer authenticated cross-job evidence reports an exclusion
- bench: propagate scenario failures, validate intended IPC peers, fix control-loop timeout units, and retain strict serial JSON evidence
- ostd/fs: upgrade `vfs_call` to `service_call_typed_bounded` with bounded timeout, sender-masked receive, and caller generation poisoning after receive errors
- vfs/cellos-fs: fix inode block allocation for inode numbers > 8 with multi-slot superblock tracking, and enforce strict rmdir directory checks
- supervisor: authorize bench role for VFS restart recovery and fix 3-byte frame parsing in hostile backend recovery
- kernel/ostd: keep grant-owner handles task-local and fix death-subscription bookkeeping quota drift across duplicate watches, delivery, cancellation, and task retirement
- e1000: fail closed on DMA authorization/OOM and program rings/descriptors with authorized IOVAs
- nvme: retain authorized IOVAs for queue bases, Identify PRPs, and sector I/O buffers
- drivers: publish initialized state before registration and fail closed when registration is rejected
- pcie: preserve exclusive Driver Cell BDF ownership and DMA pins across competing probes or unacknowledged IOMMU publication/teardown, with spec-correct RISC-V IODIR/IOTINVAL/IOFENCE batches that order prior device reads and writes before reclamation
- x86_64: preserve user state and balance CPL3 GS/PKRU across IDT, suspended SYSCALL, fresh IRET, and scheduler switches
- x86_64: correct the bootstrap SysV stack phase with an 8-byte synthetic bottom-frame slot before tail-entering Rust
- tools: validate MemInfo frame conservation and nonzero page size before independently rounding displayed KiB values
- kernel: clean completion waiter lifecycle safely
- boot: reject snapshots from mismatched RAM bases
- build: align cross-target CI and image generation
- hypha: plaintext transport workaround for net cell TLS crash
- hypha: NetClient.tcp_send now handles Data reply correctly
- hypha: foreground shell spawn now waits on child via sys_wait
- net: boot loop caused by wrong WaitForEvent tick unit
- lua: pcall binding, picolibc link, heap sbrk stub
- doom: posix fseek/ftell, vsnprintf precision, fatfs short-read
- embedded-fs: emit FAT16 to fix CorruptedFileSystem on mount
- rv32: gate rv64 module behind target_arch check
- x86_64: AT&T syntax for global_asm, HHDM PDPT NX bit
- git: untrack .logs/hook-log.jsonl causing perpetual dirty state

---

## [0.2.1] - 2026-06-08

### 🚀 Improvements
- viui: RenderCtx bundles canvas and FontContext for paint calls
- viui: FontContext with GlyphAtlas and 8x8 bitmap fallback
- viui: touch events added to Event enum
- viui: ProgressBar, Slider, TouchArea widgets
- viui: Animatable trait, Tween, easing module, AnimatedSignal
- viui: GpuCommandBuffer as struct field for allocation reuse
- readme: accurate build targets and correct GitHub URL

### 🐛 Fixes
- viui: Slider returns subscribe handle from collect_dirty_handles
- viui: ProgressBar label uses font-aware char_width

---

## [0.2.0] - 2026-05-01 "Mycelium Alpha"

### 🚀 Improvements
- rv64: SV39 paging, PLIC, SBI, UART, ELF loader with PIE
- kernel: basic shell, VirtIO block device, VirtIO keyboard
- hal: AArch64, x86_64, RV32, AArch32 HAL implementations
- security: STRIDE threat model and QEMU CI boot test

---

[Unreleased]: https://github.com/dxsl-org/Cellos/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/dxsl-org/Cellos/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/dxsl-org/Cellos/releases/tag/v0.2.0
