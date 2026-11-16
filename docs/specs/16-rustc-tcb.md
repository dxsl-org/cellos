# Spec 16 — rustc as Cellos Trusted Computing Base

> **Status**: Ratified 2026-06-29; amended 2026-08-01 by the approved D12 architectural review;
> amended 2026-11-16 (Phase 18) — the TCB includes the Tier-1 dependency closure, which the
> signing gate now ratchets, and the tier-class gate.
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
| CPU privilege model | Tier-1 dependency closure: `libs/ostd`, `libs/wire`, the vendored `virtio-drivers`, `virtio-pci-transport`, `libs/nic-driver` and the rest of `scripts/tier1-closure.generated.toml`, including `libs/abi` + `libs/types` (stable ABI) |

`rustc` becomes load-bearing because every Cell isolation invariant is enforced by the
compiler, not hardware. If `rustc` is wrong, the whole Cell boundary model collapses.
The same holds for the `unsafe` code every Tier-1 Cell links: `rustc` checks the Cell,
not the libraries under it, so their `unsafe` is assumed correct and belongs to the TCB.

---

## 2. What rustc Guarantees (and Why Cellos Depends on Each)

### 2.1 `#![forbid(unsafe_code)]` enforcement

Every Cell crate carries `#![forbid(unsafe_code)]`, outside the reviewed allowlist. This
is a **compile-time hard gate for the Cell crate only**: rustc refuses to compile `unsafe`
written in that crate. `forbid` is per crate, so it says nothing about the Cell's
dependencies, and `unsafe` expanded from a macro defined in another crate
(`ostd::declare_custom_heap!`, `ostd::cell_main!`'s `#[no_mangle]`) compiles inside a
forbidding Cell. The dependency closure is covered instead by the signing gate's closure
ratchet (§6, F1b): every in-tree crate that `cargo metadata --all-features` resolves as a
normal dependency of a Tier-1 Cell workspace member is held to per-file ceilings, and
third-party crates and proc-macros are pinned by name, version and source.

**Why load-bearing**: without this guarantee, any Cell author could write a raw pointer
cast and break out of the SAS boundary undetected. The kernel does not inspect Cell code
at load time: it verifies the Ed25519 signature and reads the signed manifest class
(`kernel/src/loader/governed_spawn.rs`), and trusts the signing pipeline (F1, F1b, F1c,
F5) for the rest.

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

**Why load-bearing**: IPC between Cells passes typed values (via `wire::ipc::encode` /
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
timing or speculative side channels through shared microarchitectural state. ARM MTE is
probabilistic spatial-memory hardening and x86 MPK is architectural page-access control;
neither is a Spectre/Meltdown mitigation.

These side channels remain a separately scoped threat-model and mitigation problem. No
cache partitioning, predictor isolation, fencing, or compiler mitigation is considered
implemented without independent design and verification. See Spec 19 for the hardware
isolation layers; those layers must not be read as a side-channel guarantee.

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
| Cellos kernel | SAS allocator, preemptive scheduler, IPC, ELF loader, CapSet, manifest check | [generated nLOC](../code-metrics.generated.md) | Only privileged code; strict boundary law per Spec 15 |
| `libs/abi` | Frozen ABI — syscall numbers, capability types (IPC encoding lives in `libs/wire`) | ~2K | Law 1: any change requires 2× user confirmation. No `unsafe`; its manifest and syscall-allowlist macros emit `#[link_section]`/`#[export_name]` into every Cell, ceilinged in the closure baseline |
| `libs/types` | Primitive types shared by kernel and Cells (`VAddr`, `PAddr`, `ViError`) | ~800 | Law 1: same change protocol. `#![forbid(unsafe_code)]` via `[lints]` |
| `libs/ostd` | Cell runtime linked by almost every Tier-1 Cell: raw syscalls, heap, executor, grants, MMIO/DMA helpers, `cell_main!`/`declare_custom_heap!` | — | Holds most of the in-tree closure `unsafe`, `syscall.rs` above all |
| `libs/wire`, `libs/nic-driver`, `libs/virtio-pci-transport`, vendored `kernel/third_party/virtio_drivers`, `libs/cellos-fs`, `libs/tensor-math` | IPC encoding; shared NIC DMA ring layout; virtio transports for the virtio driver Cells; filesystem; NEON kernels | — | The other in-tree closure crates with `unsafe` or unsafe attributes |
| Third-party crates linked into Tier-1 Cells | `memchr`, `hashbrown`, `lock_api`, `heapless`, `spin`, … | — | Pinned by name, version and full source (a git dependency's revision included) with their counts in `scripts/tier1-closure.generated.toml`, and the third-party proc-macros that write code into them pinned alike; any new crate, version, revision or count is a reviewed diff |

The in-tree closure is the generated `scripts/tier1-closure.generated.toml` (`[[package]]`),
not this table. Every file of it that uses `unsafe` or an unsafe attribute has a ceiling in
`scripts/tcb-unsafe-baseline.toml`; `python3 scripts/check-tcb-closure.py` prints the
current per-crate tally. Do not restate the tally here (Spec 21 §2, Layer 3).

**Outside the TCB by design:**

- Cell code outside the reviewed allowlist (`#![forbid(unsafe_code)]`) — verified *by* the
  TCB, not *part of* it. Allowlisted `unsafe` in Tier-1 Cells (driver MMIO/DMA, the
  hypervisor service, test witnesses, the signed tier exceptions) is a reviewed hole, counted
  in the Tier-1 total of the closure baseline.
- C/C++/Zig code runs in Tier 2 (manifest class FFI or UNTRUSTED, Spec 18): the tier-class
  gate (F1c) refuses to sign a crate with a `c-ffi` or `legacy-raw-ptr` allowlist entry
  whose source, or whose binary being signed, carries a Tier-1 manifest, unless a signed
  `[[tier_exception]]` in `scripts/unsafe-allowlist.toml` names it.
- Lua/VM guests — sandboxed by interpreter manifest restrictions or hypervisor Stage-2 paging.
- `core` / `alloc` internals — Cells link them directly; they are outside the scanned
  closure and trusted with `rustc` itself.

---

## 5. Risk Mitigations for rustc-as-TCB

### 5.1 Open Source Auditability

rustc is fully open source. Any Cellos developer can audit compiler behaviour. This is
strictly better than Singularity's Bartok compiler (closed-source, auditable only by MSFT).

### 5.2 Ferrocene qualification lane

[Ferrocene](https://ferrocene.dev) is a safety-qualified toolchain derived from rustc. It
provides toolchain qualification evidence for defined targets, host/build conditions, and
a qualified subset. That evidence does not certify Cellos, its dependencies, generated
binary, or end product.

**Lane split (verified 2026-08-01 against current Ferrocene documentation):**
- RV64 remains the Cellos reference/development and QEMU CI lane.
- `aarch64-unknown-none` is the first bare-metal safety-qualification candidate, subject
  to Ferrocene's documented host/build conditions and Cellos product evidence.
- Bare-metal RV64 is not a qualified Ferrocene target. RV64GC Linux support is not the
  same target or evidence as Cellos bare metal.

> Toolchain qualification is necessary but not sufficient for a product safety claim.
> Do not extrapolate Ferrocene evidence to all Rust crates, `alloc`, or the Cellos image.

**Adoption point**: before the first ARM64 safety-qualified product lane. Keep RV64 as
the primary reference build independently; do not wait for a speculative qualification ETA.

### 5.3 miri: MIR Interpreter for Unsoundness Detection

`miri` interprets Rust's Mid-level IR and detects undefined behaviour — including
unsoundness in `unsafe` code — that rustc's static analysis would miss. Run `miri`
in CI on:
- All kernel `unsafe` blocks (layout assumptions, raw pointer arithmetic, interrupt handlers)
- The Tier-1 closure's boundary code: `libs/ostd` (raw syscalls in `syscall.rs`, heap,
  executor, grants) — `libs/types` and `libs/abi` hold no `unsafe`

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
| **F1** | Every Cell crate MUST carry `#![forbid(unsafe_code)]`, **absolute outside a reviewed allowlist** (`scripts/unsafe-allowlist.toml`). Enforced at the signing gate by `scripts/cellos-sign --check`, which refuses to sign and fails CI on any `unsafe` outside the list. Each entry carries `class`/`reason`/`approver`/`date`, and is reported once past `review_by` (or `max_age_days`), so a temporary exemption cannot become permanent by silence. | Compiler cannot verify isolation if unsafe is permitted anywhere. Driver Cells need `unsafe` for MMIO/DMA, so the allowlist is unavoidable — but it **is** the hole in the LBI wall, and every entry must be reviewed as the unsafe itself would be. Review the current tally with `cellos-sign --check`; do not restate it in prose (Spec 21 §2, Layer 3). |
| **F1b** | The `unsafe` linked into Tier-1 Cells from outside `cells/` MUST stay within reviewed ceilings. `scripts/gen-tier1-closure.py` derives the Tier-1 dependency closure from `cargo metadata --all-features` on the three bare-metal targets (`x86_64-unknown-none`, `riscv64gc-unknown-none-elf`, `aarch64-unknown-none-softfloat`): Tier-1 Cell workspace members as roots, normal edges only. Proc-macros end the walk; a third-party one is pinned, an in-tree one is a closure crate whose files are ceilinged. The result is `scripts/tier1-closure.generated.toml`; CI regenerates it and fails on drift, and the generator refuses a `cells/` crate reached from a Tier-1 root that reads as Tier 2, and a closure crate with a build script whose `include!` takes a computed (non-literal) path while its build script or any build-dependency, in-tree or third-party, spells a counted token — unless `REVIEWED_GENERATED_INCLUDES` records a review pinned to that build script's sha256. `cellos-sign` holds every git-tracked file of an in-tree closure crate (except top-level `tests/`, `benches/`, `examples/` and `build.rs`) to two ceilings in `scripts/tcb-unsafe-baseline.toml` — `unsafe` tokens and unsafe attributes (`#[no_mangle]`, `#[export_name]`, `#[link_section]`, `global_asm!`) — and the Tier-1 total (both counts over the Tier-1 Cells' own sources plus the closure) to one more. A rise fails every run, signing included. A drop, a stale or missing Tier-1 root and an unused tier exception fail `--check --strict` (CI) and are NOTEs on `--sign`; `cellos-sign --write-baseline` lowers ceilings, never raises one. A new Cell crate therefore signs at once, with its own files in the Tier-1 total, but an in-tree crate only it links is unceilinged until CI forces the generated file to be regenerated. Third-party crates are pinned by name, version and full source with their counts; a new crate, a version or revision bump or a changed count fails CI until the generated file is reviewed and committed. Counts are textual: `cfg` is not evaluated, and only `.rs` files on disk are seen. | `forbid` is per crate (§2.1): a memory bug in `libs/ostd` or a registry crate defeats language isolation for every Tier-1 Cell. The total lets reviewed code move from Cells into a shared library without loosening anything. |
| **F1c** | A crate whose allowlist entries include class `c-ffi` or `legacy-raw-ptr` MUST declare manifest class FFI or UNTRUSTED (Tier 2), unless a signed `[[tier_exception]]` (crate, class, accepted manifest class, reason, approver, `review_by`) names it. `cellos-sign` checks this twice. From source (every run), failing closed: any spelling other than the literal `abi::manifest` constant, or a binary root (cargo's targets, from the generated `[[bin]]` rows and the on-disk superset) without a manifest, reads as Tier 1. From the binary (every `--sign`): each target ELF's `__ViCell_manifest` is read as the loader reads it, and a target that loads in Tier 1 and is named after a binary of such a crate is refused unless the exception names the class the binary carries; a malformed manifest is refused. Residual (documented, owner 2026-11-16): a target is attributed by file name only, so one whose name is no `cells/` binary (a renamed copy such as `init`) is only reported, and a renamed copy of a gated crate's binary would be signed on the source reading alone. Package names under `cells/` must be unique, a file with an `extern` block must be classed `c-ffi` (or a test class), and allowlist classes are restricted to six known values. | The kernel picks the tier from the signed manifest byte. C-ABI and raw-pointer `unsafe` is exactly what the type system cannot vouch for, so it belongs behind a page-table wall; an exception keeps that decision visible and dated. |
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
| **seL4** | Hardware MMU (formal proof) | None (no LBI) | IPC ~300–400 cycles; formal proof covers kernel only; drivers in ring-3; Cellos has no measured direct-path comparison yet |

**Key differentiator**: Cellos is the only production-targeting LBI OS using `rustc`,
giving it both the <5% overhead of LBI *and* a qualification path (Ferrocene) that
Singularity/Midori never had.

---

## 8. Cross-References

| Topic | Document |
|---|---|
| Kernel boundary — what goes in the kernel vs. Cells | `docs/specs/15-kernel-boundary.md` |
| LBI cost numbers (Singularity benchmarks) | `docs/specs/00-context.md §5` · `docs/research/research-singularity-midori.md` |
| Hardware security layers (W^X, MPK, domain tables) — complement to LBI | `docs/specs/19-hardware-isolation-layers.md` |
| Cell trust tiers — what the signature attests, tier admission | `docs/specs/18-cell-trust-tiers.md` |
| Async owned-buffer invariant (Law 2) | `docs/specs/03-runtime.md` |
| Capability model (CapSet, manifest, grant) | `docs/specs/01-core.md` |
| Security model overview | `docs/security-model.md` |
