# Cellos Code Standards

**Scope**: Rust code across kernel, HAL, libraries, and Cells  
**Edition**: 2021  
**Nightly**: Required for `no_std` bare-metal features  
**Last Updated**: 2026-11-16 (patterns from the architecture remediation plan: arch seam, lock ranks, residents, allowlist classes and tier exceptions, Tier-1 closure baseline, posture; module-file rule corrected)

---
## Solo-First Development and Independent Promotion

- One accountable maintainer MAY own planning, implementation, testing,
  self-review, documentation, and development release work. Different roles do
  not imply different people unless a gate explicitly says
  `independent member`.
- AI agents, local subagents, and CI jobs are automated assurance. They MUST
  NOT be represented as independent accountable identities or human approval.
- Missing independent-member approval blocks only the claim or promotion that
  names that gate. It MUST NOT block unrelated implementation, documentation,
  host validation, QEMU validation, or exact-device development work whose own
  technical gates pass.
- An independent-member decision is valid only when a repository member other
  than the accountable maintainer posts an explicit `YES` or `NO` on the
  GitHub issue or pull request containing the exact proposal, commit, and
  evidence references. Silence, reactions, aliases, email, chat, AI output,
  and CI results do not count.
- The request MUST state one binary question and bind the exact proposal,
  commit SHA, and evidence URLs. The member's comment MUST contain
  `DECISION: YES` or `DECISION: NO`. Any material change to those bound inputs
  invalidates the decision and requires a new GitHub response.
- External standards, vendor contracts, or production controls that explicitly
  require more parties remain external-gated. Solo development never promotes
  host, QEMU, or development-hardware evidence to production.
- See
  [ADR-0013](decisions/0013-solo-first-development-independent-promotion.md).

---

## The 8 Coding Laws (Non-Negotiable)

### Law 1: Interface is Sacred

- **Scope**: `libs/abi/` (frozen kernel ABI) and `libs/types/`; service contracts in `libs/wire/` are extensible without it
- **Rule**: Any change requires two explicit confirmations from the accountable
  maintainer at separate checkpoints: design approval before editing, then
  implementation approval after reviewing the exact ABI delta and evidence.
  This is a two-step owner check, not a two-person approval.
- **Reason**: These define the stable ABI between kernel and Cells
- **Implementation**:
  - Use `#[repr(C)]` on public structs/enums that cross the ABI boundary
  - Keep public trait method signatures additive and document wire/layout assumptions explicitly
  - Document trait contract in doc comments
  - Preserve method signatures when extending traits

#### Security-Sensitive KMS ABI

- The fixed-frame KMS v1 wire contract is append-only. Never renumber, remove,
  or reinterpret an existing opcode, error, field, or encoding; add a distinct
  purpose-specific operation and preserve canonical zeroed reserved bytes.
- A signing operation must accept typed protocol state only. Do not expose a
  generic prehash/raw-message signer, caller-selected key or algorithm, or
  private-key material.
- Authorize the live caller generation and registered service TID before any
  provider access. Replay identifiers must be nonzero and monotonic, and replay
  state advances only after a signature has passed all checks.
- Keep one protected-root provider boundary with independent, typed capability
  leaves. C2C X25519 readiness, generation, assessment, rotation, or failure
  must never authorize or substitute for Relay P-256 state, or vice versa.
- KMS must reconstruct the purpose-specific signing input, validate provider
  output, normalize ECDSA output to low-S, and self-verify before returning it.
  Provider failure never falls back to another key, provider, or weaker path.

#### KMS SSH Custody Boundary

- KMS holds at most one ed25519 SSH identity per store (Law 1: no caller-selected
  key or algorithm). A signing request carries the session id, the username and
  the expected 32-byte public key (a confirmation that must equal the stored
  key, never a selector); KMS reconstructs the RFC 4252 userauth input and never
  accepts a raw message, prehash or algorithm choice.
- SSH opcodes 15-24 are append-only; opcodes 1-14 and errors 1-26 never change.
  Import (admin class), unlock and sign (login class) are separate caller
  classes; the SSH sign right is a kernel-attested caller flag, not a registered
  service TID, and an admin caller is never `cellbox-ssh`.
- A passphrase-wrapped identity is unlocked per login: one staged secret
  authorizes exactly one signature and is zeroized afterwards. The only accepted
  encrypted container is `openssh-key-v1` with bcrypt (rounds 1..64, salt
  16..64 bytes) and AES-256-CTR, checked before any KDF work.
- KMS SSH custody (KSSH K5) is behind the opt-in `ssh-dev-custody` feature; a
  default build answers every SSH opcode `SshKeyUnavailable` after
  attested-caller validation, which is a status, not a feature. T0 holds the
  identity in RAM (`DevPostureVolatile`). The opt-in `ssh-dev-persist` feature
  (T1, x86_64 bare metal only, implies `ssh-dev-custody`) persists only
  passphrase-wrapped identities, as MAC-framed records in
  `/srv/cellos/kms/ssh-{a,b}.bin`; a `Plain` identity is never written. SSH
  custody is `DEV_POSTURE`, never reported as production-qualified, and T1 makes
  no rollback-protection and no crash or power-loss durability claim (its dev
  root MAC key is a public constant).

#### Development Silo Provider Boundary

- Silo is not a public SDK or general cryptographic API. Do not add direct
  connect, key initialization, generic digest/message signing, ECDH,
  caller-selected command, or raw-opcode surfaces. The only implemented guest
  signing purpose is the KMS-mediated TLS 1.3 client `CertificateVerify`
  operation.
