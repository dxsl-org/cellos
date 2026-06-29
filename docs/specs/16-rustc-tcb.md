# Spec 16 — rustc as Cellos Trusted Computing Base

> **Status**: Ratified 2026-06-29 — does not change without explicit architectural review.
>
> This document specifies the role of `rustc` as Cellos's load-bearing TCB, the guarantees
> it provides, its limits, risk mitigations, and the operational policies that follow.
> It is the authoritative reference. The summary in `docs/specs/00-context.md §5` is
> derived from this spec.

---

## 1. What "TCB" Means for Cellos

A **Trusted Computing Base** (TCB) is the set of components whose correctness is
*assumed*, not verified — the security of the entire system depends on them being bug-free.
Every component outside the TCB is verified *by* the TCB.

Traditional OSes have a small, hardware-enforced TCB: the kernel running in ring-0/EL1.
All other code runs unprivileged; hardware MMU enforces the boundary.

Cellos takes a different bet: **the address space is shared** (SAS — Single Address Space).
There is no hardware MMU boundary between Cells. The boundary is enforced by the Rust
type system at compile time. This shifts the TCB:

| Traditional OS TCB | Cellos TCB |
|--------------------|------------|
| Kernel (ring-0 code) | `rustc` (compiler) |
| Hardware MMU | Cellos kernel (scheduler, IPC, cap enforcement) |
| CPU privilege model | `libs/api` + `libs/types` (stable ABI) |

`rustc` becomes load-bearing because every Cell isolation invariant is enforced by the
compiler, not hardware. If `rustc` is wrong, the whole Cell boundary model collapses.

---

## 2. What rustc Guarantees (and Why Cellos Depends on Each)

### 2.1 `#![forbid(unsafe_code)]` enforcement

Every Cell crate carries `#![forbid(unsafe_code)]`. This is a **compile-time hard gate**:
rustc refuses to compile a Cell if any `unsafe` block exists anywhere in its dependency
tree (excluding explicitly excepted crates).

**Why load-bearing**: without this guarantee, any Cell author could write a raw pointer
cast and break out of the SAS boundary undetected. The kernel has no way to verify Cell
binaries at load time — it trusts rustc.

### 2.2 Ownership and Borrow Check (Memory Safety)

Rust's ownership rules ensure:
- No two live mutable references to the same memory can coexist.
- No reference can outlive the data it points to.
- No use-after-free, double-free, or dangling pointers.

**Why load-bearing**: Cells share one physical address space. A memory-safety violation
in one Cell can directly read or corrupt another Cell's heap. The hardware has no
protection wall. `rustc` is the only wall.

### 2.3 Type Safety (No Transmute Across Cell Boundaries)

Type transmutation (`mem::transmute`, raw casts, union punning) is `unsafe`. Combined
with `#![forbid(unsafe_code)]`, no Cell can reinterpret another Cell's data layout.

**Why load-bearing**: IPC between Cells passes typed values (via `api::ipc::encode` /
`postcard`). If a Cell could freely transmute, it could forge a `CapSet` handle or a
`GrantId` to escalate privilege.

### 2.4 `Send + Sync` Enforcement (Concurrency Safety)

`rustc` enforces `Send`/`Sync` bounds at compile time. A `!Send` type cannot be moved
across task/thread boundaries; a `!Sync` type cannot be accessed concurrently without
synchronization.

**Why load-bearing**: Cellos runs tasks (Cells) concurrently on multiple harts/cores. A
data race in a Cell would be an address-space-wide corruption event. Hardware offers no
mitigation in SAS.

### 2.5 Lifetime Validity (No Dangling References Across Async Await Points)

Rust's lifetime rules, combined with the `async` borrow checker, ensure no reference
escapes the scope of its owner across `.await` yield points.

**Why load-bearing**: See **Law 2** (Owned Buffers for Async). Cellos IPC passes owned
`Box<[u8]>` across cell boundaries for async paths precisely because a borrowed `&[u8]`
would violate this guarantee if the owning future were dropped before the async IPC
completes.

---

## 3. What rustc Does NOT Guarantee

These are the hard limits of LBI. They are handled by the Cellos kernel or hardware
layers, not by `rustc`.

### 3.1 Capability Revocation

