# Controlled Lock Retirement & Subordinate Rebase Plan

**Framework:** Law-1 Dual-Checkpoint Protocol (Post-CP2 Execution Phase)  
**Authority Base:** `.agents/cell-contract-0024-locks.md` & `.agents/261011-1200-cell-contract-authority/plan.md`  
**Status:** DRAFT GOVERNANCE REBASE ROADMAP  
**Recorded:** 2026-10-16  

---

## 1. Principles of Controlled Lock Retirement

Following the formal ratification of **Law-1 Checkpoint 2 (CP2)**, the 38 plans previously blocked by `CELL-CONTRACT-0024` are eligible for structured, phased retirement.

To prevent architectural regression, lock release adheres strictly to the four inviolable rules:
1. **No Blanket Unlock:** Documentation completion, green historical tests, or finishing the controlling plan does NOT automatically unlock all legacy slices.
2. **Contract Rebase Required:** Every subordinate plan must rebase its scope against Spec 25 v1.3 (AC-01..AC-50), Spec 26 v1.1, and the ratified Wire ABI (CP1/CP2).
3. **Atomic Relation Update:** Each unlocked slice must update its `executionLock`, `executionStatus`, and `blockedBy` entries simultaneously with updating `cell-contract-0024-locks.md` and passing `scripts/verify-cell-contract-p01.py --negative-controls`.
4. **Clean Cutover Invariance:** Legacy compatibility shims, duplicate capability registries (`CapTable`), or ambient authority loopholes are forbidden.

---

## 2. Phased Retirement Tranches

The 38 execution locks are partitioned into 4 sequential tranches based on architectural dependency:

```
[CP2 Ratified Baseline]
         │
         ▼
┌────────────────────────────────────────────────────────┐
│ Tranche 1: VFS, Storage & Buffer Lifetimes (7 plans)    │
└────────────────────────────────────────────────────────┘
         │
         ▼
┌────────────────────────────────────────────────────────┐
│ Tranche 2: Core Kernel, Authority & Loader (8 plans)   │
└────────────────────────────────────────────────────────┘
         │
         ▼
┌────────────────────────────────────────────────────────┐
│ Tranche 3: Native SDK, Runtime & Concurrency (11 plans)│
└────────────────────────────────────────────────────────┘
         │
         ▼
┌────────────────────────────────────────────────────────┐
│ Tranche 4: Distributed, VMM & Hardware Lanes (12 plans)│
└────────────────────────────────────────────────────────┘
```

---

### Tranche 1: VFS, Storage & Buffer Lifetimes (7 Plans)
Focuses on file system hardening, revocable directory access, and borrowed-buffer memory fences.

| Plan ID & Link | Original Scope | Rebase Criteria against CP1/CP2 |
|---|---|---|
| `260603-1803-phase-h-vfs-hardening` | VFS permission & mutation | Rebase onto CSpace file slots (`D01`) & `ViDirHandleAttestation` (`D05`). |
| `260605-1538-milestone-2-1-vfs-complete` | Async-read integration | Consume async operation submit/reply (`D04`) without kernel-side ambient authority. |
| `260809-0922-sas-lbi-revocable-vfs-access` | Revocable VFS access | Rebase onto explicit CSpace slot attenuation and handle revocation. |
| `260827-1015-sas-caller-owned-range-ledger` | Caller-owned range ledger | Rebase onto bounded borrowed-buffer grant lifetimes (`D06`). |
| `260801-midori-phase01-completion` | ReadGrant & fast-IPC | Rebase onto grant quarantine and single-source completion. |
| `260801-parallel-midori-closure` | Peer-death & merge gates | Ensure peer termination funnels through whole-Cell fault model (`D07`). |
| `260727-2101-midori-lessons-cellos` | Async VFS/DMA followups | Adopt verified A/B superblock journal & AuthorityGate lifecycle. |

---

### Tranche 2: Core Kernel, Authority & Loader (8 Plans)
Focuses on cleaning up legacy capability registries, loader isolation, and cross-hart TLB fences.

| Plan ID & Link | Original Scope | Rebase Criteria against CP1/CP2 |
|---|---|---|
| `260621-0830-cell-perms-p2-p5` | Cell perms & delegation | Eliminate legacy `CapSet`; strictly use single CSpace under scheduler lock (`D03`). |
| `260624-0630-kernel-boundary-cleanup` | Kernel vs Driver Cell | Enforce zero driver ambient authority; drivers receive only explicit grant handles. |
| `260707-1726-g2-loader-redesign` | Loader address access | Load userland binaries via BootFS requests without direct kernel fd bypass (`D09`). |
| `260712-1900-manifest-v2` | Manifest cap_args | Enforce static syscall allowlists and immutable capacity ceiling declarations (`D11`). |
| `260808-1544-wx-cross-hart-tlb-shootdown` | Cross-hart TLB shootdown | Verify non-blocking shootdown fences under SMP multi-CPU scheduler (`P03`). |
| `260822-phase08-manifest-predesign` | Manifest predesign | Rebase manifest v2 parsing against wire-safe byte representations. |
| `260927-0739-kernel-architecture-repair` | Architecture repairs | Verify complete removal of legacy `CapTable` and obsolete syscall branches. |
| `260528-2016-vicell-full-implementation` | Legacy vicell umbrella | Archive completed slices; discard superseded ambient authority designs. |