- `development-silo-provider` is `DEV_REFERENCE`, AArch64-QEMU-only, and
  non-production. It must remain incompatible with production builds and must
  report `production_capable=false`. Stage-2 under the same Cellos EL2 host is
  software containment evidence, not independent hardware custody.
- Package the standalone guest through its locked build, then reject empty,
  oversized, or digest-mismatched bytes before VM creation. The VM must execute
  exactly the admitted bytes.
- Publish Silo readiness only after artifact admission, VM load, one-time
  development initialization, guest readiness, and public-metadata validation.
  The registration authority must remain `test-hooks`-only, exact to the
  governed `/bin/silo` root task and `service::SILO`, non-manifestable, and
  non-delegable.
- Authenticate the live KMS instance before decoding the private command.
  Guest/protocol/VMM faults, malformed or stale responses, and reset permanently
  fail the current instance closed. Never retry within the instance or fall
  back to an in-process key.
- Fatal VM-exit diagnostics must redact arbitrary guest registers. Preserve HVC
  x0/x1 only when x0 is a recognized private Silo function identifier; unknown
  HVC values retain only non-register metadata such as the instruction immediate.

#### Relay Certificate Provisioning Boundary

- Keep opcodes 9–14 purpose-specific and append-only. Opcode 14 exposes only
  the active generation's public key; never reinterpret it as pending-key
  proof. Any future authenticated precommit binding requires a distinct
  reviewed contract.
- Enrollment begin and ordered CSR reads require the exact live supervisor;
  profile staging and active-key inspection require the exact live
  service-net instance. Authorize before provider access or lifecycle mutation.
- Build the frozen bounded CSR inside KMS. The provider must create a fresh
  non-exportable key, independently reconstruct and sign the canonical
  `CertificationRequestInfo`, and return only public proof. KMS must validate
  the point, normalize to low-S, self-verify, and publish the CSR only through
  its one-shot restart-bound ordered handle.
- Permit one pending generation and enforce
  `Prepared -> CsrIssued -> Staged -> committed`. A stale, foreign, repeated,
  or out-of-order handle use must invalidate the flow. Do not commit until all
  CSR chunks are consumed and the exact generation, policy epoch, and profile
  digest are staged.
- Every abort and post-creation failure must destroy the provider's pending key
  or retain a cleanup tombstone until deletion/absence is confirmed. Provider
  promotion, lifecycle activation, and persistence failure must never leave a
  mixed tuple able to serve; seal on disagreement.
- Persist only committed lifecycle facts and protected monotonic floors.
  Missing, torn, unavailable, or regressed protected persistence or
  authenticated time must seal enrollment and serving; volatile state is never
  a substitute.
- Relay manifests must use an explicit allowlist, canonical bounded mount
  paths, and no client private-key field. Mounted certificate chains must obey
  the frozen count/size and strict DER bounds, require clientAuth without
  serverAuth, and bind the active leaf SPKI to opcode 14 plus both KMS and
  manifest NodeId values. This active-key check must not authorize pending
  profile staging.

#### Production Protected-Root Evidence

- Never infer an exact product identity, configuration, capability, lifecycle,
  support contract, or production suitability from a generic architecture,
  reference design, masked family, design release, development board, emulator,
  or deployment announcement.
- Classify a required product-specific fact as `UNVERIFIED` when exact
  vendor-backed evidence is absent. Absence is never a pass and must not be
  filled by architectural similarity or generic design capability.

#### Boundary-specific ABI records

- A record that crosses a real trust or update boundary needs a written
  contract. Such boundaries are serialized records, guest/native ABI, persisted
  metadata, and independently built or updated components. The contract states
  identity/type, version policy, an exact or explicitly bounded length,
  reserved/unknown-bit handling, ownership, and validation before any effect.
  An existing typed envelope or exact-length contract may cover a field. The
  census and template are in [ABI boundary records](abi-boundary-records.md).
- Same-build typed Rust calls, including Tier-1 SAS method calls, get no magic,
  version or size header and no per-call wire decode. They still need their
  bounded holder/rights/live-lineage checks and a snapshot of shared metadata
  at admission.
- New decoders refuse non-zero reserved fields, undefined flag bits, and
  trailing bytes the record does not explain. They refuse an unknown version
  but keep accepting every version the policy still supports.
- Feature bits describe real negotiated behavior, never a test stage or a
  capability grant.
- Malformed-record evidence comes from fixtures signed after construction by
  the normal test-key route. Never patch a record after signing and present the
  signature failure as parser evidence. Never weaken the signer.

### Law 2: Owned Buffers for Async (SAS Safety)

**Forbidden**:
```rust
async fn process(data: &mut [u8]) { }  // ❌ LIFETIME VIOLATION
```

**Required**:
```rust
async fn process(data: Box<[u8]>) -> Box<[u8]> { }  // ✅ OWNED
```

**Why**: Single Address Space (SAS) means no process boundaries for cleanup. Owned buffers ensure deterministic drop semantics across async boundaries.

#### Grant Lookup and Lease Linearization

- Any operation that returns a Grant mapping whose use is protected by a VFS
  lease must resolve the PAGE or REG entry and publish the exact lease while
  still holding that matching grant-table lock. Lookup, copied fields, and a
  later lease publication are not equivalent and create a free/reuse TOCTOU.
- Validate every fallible output before publishing the lease. After
  publication, returning the mapping and its registered length must be
  infallible so a failed lookup cannot strand a pin.
- `GrantFree` and `GrantUnregister` must check the pin registry and remove the
  entry in the same grant-table critical section. A live lease refuses teardown;
  owner death quarantines frames until the exact holder/request release.
- VFS must copy grant bytes through the safe bounded OSTD adapter. The copy is
  capped by the destination and registered grant lengths, permits overlap, and
  must match the requested byte count before backend mutation or commit
  acknowledgement. Do not construct a service-local raw slice for this path.

