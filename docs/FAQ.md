# Cellos — Frequently Asked Questions

---

## 1. What is Cellos?

Cellos is a Rust research OS organized around Cells. Its sole active direction
is **Cell-to-Cell Anywhere on Intel x86-64**, under
[ADR-0022](decisions/0022-intel-x86-64-c2c-only-direction.md).
Trusted native Cells use SAS/LBI; isolated native domains and VM guests use
hardware boundaries. The three-tier destination is not yet qualified on Intel.

---

## 2. Why "Cellular"?

A Cell is the fundamental unit of Cellos software, analogous to a process in Linux but
lighter in its trusted native profile. Trusted Cells may share an address space
and use ownership-aware local IPC. Remote C2C uses explicit service/transport
contracts, not shared address-space pointers or automatic application distribution.

---

## 3. How is Cellos different from Redox, seL4, or Theseus?

| | Cellos | Redox | seL4 | Theseus |
|-|------|-------|------|---------|
| Isolation | Rust type system (LBI) | Hardware MMU | Hardware MMU + formal proof | Language + type system |
| IPC | Zero-copy owned-buffer | Message passing | Capability IPC | No-copy via type safety |
| Language | Rust (no_std) | Rust | C (kernel), Rust user | Rust |
| Focus | Edge-to-Cloud, Cellular SAS | POSIX-compatible | High-assurance embedded | Live evolution / hot-swap |

Cellos shares SAS/lifecycle research interests with Theseus. Current work is
restricted to a fixed Intel C2C target, not broader hardware coverage or a
claim that every IPC path is zero-copy.

---

## 4. Why Rust nightly?

Cellos uses several nightly-only features required for bare-metal `no_std` programming:

- `-Z build-std=core,alloc` — build the standard library for bare-metal targets
- `#![feature(custom_test_frameworks)]` — test runner in `no_std` kernels
- `#![feature(naked_functions)]` — assembly stubs without prologue/epilogue

The toolchain version is pinned in `rust-toolchain.toml`; updates are deliberate and
tested.

---

## 5. Does Cellos use hardware MMU isolation?

Yes: the architecture separates trusted SAS/LBI native Cells, private-MMU
native domains and hardware-isolated guests. Rust types are not a security
boundary for arbitrary C/C++ or unaudited unsafe code. Intel x86 Tier 2 admission
and C/C++ runtime support still need qualification, and Intel VMX remains
incomplete. See [current focus](roadmap/current-focus.md).

---

## 6. What hardware does Cellos run on?

**Current target:** one exact Intel x86-64 configuration, then a second identical
node for physical C2C. The [HCL](hardware-compatibility-list.md) has no qualified
physical Intel row yet. x86 QEMU/controller and SVM guest evidence is software-only;
it does not prove Intel VMX guest execution.

Existing ARM/RPi3/RISC-V/AMD code and evidence are retained. New board, peripheral
and non-Intel platform work is parked; existing regressions may protect shared
changes. No Pi4/Pi5/HiFive or other purchase/port is scheduled by this direction.

---

## 7. Why no Linux compatibility?

A POSIX compatibility shim is tracked in `libs/api/src/services/posix.rs` for basic
syscall forwarding, but full Linux ABI compatibility is not a goal.  Cellos is a
research OS exploring a different design point; apps are written as Cells, not
POSIX processes.  For Linux app compat, see Redox or Asterinas.

---

## 8. How do I report a security issue?

**Do not open a public GitHub issue for security vulnerabilities.**

Email the maintainers at the address in `SECURITY.md` (root of repo).  We follow a
90-day responsible-disclosure window.  If no `SECURITY.md` exists yet, open a
**private** GitHub security advisory via the Security tab.

---

## 9. Where do I get help?

1. **GitHub Discussions** — Q&A category for questions; Show-and-Tell for projects
2. **GitHub Issues** — bug reports and feature requests only
3. **`docs/getting-started.md`** — step-by-step setup + common errors table
4. **Code comments** — public items have rustdoc; start with `kernel/src/main.rs`

Please search existing issues and discussions before opening a new one.

---

## 10. When is v1.0?

The v1.0 target window is **2027 H1**.  The criteria are:

- All three architectures (RV64, AArch64, x86_64) boot to shell in QEMU
- VirtIO block + input + GPU + network work without hangs
- `cargo test --workspace` passes with ≥ 80% coverage
- CI is green on every PR
- Public docs site is live

See [`docs/project-roadmap.md`](project-roadmap.md) for the current roadmap index.