J-Kernel (Cornell 1999) formally proves: a type-safe language prevents capability *forgery*
but not *stale authority retention*. If Cell A holds a `CapRef` and the kernel revokes
it, Cell A's Rust reference to the object does not disappear. Only the kernel's `CapSet`
enforcement — running in privileged code — can enforce revocation. This is why
`CapSet` stays in the kernel (see Spec 15 §1.2).

### 3.2 Time and Scheduling

rustc cannot enforce preemption or bound CPU time. A Cell can spin in a tight loop and
starve other Cells. The scheduler in the kernel enforces time-slicing via the timer
interrupt, which arrives in privileged mode.

### 3.3 Side Channels (Spectre / Meltdown)

Memory safety is distinct from speculative-execution leaks. A Cell cannot *directly*
read another Cell's memory (type safety), but it may be able to *infer* values via
timing side channels. Mitigations are hardware-level (ARM MTE pointer tagging, x86 MPK
domain separation — see Spec `layer2_hw_security`).

### 3.4 Supply Chain / Compiler Compromise

If the `rustc` binary itself is compromised (malicious toolchain injection, LLVM
backdoor), all Cell isolation guarantees collapse. This is a structural dependency;
see §5 for mitigation.

### 3.5 Trusted I/O Path

`rustc` cannot guarantee that a Driver Cell correctly interprets hardware registers.
A buggy GPIO driver can assert a physical pin on the wrong device. Hardware capability
boundaries (IOMMU for DMA) and MMIO BAR allowlists (kernel-enforced) are separate
layers outside `rustc`'s purview.

---

## 4. TCB Component Inventory (smallest → largest)

| Component | Role | Approx LOC | Notes |
|---|---|---|---|
| `rustc` (nightly Rust compiler + LLVM backend) | Enforce all LBI invariants listed in §2 | ~3–5M (compiler) + ~1M (LLVM) | Open-source; Ferrocene-audited subset for ARM64/x86 |
| Cellos kernel | SAS allocator, preemptive scheduler, IPC, ELF loader, CapSet, manifest check | ~11.5K | Only privileged code; strict boundary law per Spec 15 |
| `libs/api` | Stable ABI — syscall numbers, capability types, IPC encoding | ~2K | Law 1: any change requires 2× user confirmation |
| `libs/types` | Primitive types shared by kernel and Cells (`VAddr`, `PAddr`, `ViError`) | ~800 | Law 1: same change protocol |

**Outside the TCB by design:**

- Cell code (`#![forbid(unsafe_code)]`) — verified *by* the TCB, not *part of* it.
- C/Zig Tier 1b libraries via FFI — sandboxed by the `unsafe` boundary and grant/IPC validation.
- Lua/VM guests — sandboxed by interpreter manifest restrictions or hypervisor Stage-2 paging.
- `std` / `alloc` — Cell-visible only via `libs/ostd`; TCB does not extend to `std` internals.

---

## 5. Risk Mitigations for rustc-as-TCB

### 5.1 Open Source Auditability

rustc is fully open source. Any Cellos developer can audit compiler behaviour. This is
strictly better than Singularity's Bartok compiler (closed-source, auditable only by MSFT).

### 5.2 Ferrocene: ISO 26262 ASIL-D Certified Subset