**Pattern**:
- Input: `Box<[u8]>` or `Vec<u8>` (caller owns until call)
- Output: `Box<[u8]>` or `Vec<u8>` (callee owns return)
- Channels: `mpsc::Sender<Box<[u8]>>` for zero-copy IPC

### Law 3: Multi-Architecture Awareness

**Forbidden**:
```rust
let addr: u64 = 0xFFFF_FFFF_8000_0000;  // ❌ ASSUMES 64-BIT
```

**Required**:
```rust
let addr = VAddr(0x8000_0000);  // ✅ ARCH-AGNOSTIC
```

**Rules**:
- Never hardcode pointer sizes (`usize`, `u64`)
- Always use `VAddr` for virtual addresses, `PAddr` for physical
- The bootable targets are `x86_64-unknown-none`, `riscv64gc-unknown-none-elf` and
  `aarch64-unknown-none-softfloat`; a shared kernel change compiles on all three. The 32-bit
  targets are quarantined in `kernel/src/arch/legacy/` and do not build
  ([per-architecture status](generated/arch-status.md)).
- **New architecture code goes under `kernel/src/arch/<arch>/`** (2026-11-14, remediation
  Phase 14). Each arch module exports the same function sets, and shared code calls
  `crate::arch::<set>::<fn>()` without naming an architecture (`kernel/src/arch/mod.rs:1-16`).
  Existing `target_arch` lines outside it are capped, not gone: `scripts/check-hal-boundaries.sh`
  (run by `scripts/check-board-configs.sh`) fails above 692 such lines or any `asm!` outside
  `kernel/src/arch/` (`check-hal-boundaries.sh:64-80`). The ceilings only go down: lower them in
  the commit that moves code, never raise them. Do not add
  `cfg_attr(not(target_arch = …), allow(dead_code))` to hide per-arch code.
- Read board data through `board::active()` only.

### Hardware Platform Ownership

- Root `boards/` packages contain only identity/compatibles, boot/firmware
  contract, pinmux/PHY wiring, fallback memory/DT assets, typed SoC identity,
  and enabled shared-driver selection.
- Immutable SoC MMIO, IRQ topology, controller presence, and access quirks live
  under `hal/soc/`.
- Shared HAL↔kernel Rust ABI declarations live in `hal/traits/arch/src/kernel_abi.rs`.
  Architecture code must import that shared module instead of re-declaring
  local `extern "Rust"` blocks, and declaration sites should keep compile-time
  assertions close to the hook they validate.
- For x86 PC-compatible targets, `hal/soc/x86` owns static port/ISA wiring and
  bounded legacy firmware windows. LAPIC, IOAPIC, HPET, and ECAM addresses must
  come from validated ACPI and remain unavailable when firmware evidence fails.
- Register access, interrupt programming, and CPU-architecture mechanisms live
  under `hal/arch/`; shared device mechanisms remain single-copy in the kernel
  integration layer or `cells/drivers/`.
- Cargo board features select integration data. They must not fork UART, SDHCI,
  DesignWare I2C/SPI, GIC/PLIC, VirtIO, or PCIe mechanisms.
- Run `bash scripts/check-board-configs.sh` after changing a board, SoC profile,
  driver-selection boundary, build feature, or any `target_arch` line in the kernel.

### Law 4: Unsafe Code Management

**Ordinary Rust Cells**:
```rust
#![forbid(unsafe_code)]  // ABSOLUTE
```

Driver, runtime, shim, and FFI cells may carry reviewed unsafe only when listed
in the unsafe allowlist / signing policy checks. Do not describe Cellos as
"every Cell forbids unsafe" without that exception.

**Allowlist and Tier-1 perimeter** (2026-11-16, remediation Phase 18; enforced by
`scripts/cellos-sign --check --strict` in CI and before every `--sign`):

- Every `scripts/unsafe-allowlist.toml` entry carries `reason`, `approver`, `date`,
  `review_by` and one of six classes: `driver-mmio`, `c-ffi`, `legacy-raw-ptr`, `hypervisor`,
  `test-harness`, `test-violation` (`scripts/cellos_sign/allowlist.py:37-47`). A stale entry
  stops signing, so re-review entries before their `review_by`.
- A file with an `extern` block is classed `c-ffi`, `test-harness` or `test-violation`.
- A crate with a `c-ffi` or `legacy-raw-ptr` entry must declare manifest class FFI or UNTRUSTED
  (Tier 2). Keeping it in Tier 1 needs a `[[tier_exception]]` naming crate, class and manifest,
  approved by the owner, with an exit condition (`scripts/unsafe-allowlist.toml:1054-1082`).
  The signer reads the class from source and again from the built ELF's manifest.
- Code linked into a Tier-1 cell from outside `cells/` (`libs/ostd`, `libs/wire`, vendored
  crates, …; `scripts/tier1-closure.generated.toml`) is held to per-file ceilings on `unsafe`
  tokens and unsafe attributes (`no_mangle`, `export_name`, `link_section`, `global_asm!`) in
  `scripts/tcb-unsafe-baseline.toml`, plus a combined Tier-1 total. A new file with `unsafe` or
  a higher count fails.
- When you remove `unsafe`, run `scripts/cellos-sign --write-baseline` in the same commit. It
  lowers ceilings and never raises them; a lower count left unrecorded fails CI strict. Raising
  a ceiling or adding a row is a reviewed change, like an allowlist entry.
- A dependency change that alters the Tier-1 closure needs
  `python3 scripts/gen-tier1-closure.py` (CI runs `--check`).

