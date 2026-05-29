# ViOS Public Roadmap

> **Living document** — updated each release.  Internal phase details live in
> `.agents/`; this file is the public-facing narrative.

---

## What is ViOS?

ViOS is a research operating system written in Rust that replaces hardware MMU
isolation with **Language-Based Isolation (LBI)**.  Instead of separate processes,
software is organized as **Cells** sharing one address space — isolated by the
Rust type system, not page-table switches.  The result: zero-copy IPC, µs-range
context switches, and a kernel that fits in < 10 MB.

Primary target: **RISC-V 64** (QEMU `virt` board).  Secondary: AArch64, x86_64.

---

## Now — v0.2.x "Mycelium" (current)

What works today:

| Area | State |
|------|-------|
| RV64 HAL (SV39, PLIC, SBI) | ✅ Stable |
| Kernel ELF loader (PIE + relocations) | ✅ Stable |
| Basic shell REPL (`ls`, `cat`, `echo`) | ✅ Stable |
| RamFS VFS with `/bin/` + `/tmp/` | ✅ Stable |
| VirtIO block device | ✅ Fixed (was hanging) |
| Keyboard input (VirtIO) | ✅ Fixed (was deadlocking) |
| FileHandle IPC (capability model) | ✅ Implemented |
| AArch64 HAL | ✅ Implemented |
| x86_64 HAL | ✅ Implemented |
| RV32 + AArch32 HAL stubs | ✅ Implemented |
| Security: STRIDE model + fuzzing infra | ✅ Implemented |
| CI/CD (GitHub Actions, multi-arch) | ✅ Running |
| External ELF loading from `/bin/` | ✅ Implemented |
| Unit + integration test suite | 🔄 In progress |
| VFS mkdir/rmdir/unlink IPC | ✅ Implemented |

---

## Next — v0.3 "Rhizome" (in progress)

Focus: **making the system useful for real workloads**.

- **Full VFS service** — FAT32 on VirtIO block, persistent writes, disk quota
- **Complete input service** — focus routing, modifier tracking, mouse events
- **Network stack** — smoltcp on VirtIO-net, TCP/UDP, DHCP
- **Enhanced shell** — I/O redirection, pipes, job control, tab completion
- **Standard utilities** — `cp`, `mv`, `grep`, `find`, `wc`, `tee`, `head`, `tail`
- **Test coverage** — ≥ 80% on `kernel/src/`; QEMU-driven integration suite
- **Benchmarking** — context-switch < 100 µs, IPC < 50 µs validated in CI

---

## Later — v1.0 "Forest" (target: 2027 H1)

The v1.0 milestone means **all three architectures boot to a shell prompt** in
QEMU with VirtIO block + input + GPU + network all working, CI is green on every
PR, and the public docs site is live.

| Milestone | Description |
|-----------|-------------|
| Three-arch parity | RV64, AArch64, x86_64 boot to shell in QEMU |
| Compositor & GPU | Basic Wayland-compatible surface compositor |
| Lua + MicroPython | Scripting runtimes usable from the shell |
| Hot migration | Live Cell upgrade without reboot |
| Docs site | GitHub Pages with API reference + tutorials |
| Community | ≥ 10 active contributors; first external PR merged |

---

## Stretch — v1.x (post-1.0)

- Real hardware targets (HiFive Unmatched, Raspberry Pi 4/5)
- Wayland compatibility protocol layer
- Container / sandbox guest model
- Formal verification of capability model (Kani / Coq)
- RISC-V 128-bit support

---

## How to Track Progress

- **Releases**: [GitHub Releases](../../releases) — tagged at each milestone
- **Changelog**: [`docs/project-changelog.md`](project-changelog.md)
- **CI status**: badge at the top of the README

---

## Community Contributors

*This section will grow as contributors join.*  If you shipped a patch, opened
a useful issue, or wrote documentation — thank you.  Your name belongs here.

---

*Last updated: 2026-05-29 | Branch: main*