[Ferrocene](https://ferrocene.dev) is a safety-qualified toolchain derived from rustc. It
provides formal qualification evidence for ISO 26262 (automotive), IEC 61508 (industrial),
and IEC 62278 (rail) standards.

**Qualified targets (as of 2026-06):**
- `aarch64-unknown-none` ✅
- `x86_64-unknown-none` ✅ (in progress)
- `riscv64gc-unknown-none-elf` ⚠️ **Not yet qualified** — ETA 12–24 months from now.

> ⚠️ **Do not make safety claims for RISC-V builds until Ferrocene qualifies the
> riscv64 target.** G1 RISC-V is development/demonstration only.

**When to adopt Ferrocene**: Before G2 production release on ARM64 hardware (RK3588).
Ferrocene is a drop-in replacement for `rustc`; no source changes required.

### 5.3 miri: MIR Interpreter for Unsoundness Detection

`miri` interprets Rust's Mid-level IR and detects undefined behaviour — including
unsoundness in `unsafe` code — that rustc's static analysis would miss. Run `miri`
in CI on:
- All kernel `unsafe` blocks (layout assumptions, raw pointer arithmetic, interrupt handlers)
- `libs/types` and `libs/api` FFI boundary code

This catches a class of bugs that would silently corrupt the TCB.

### 5.4 Reproducible Builds + Toolchain Pin

Cellos pins `rustc` to a specific nightly via `rust-toolchain.toml`. Every CI run
uses an exact pinned hash. This prevents accidental toolchain drift and provides a
stable baseline for security review.

### 5.5 P0 Incident Protocol: Soundness Hole in rustc

If a soundness bug is discovered in rustc's borrow checker or codegen that allows `unsafe`
code to bypass `#![forbid(unsafe_code)]` constraints:

1. **Immediately halt** all Cell code deployment.
2. **Downgrade** to the last known-good rustc nightly (before the soundness regression).
3. **Audit** all Cells compiled with the compromised compiler version for exploitation.
4. **Upgrade** to a patched rustc once the fix lands; re-compile all Cells.
5. **Document** in the security changelog with CVE reference if applicable.

This is a P0 kernel security breach equivalent — treated the same as a kernel privilege
escalation vulnerability.

---

## 6. Policies Derived from This Spec

These are operational mandates for Cellos development. Violating any of these invalidates
the safety argument in §2.

| Policy | Rule | Rationale |
|---|---|---|
| **F1** | Every Cell crate MUST carry `#![forbid(unsafe_code)]` | Compiler cannot verify isolation if unsafe is permitted anywhere |
| **F2** | `unsafe` in kernel/HAL MUST be documented with `// SAFETY:` explaining the invariant | Undocumented unsafe = unaudited TCB hole |
| **F3** | Async IPC/Grant paths MUST use owned buffers (`Box<[u8]>`) not borrows | Rust lifetime rules do not cover borrows across `.await` yield points in SAS |
| **F4** | `static mut` in kernel is only permitted inside `Spinlock<Option<T>>` | Mutable statics are "ambient authority" (Duffy 2013); raw `static mut` = type safety bypass in kernel code |
| **F5** | rustc MUST be pinned to a specific nightly in `rust-toolchain.toml` | Unpinned toolchain → silent ABI change → TCB integrity unknown |
| **F6** | Any soundness hole in rustc is a P0 incident; see §5.5 protocol | LBI guarantee collapses completely if the compiler is unsound |
| **F7** | RISC-V safety claims require Ferrocene qualification first | Currently unqualified target; do not market G1 RISC-V as safety-certified |

---

## 7. Comparison with Prior Art

| System | Isolation Mechanism | Compiler TCB | Notes |
|---|---|---|---|
| **Cellos** | Rust type system (LBI) | `rustc` (open source) | No GC; RAII = deterministic; Ferrocene qualification path exists |
| **Singularity** | Spec# / Sing# type system (LBI) | Bartok (closed source) | ~<5% LBI overhead confirmed; exchange heap IPC ~1,200 cycles; cancelled 2012 |
| **Midori** | C# type system (LBI) | Bartok/Midori (closed) | GC pauses caused unsolvable RT problems; cancelled 2015 |
| **Theseus OS** | Rust type system (LBI) | `rustc` | Academic; no preemption; not production-ready; Cellos is independent parallel work |
| **seL4** | Hardware MMU (formal proof) | None (no LBI) | IPC ~300–400 cycles; formal proof covers kernel only; drivers in ring-3; Cellos has cheaper IPC |

**Key differentiator**: Cellos is the only production-targeting LBI OS using `rustc`,
giving it both the <5% overhead of LBI *and* a qualification path (Ferrocene) that
Singularity/Midori never had.

---

## 8. Cross-References

| Topic | Document |
|---|---|
| Kernel boundary — what goes in the kernel vs. Cells | `docs/specs/15-kernel-boundary.md` |
| LBI cost numbers (Singularity benchmarks) | `docs/specs/00-context.md §5` · `docs/research/research-singularity-midori.md` |
| Hardware security layers (MTE, MPK, CET) — complement to LBI | `docs/specs/` *(layer2-hw-security — to be extracted)* |
| Async owned-buffer invariant (Law 2) | `docs/specs/03-runtime.md` |
| Capability model (CapSet, manifest, grant) | `docs/specs/01-core.md` |
| Security model overview | `docs/security-model.md` |