**Kernel & HAL**:
- Unsafe only for hardware I/O (CSRs, MMIO)
- **Every `unsafe` block must have a `// SAFETY:` comment** explaining:
  - Why safety invariants are maintained
  - What preconditions the caller must satisfy
  - What could go wrong if misused

**Example**:
```rust
// SAFETY: We assume mmu is initialized and this vaddr is mapped in current page table.
// CSR access is safe: no concurrent hart touches mepc during boot.
unsafe { riscv::register::mepc::write(func as usize); }
```

#### Security-Sensitive Raw Output Buffers

- An allowlist bit proves only that a caller may request an operation. It does
  not prove pointer provenance or authorize the pointed-to range.
- Before constructing a mutable slice or writing any output, syscall code must
  checked-add and cap the length, then prove that the complete range is mapped,
  caller-owned, and writable.
- Null, overflowed, oversized, unmapped, kernel, and peer-cell ranges must be
  rejected before any read or write. Tests for a qualifying boundary must call
  the syscall directly with each hostile class; a typed wrapper alone is not
  evidence.
- Entropy APIs must return admitted real entropy or an observable zero/error
  when unavailable. Predictable `dev-weak-rng` output is permitted only in an
  explicit development profile and is never production, cryptographic, PAL
  support, or promotion evidence.

#### Pinned Rust Toolchain and Feasibility Evidence

- A private Rust `std` integration must use an exact, content-addressed,
  no-fuzz source overlay against the matching pinned compiler checkout. Base
  source, patch, result, target metadata, private sysroot, and linker inputs
  must remain provenance-bound.
- Cellos `std` means an internal `library/std` PAL selected by matching rustc
  target metadata. External PAL plug-ins, target-OS impersonation, `std` over
  mlibc/POSIX, fake/unsupported `std`, and renamed `core` + `alloc` are not
  acceptable substitutes.
- Fixture-only benchmark validators may test schemas, parity, ordering,
  interference rejection, and closed linker inputs. Synthetic fixture reports
  are non-promotional and must never be described as live capture,
  authenticated evidence, human approval, runtime availability, or target
  qualification.
- Feasibility verification does not authorize implementation. A PAL, target,
  sysroot, runtime, target/triple publication, or promotion may be claimed only
  after its named human approvals and implementation/production gates are
  explicitly granted.

### Law 5: Module Structure

**Correction (2026-11-16):** this law used to say `foo/mod.rs` is banned and enforced by a CI
lint. Neither was true: the tree holds 41 tracked first-party `mod.rs` files (outside vendored
`third_party/` code; for example `kernel/src/loader/launch_profile/mod.rs`,
`kernel/src/acpi/mod.rs`), and no CI job, clippy lint or script checks the form.

**Convention**: both forms exist.

```
foo.rs          parallel file + folder (182 first-party pairs; the more common form)
foo/
├── bar.rs
└── baz.rs

foo/mod.rs      also present in the tree
```

**Rules**:
- Declare the module in its parent: `mod foo;`
- Prefer `foo.rs` beside `foo/` for new modules, and match the form already used in the
  directory you edit. Do not convert an existing module between the forms in an unrelated
  change.
- Submodules: `foo/bar.rs`, `foo/baz.rs`
- Use snake_case: `file_system.rs`, not `FileSystem.rs`
- A new kernel file needs a `kernel/residents.toml` row with its Spec 15 category;
  `scripts/check-kernel-residents.py` fails on an unlisted or removed file.

### Law 6: Cellos Naming Convention

| Category | Rule | Examples |
|----------|------|----------|
| **Public Traits** | `Vi` prefix (Vi-something) | `ViFileSystem`, `ViDriver`, `ViBlockDevice`, `ViAsyncFileSystem` |
| **Error Types** | `Vi` prefix | `ViError`, `ViResult<T>` |
| **Core Structs** | `Vi` prefix (or generic) | `ViConfig`, `ViBenchmark` |
| **Address Types** | `V` or `P` prefix | `VAddr`, `PAddr` |
| **Filesystem Names** | retired `viFS1` / `viFS2`; active boot FS `VIFS1` | `VIFS1` (kernel BootFS/initramfs) |
| **Modules/Files** | snake_case | `task.rs`, `memory.rs`, `frame_allocator.rs` |
| **Functions** | snake_case | `init_paging`, `handle_interrupt` |
| **Constants** | UPPER_SNAKE | `MAX_CELLS`, `KERNEL_HEAP_SIZE` |
| **Type Params** | PascalCase | `T`, `E`, `CellState` |
| **Library crates (new)** | `vi` prefix, lowercase, one word where possible; module paths for parts | `viui`, `vitui` (`vitui::backend`, `vitui::session`, `vitui::event`, `vitui::theme`), `vimio`, `vipolling` |

**Library naming rule (decided 2026-10-23):**
- Cellos library crates use the `vi` prefix, in the same family as `viui`.
- The `*-cellos` suffix (e.g. the former `tui-cellos`, `mio-cellos`, `polling-cellos`) is **retired**. Adapters of upstream crates are also named `vi*` (`vimio` adapts the mio API, `vipolling` the polling API, `vitui` Ratatui).
- **Do not repeat** `cellos` or `vi` inside a name: no `vitui-cellos`, `cellos-vitui` or `vi-vi*`.
- A framework's parts are **modules of one crate** (`vitui::session`), not sibling crates, unless a dependency boundary requires a split. Example: a part that must not pull Ratatui into consumers that don't need it.
- Existing non-`vi` crates (`abi`, `wire`, `ostd`, `types`, `terminal-sdk`, `cellos-fs`, `cellos-boot-config`, …) keep their names until renamed by an explicit decision. New crates must follow this rule.

### Law 7: Trait Objects for Polymorphism