---

### Tranche 3: Native SDK, Runtime & Concurrency (11 Plans)
Focuses on userland application developer interfaces, standard library PAL, and high-concurrency reactors.

| Plan ID & Link | Original Scope | Rebase Criteria against CP1/CP2 |
|---|---|---|
| `260615-1325-vicell-app-sdk-v1` | App SDK v1 (`AppContext`) | Rebase SDK client dispatchers onto CP1 48-byte bounded submit requests (`D04`). |
| `260722-0917-g4-full-std-tier1` | Standard library PAL | Port PAL I/O primitives strictly to neutral stream mechanisms (`D08`). |
| `260821-0642-app-tiers-completion` | Application tier admission | Enforce Tier 1/2/3 separation without ambient POSIX linkage (`D09/N07`). |
| `260821-1700-tier1-baseline-admission` | Tier 1 baseline admission | Verify static manifest verification before application spawn. |
| `260821-1800-tier1-rust-std-pal-feasibility`| Rust std PAL feasibility | Consume verified CSpace file handles and pipe stream descriptors. |
| `260825-sdk-delivery` | SDK client delivery | Ensure typed caller wrappers do not expose raw pointers in safe Rust. |
| `260607-1200-phase-27-protocol-hardening` | Protocol hardening | Verify typed wire serialization against Law-1 CP1 schema constraints. |
| `260605-1406-phase28-wasm-cells-epmp` | WASM cells host authority | Bind WASM runtime to strict CSpace capability sandbox. |
| `261006-0934-httpd-single-cell-concurrency` | HTTPD concurrency | Utilize multi-waiter completion queues (`D04`) and bounded pipe channels (`D08`). |
| `260827-1004-hardware-independent-roadmap` | Hardware roadmap | Rebase roadmap milestones onto Intel C2C priority (`ADR-0022`). |
| `260905-1139-sas-lbi-outcome-closure` | SAS LBI outcome closure | Rebase outcome transactions onto monotonic epoch-fenced state stash (`P06`). |

---

### Tranche 4: Distributed, VMM & Hardware Lanes (12 Plans)
Focuses on multi-node C2C transport, Intel VMX virtualization, and physical bare-metal hardware.

| Plan ID & Link | Original Scope | Rebase Criteria against CP1/CP2 |
|---|---|---|
| `260711-1917-tier3b-x86-vtx` | Intel VMX/EPT guest | Rebase onto verified VMX root operation and zero-ambient guest bridge (`P09`). |
| `260712-0952-tier3b-vm-hardening-compat` | VM hardening & compat | Enforce guest UID 0 unprivileged invariant; invalidate stale generation replies. |
| `260722-2330-tier3b-finish-g5-lite` | Guest clone / VMID | Preserve guest generation tracking without host capability escalation. |
| `260712-0800-supervisory-cell-migration` | Supervisory cell | Rebase supervisory operations onto Budget Authority and Placement Controller. |
| `260712-1000-cell-package-distribution` | Package distribution | Integrate with authenticated C2C transport envelopes (`D12`). |
| `260712-1902-dice-attestation-identity` | DICE attestation | Rebase attestation identities onto NCF1 Ed25519 proof carriers. |
| `260819-1416-port-common-drivers-g1-g2-g3`| Common driver porting | Enforce strict IOMMU/DMA isolation and bounded MMIO mappings (`D06`). |
| `260825-1726-kms-silo-production-root` | KMS/Silo production root| Integrate with service-net broker TLS relay and immutable origin proofs. |
| `260826-1605-phase4-dev-reference-authority`| Dev reference authority | Rebase onto independent Budget Authority multi-vector ledger (`P08-C`). |
| `260927-1100-c2c-anywhere-tier-aware` | C2C anywhere | Rebase cluster calls onto 112-byte C2C transport and 6-gate ingress core. |
| `261004-1957-x86-pc-lane` | Physical x86 PC lane | Execute bare-metal boot and hardware qualification on physical Intel PC. |
| `260605-2107-full-reliability-track` | Reliability track | Verify whole-node crash recovery and A/B superblock resilience. |

---

## 3. Execution Procedure for Unlocking a Plan

To unlock any specific plan among the 38:
1. **Rebase Assessment:** Conduct an audit comparing the subordinate plan's proposed syscalls, structures, and assumptions against the ratified CP1/CP2 specifications.
2. **Draft Rebase Amendment:** Document the specific cutovers (e.g. replacing legacy calls with `D01` or `D04`).
3. **User Ratification:** User approves the specific rebase handoff.
4. **Synchronous Lock Retirement:**
   - In subordinate `plan.md`: change `executionStatus: unblocked`, remove `CELL-CONTRACT-0024` from `blockedBy`.
   - In `.agents/cell-contract-0024-locks.md`: record the unlocked status and date in the exact block table.
   - Run `python3 scripts/verify-cell-contract-p01.py --negative-controls` to verify repository-wide consistency.