**Pattern**:
```rust
pub fn register_driver(driver: Arc<dyn ViDriver + Send + Sync>) { }
```

**Rules**:
- Use `dyn Trait` at system boundaries (Cells, drivers, services)
- Always specify bounds: `Send + Sync` for multi-cell safety
- `Box<dyn T>` for single owner (Cell)
- `Arc<dyn T>` for shared resources (kernel registry)
- Implement `Drop` for cleanup (Law 8)

**Why**: Enables dynamic Cell loading without recompilation.

**Filesystem naming note**: use `VIFS1` only for the kernel BootFS/initramfs. Keep
`viFS1` / `viFS2` only in historical references to retired designs.

### Law 8: RAII - Implement Drop

**Rule**: All resources must implement `Drop` for explicit cleanup.

**Pattern**:
```rust
pub struct FileHandle { fd: u32 }

impl Drop for FileHandle {
    fn drop(&mut self) {
        // Close file, release resource
        syscall::close(self.fd).ok();
    }
}
```

**Why**: In SAS, there's no process cleanup. Resources don't auto-free when a task dies. You must manually manage.

**Resources Requiring Drop**:
- `FileHandle`, `DirHandle` — system resources
- `GrantEntry`, `Lease` — capability objects
- `Lock<T>` — mutual exclusion
- Custom allocations — via `alloc` crate

---

## Error Handling

### Result Pattern (Not Panic)

```rust
pub type ViResult<T> = Result<T, ViError>;
```

**Rule**: Use `Result<T, E>` everywhere except kernel invariants.

**ViError Variants**:
```rust
pub enum ViError {
    OutOfMemory,
    InvalidArgument,
    NotFound,
    PermissionDenied,
    AlreadyExists,
    WouldBlock,
    NotSupported,
    IO(String),
    InvalidInput,
    IsADirectory,
    NotADirectory,
    Unknown,
}
```

**Syscall Wrapper Example**:
```rust
pub fn open(path: &str, flags: u32) -> ViResult<FileHandle> {
    let fd = unsafe { syscall(SysCall::Open, path, flags)? };
    Ok(FileHandle { fd })
}
```

---

## Async & Concurrency

### Async Functions

```rust
pub async fn read_file(path: &str) -> ViResult<Vec<u8>> {
    let file = open(path, READ).await?;
    file.read_all().await
}
```

**Rules**:
- Use `async/await` syntax (not `Future` trait directly)
- Owned buffers: `Box<[u8]>`, never `&mut [u8]`
- Spawn tasks with kernel executor: `spawn_async(future)`

### Spinlocks for Synchronization

```rust
static REGISTRY: Spinlock<Registry> =
    Spinlock::ranked(Registry::new(), crate::sync::rank::SERVICE_REGISTRY);

fn register(id: CellId, cell: Cell) {
    let mut map = REGISTRY.lock();
    map.insert(id, cell);
}
```

**Why**: Spinlock handles interrupt safety automatically (disables on lock, re-enables on drop).

**Lock ranks** (2026-11-11, remediation Phase 10). Every kernel `static Spinlock` is built with
`Spinlock::ranked` and a rank from the `lock_ranks!` table in `kernel/src/sync/rank.rs`. A hart
may only take a ranked lock whose rank is greater than every ranked lock it holds, so the table
order is the lock order. Instance locks inside per-object structs stay unranked
(`Spinlock::new`).

- Test-hooks kernels and host tests panic on an out-of-order acquisition, naming both locks;
  dev and release kernels carry no check.
- A new static lock needs a rank in that table and an entry in `scripts/mutable-globals.toml`.
  `tools/mutable-globals --check` (CI) fails on a static spinlock without a rank, an unknown
  rank, or a stale [generated rank table](generated/lock-ranks.md).
- Do not justify an order with a comment; the rank is the contract.

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_allocation() {
        let alloc = FrameAllocator::new();
        let frame = alloc.allocate().expect("should allocate");
        assert!(frame.0 > 0);
    }
}
```

**Rules**:
- Test critical logic (allocators, scheduler, IPC)
- Use `expect()` with clear messages, not `unwrap()`
- No integration tests in kernel (use architecture tests in `tests/`)
- Self-test and fixture code compiled into the kernel stays behind `test-hooks` and is
  categorised `test` in `kernel/residents.toml`; a release kernel carries no self-test symbol.

### Integration Tests

Located in `tests/architecture-validation/`:
```
tests/
├── step1_spec_verification.md
├── step2_dependency_analysis.md
└── (20+ checks)
```

**Run**:
```bash
cargo test --test '*' --release
```

### Definition of Done (runtime evidence, not checkboxes)

> Ratified 2026-07-06 after the functional audit (a "23/23 complete" claim
> re-scored to 12 done + 6 partial) and the boot.rs false-green incidents.

A feature, phase, or fix is **done** only when ALL of:

1. **Builds clean** on every architecture it targets (`cargo build --release`,
   the arch feature matrix in CI).
2. **Runs on QEMU with observable evidence**: a boot-log line, an integration
   test, or a shell interaction proving the behavior — captured in the plan or
   commit message. `cargo check` passing is NOT evidence of anything running.
3. **Its integration test is wired into CI** (the `boot-suite` job or a
   dedicated job). A test that only runs on one developer's machine rots —
   the main suite once rotted 4 days because it lived outside CI.
4. **Fails loud, never silent**: no silent-deny, silent-empty-reply, or
   silent-skip paths in the feature OR its tests. `prerequisites_ok()` must
   go through `ci_guard()`; degraded modes must log.
5. **Status text updated in the same commit** (roadmap/plan frontmatter/docs
   body) — stale "✅" markers poison every later planning session.

Plans record status as: 📋 planned → 🔨 code-complete (builds, unverified) →
✅ **verified** (runtime evidence linked). Never mark ✅ from 🔨 without a run.

---

## Comments & Documentation

### Doc Comments (Public Items)

```rust
/// Opens a file from the virtual filesystem.
///
/// # Arguments
/// * `path` - Absolute path (e.g., "/bin/hello")
/// * `flags` - Open flags (READ, WRITE, APPEND)
///
/// # Returns
/// A `FileHandle` or `ViError` if not found or permission denied.
///
/// # Example
/// ```
/// let handle = open("/bin/hello", READ)?;
/// let bytes = handle.read_all().await?;
/// ```
pub async fn open(path: &str, flags: u32) -> ViResult<FileHandle> { }
```

**Rules**:
- Document all public traits, functions, types
- Include # Arguments, # Returns, # Errors sections
- Add examples for complex logic
- Link to related specs: `See docs/specs/03-runtime.md for async safety rules.`

### Safety Comments

```rust
// SAFETY: We guarantee UART is initialized before this point.
// CSR access is atomic: no other hart modifies mepc during boot.
unsafe { riscv::register::mepc::write(func as usize); }
```

**Format**:
```
// SAFETY: [Why it's safe: preconditions, guarantees, no data races]
unsafe { ... }
```

### Inline Comments (Sparse)

Only when WHAT the code does is unclear:

```rust
// Bad:
x = x + 1;  // Increment x

// Good:
// Align heap pointer to next 4KB boundary (page size)
heap_ptr = (heap_ptr + 0xFFF) & !0xFFF;
```

---

## Code Organization

### Imports

```rust
// System imports (std, no_std)
use core::ptr;

// External crates
use spin::Spinlock;
use xmas_elf::ElfFile;

// This crate
use crate::memory::{VAddr, PAddr};
use crate::task::Task;

// Pub re-exports at module level
pub use crate::types::{ViError, ViResult};
```

**Order**: System → External → Internal → Re-exports.

### File Size & Directory Organization

- **Limit**: 200-300 LOC per file
- **Exceeding**: Split into submodules
- **Example**: `task.rs` (1000 LOC) → `task/scheduler.rs`, `task/syscall.rs`, `task/ipc.rs`

### Cells Directory Structure

Cellos organizes cells into 8 semantic groups (parallel to code, not functionality):

```
cells/
├─ tools/        — System utilities (shell, init, sys-tools, net-tools)
├─ apps/         — User applications (choose tier by execution boundary and runtime profile)
├─ demos/        — Demonstrations & graphical showcases (periph-demo, sensor-demo, doom, tetris*, etc.)
├─ drivers/      — Hardware device drivers (trusted native cells; gpio, i2c, spi, uart, etc.)
├─ services/     — System services (vfs, net, input, compositor, silo, hypervisor, etc.)
├─ runtimes/     — Trusted native runtime profiles (lua; MicroPython historical only)
├─ tests/        — Integration & stress test cells (bench, vfs-test, etc.)
└─ guests/       — Hypervisor guests (silo-guest, aarch64-unknown-none)
```

**Classification rules:**
- **tools/** — Always-running infrastructure (shell, init, system daemons)
- **apps/** — Interactive/rich user applications; use the tier/runtime profile docs to pick the execution boundary
- **demos/** — Showcases of system capabilities: hardware drivers, rendering, audio, scripting, games. Run on-demand from the shell; never auto-spawned at boot.
- **drivers/** — Hardware devices + driver Cells (trusted native cells, mapped via kernel Resource Registry or IPC)
- **services/** — Long-lived stateful services with IPC servers (VFS, net, input, compositor, broker-style cells). Cross-machine brokers must fail closed by default; readable configuration is neither authorization nor secret storage.
- **runtimes/** — Trusted native runtime profiles (Lua; MicroPython is historical only)
- **tests/** — Integration test & benchmark cells spawned by CI or manual runs (disposable, single-purpose)
- **guests/** — Hypervisor guest binaries (bare-metal or minimal OS images, non-x86/ARM64 targets)

### HAL / Board Ownership

- Root `boards/` stays outside HAL; it owns immutable board descriptors and fallback assets.
- `hal/soc/riscv` owns RISC-V SoC profile facts only: compatible-string sets and fail-closed access policies.
- `hal/soc/x86` owns PC-compatible COM/ISA wiring and legacy firmware windows;
  ACPI-discovered MMIO stays fail-closed in the kernel integration boundary.
- Shared drivers stay in `cells/drivers/`; do not copy UART, SDHCI, DW I2C/SPI, GIC/PLIC, or PCIe drivers per board.

### Visibility

```rust
// Kernel only
fn internal_fn() { }

// Public to cells (part of syscall ABI)
pub unsafe fn syscall_handler() { }

// Public trait (stable ABI)
#[repr(C)]
pub trait ViFileSystem {
    fn open(&self, path: &str) -> ViResult<Box<dyn ViFile>>;
}
```

---

## Build & Compilation

### Cargo Features

Architecture is selected by the target triple, not by a Cargo feature. Kernel features select
the **posture** (2026-11-10, remediation Phase 04): `kernel/src/posture.rs` derives exactly one of
`dev`, `test` or `release` from the feature set, and `kernel/build.rs` refuses a mixed set at
build time ("kernel feature set refused: …").

- The default features are the dev posture (`kernel/Cargo.toml:81`). `test-hooks` makes the
  test posture. A release kernel is `--no-default-features --features release-posture`.
- Never add a development, test, fixture or measurement feature without listing it in
  `FORBIDDEN_IN_RELEASE` (`kernel/src/posture.rs:18-34`) when it must not reach a release kernel.
- Every bootable 64-bit kernel needs `native-domains`; `compile-only` is for builds that never
  boot.
- Trace and accounting switches are Cargo features (`p01-trace`, `g1-cpu-accounting`), not raw
  `--cfg` flags from the environment.

Production-relay feature selection is deny-by-default. The named production
image checker must accept exactly one approved hardware provider with verified
TLS and reject fixture/Silo/test hooks, development keys or RNG, insecure/raw
relay paths, and K1 fallback. The image builder must not bypass a blocked
checker or turn a missing provider, qualification record, or provenance input
into a development artifact.

**Conditional Code**: per-architecture code goes under `kernel/src/arch/<arch>/` (Law 3), so
shared kernel code calls one function and names no architecture
(`kernel/src/memory/paging.rs:108`):

```rust
// kernel/src/arch/{x86_64,riscv64,aarch64}/tlb.rs each export `flush_all`.
crate::arch::tlb::flush_all();
```

### Cell Signing: Test Key vs Release Key

Test and release are separated by **key and kernel posture, never by file name**
(plan `.agents/261031-1200-test-release-key-separation`, user decision 2026-10-31).

| | Test / alpha images | Release images |
|---|---|---|
| Kernel features | default (`dev-signing-key`) | `--no-default-features --features release-posture,…` |
| Kernel trusts | dev key only (seed `0x43*32`, public) | release key only (`CELLOS_RELEASE_CELL_PUBLIC_KEY`) |
| Who signs | anyone: `scripts/cellos-sign --sign` / `sign_cells`, every cell incl. `cellbox` | CI only (private key is a CI secret; `guard_prod_key`) |
| Unsigned cell | Tier 2 | denied |
| Signed cell | Tier 1, or Tier 2 if the manifest is FFI/UNTRUSTED | same |

Rules:
- Sign every native cell of a test image with the dev key. Do not skip a cell by name,
  invent a private seed or an environment bypass to get it signed.
- Never combine `dev-signing-key` with `release-signing-key` (build.rs refuses), and never
  ship a kernel built with default features as a release.
- A dev-signed artifact is safe only because release kernels do not trust the dev key;
  do not add the dev key to a release kernel "temporarily".
- The two quarantined hashes in `scripts/cellos_sign/signing.py` stay refused for every key.
- Check the boot log line `[signing] cell trust posture=… unsigned=…` before trusting any
  evidence about signing or tiers.

### Compiler Flags

```toml
[profile.release]
panic = "abort"        # No unwinding in kernel
lto = true             # Whole program optimization
opt-level = "z"        # Size + speed tradeoff
```

---

## Common Patterns

### Global State (Kernel)

```rust
// kernel/src/task.rs:487-488
pub(crate) static SCHEDULER: Spinlock<Option<Scheduler>> =
    Spinlock::ranked(None, crate::sync::rank::SCHEDULER);
```

Every writable kernel global needs an entry in `scripts/mutable-globals.toml` (lifetime,
mutation mechanism, owner, contexts, retirement); `tools/mutable-globals --check` runs in CI.

### Capability Object (Syscall)

```rust
pub struct Grant {
    capability: Capability,
    from: CellId,
    to: CellId,
}

impl Drop for Grant {
    fn drop(&mut self) {
        // Revoke capability on drop
    }
}
```

### Async Executor Task

```rust
pub async fn read_with_timeout(path: &str, timeout_ms: u64) -> ViResult<Vec<u8>> {
    select! {
        result = read_file(path) => result,
        _ = timer::sleep(timeout_ms) => Err(ViError::WouldBlock),
    }
}
```

### App Development (Cell Writing)

Use the Cellos App SDK (`libs/ostd/`) to eliminate boilerplate:

The App SDK is one family of named modules/layers, not a numbered tier system.
Application Tier 1/2/3 describes the execution/isolation boundary; runtime
profiles and SDK modules describe how code is built and which APIs it uses.

Manifest v2 uses a protection-class byte for the x86 PKU floor. New Rust code
must use `PROTECTION_CLASS_*`, `CellManifest::protection_class()`, and
`granted_protection_class()`; do not call this byte an application execution
tier. The ABI-stable `tier` field, `TIER_*`, `tier()`, constructors, and existing
manifest macro forms remain compatibility surfaces.

The binary contracts are exact:

- Rust v2 is the 16-byte little-endian record
  `{magic:u32, version=2:u8, tier:u8, flags:u16, cap_args_off=0:u32,
  reserved=0:u32}`. Reserved fields must remain zero.
- Zig v1 is the 8-byte record
  `{magic:u32, version=1:u8, flags:u8, pad=[0;2]}`. The Rust parser
  zero-extends its flags and assigns legacy protection behavior.

The kernel classifies the unique `__ViCell_manifest` section before task
creation as `Absent`, `Valid` (v1 or v2), or `Malformed`. Only structurally
valid absence may use the explicit legacy path policy. Never turn malformed
metadata, duplicate sections, a named `SHT_NOBITS` section, unknown
class/flags, non-zero reserved bytes, or version/length mismatch into absence.

Use `python3 tools/check_elf.py <path>` for strict, read-only inspection. Its
labels have deliberately separate meanings: `Execution tier` is external
application policy, `Runtime profile` is the build/runtime choice, `Protection
class` is the manifest PKU request, `Capabilities` are requested manifest bits,
and `Evidence` states only what the manifest section proves. The tool does not
assert a signature or runtime measurement and does not mutate the ELF.

**Before (manual dispatch)**:
```rust
#![no_std]
extern crate alloc;

use abi::declare_manifest;
use ostd::syscall::{sys_recv, sys_send, MessageBuf};

declare_manifest!(spawn = true);

#[no_mangle]
pub extern "C" fn main() {
    let mut buf = MessageBuf::new();
    loop {
        if sys_recv(&mut buf, Some(100)).is_ok() {
            // Handle message...
            sys_send(buf.sender, &[0x00]).ok();
        }
    }
}
```

**After (app_entry! macro)**:
```rust
use ostd::{app_entry, clients::VfsClient, runtime::CellRuntime};

app_entry!(handler = run);

async fn run() {
    let vfs = VfsClient::new();
    let data = vfs.read_file("/data/config.txt").await.ok();
    println!("Config loaded");
}
```

**Pattern summary**:
- Use `app_entry!` for apps and `ostd::service::serve` for services
- Access services via typed client facades (`VfsClient`, `NetClient`, `InputClient`)
- `CellRuntime` handles manifest generation, permission sets, lifecycle
- Apps declare minimal syscall set; kernel enforces via allowlist

---

## I/O Trait Layers (embedded-io Integration)

Cellos integrates [`embedded-io`](https://docs.rs/embedded-io) for byte-stream I/O. The two systems serve distinct purposes and must not be conflated:

### Which trait system to use

| Layer | Use | Avoid |
|---|---|---|
| **Stream I/O** (byte streams) | `embedded_io::Read + Write + Seek` | Custom `ViRead`/`ViWrite` |
| **Hardware peripherals** (GPIO, I2C, SPI, ADC, PWM) | `Vi*` HAL traits | `embedded_io` (no coverage) |
| **Async IPC wire format** | `Box<[u8]>` owned buffers (Law 2) | `embedded_io_async` at Cell boundary |
| **Intra-cell async I/O** | `embedded_io_async::Read + Write` | (safe — borrow stays on Cell stack) |

### Rules for App Cell developers

- **Only import from `ostd::*`** — never import `embedded_io` directly in app code.
- `ostd::fs::File`, `ostd::io::Stdin`/`Stdout`, and `ostd::clients::TcpStream` already implement `embedded_io::Read + Write`. Pass them directly to ecosystem crates that accept `impl embedded_io::Read`.
- `embedded_io` is re-exported as `ostd::embedded_io` if explicit trait bounds are needed.

### Rules for Driver Cell developers

- Hardware device cells implement `Vi*` HAL traits (`ViGpio`, `ViI2c`, `ViSpi`, `ViAdc`, `ViPwm`, `ViCan`).
- Byte-stream devices (UART/serial, TCP, file) additionally implement `embedded_io::Read + Write` via the `OstdError` newtype bridge in `ostd::io`.
- A driver may implement both a `Vi*` trait and `embedded_io` traits if appropriate.

### ostd stream handles

| Handle | Traits implemented | Backed by |
|---|---|---|
| `ostd::io::Stdin` | `Read` | `sys_read` |
| `ostd::io::Stdout` | `Write` | `sys_log` |
| `ostd::fs::File` | `Read`, `Write` | `sys_read_cap`, VFS IPC |
| `ostd::clients::TcpStream` | `Read`, `Write` | IPC → net service |

> **Note — File Seek:** `embedded_io::Seek` on `File` requires a `SeekCap` syscall (not yet implemented). Adding it is a Law 1 change — two confirmations required. Until then, use `ReadGrant` IPC for offset-based reads.

---

## Deprecations & Breaking Changes

When changing public API in `libs/abi/` or `libs/wire/`:

```rust
#[deprecated(since = "0.3.0", note = "use ViAsyncFileSystem instead")]
pub trait ViFileSystem {
    // old impl
}

pub trait ViAsyncFileSystem {
    // new impl
}
```

**Changelog Entry** (in `docs/project-changelog.md`):
```markdown
## [0.3.0] - 2026-06-15
### Deprecated
- `ViFileSystem::open()` → use `ViAsyncFileSystem::open().await` instead
```

---

## Quick Reference Card

| Rule | Status | Enforcement |
|------|--------|-------------|
| `foo.rs` + `foo/` for new modules (`mod.rs` also exists) | ✅ GUIDELINE | Code review (no lint) |
| Static kernel locks are ranked | ✅ REQUIRED | `tools/mutable-globals --check`; run-time check in `test-hooks` |
| `target_arch` outside `kernel/src/arch/` ≤ 692 lines; no `asm!` outside it | ✅ REQUIRED | `scripts/check-hal-boundaries.sh` ceilings |
| rustfmt-clean | ✅ REQUIRED | `cargo fmt --all --check` (`.github/workflows/ci.yml:199`) |
| Kernel files listed in `kernel/residents.toml` | ✅ REQUIRED | `scripts/check-kernel-residents.py` |
| Tier-1 closure `unsafe` within baseline | ✅ REQUIRED | `cellos-sign --check --strict`; lower with `--write-baseline` |
| Owned buffers in async | ❌ FORBIDDEN | Compiler error |
| Unsafe requires SAFETY comment | ❌ FORBIDDEN | Code review |
| Ordinary Rust Cells can't use unsafe | ❌ FORBIDDEN | `#![forbid(unsafe_code)]`; audited FFI/runtime/driver exceptions only |
| Vi prefix for public traits | ✅ REQUIRED | Code review |
| Result<T, E> over panic! | ✅ REQUIRED | Code review |
| Implement Drop | ✅ REQUIRED | Code review |
| 200-300 LOC per file | ✅ GUIDELINE | Code review |

---

## See Also

- **CLAUDE.md** — Quick agent reference
- **patterns.md** — Deep patterns & examples
- **system-architecture.md** — System design
- **api-reference.md** — Full trait reference
- Specs: **docs/specs/0X-*.md** — Feature specifications
