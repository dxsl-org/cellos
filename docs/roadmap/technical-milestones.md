# Technical Milestones

**Last updated**: 2026-10-08 (direction routing; prior technical snapshot retained)

The sole program is **Intel x86-64 C2C Anywhere** under
[ADR-0022](../decisions/0022-intel-x86-64-c2c-only-direction.md). Use
[current-focus.md](current-focus.md) for required outcomes and gaps, and the
[portfolio](../../.agents/plan-portfolio.md) for the next executable slice.
The following technical snapshot is retained evidence, not authorization to
continue all architectures or runtime programs. See
[completed-history.md](completed-history.md) for completion provenance.

| Area | Current Status |
|---|---|
| Kernel core | Active; size and boundary residue tracked by generated metrics and roadmap notes |
| HAL/arch | RV64, AArch64, and x86_64 have implementation and smoke/build evidence; RV32/AArch32 remain separate qualification tracks |
| HAL to kernel Rust ABI | Centralized in `hal/traits/arch/src/kernel_abi.rs`; boundary script rejects local HAL declarations |
| Boards | Existing descriptors retained; Intel x86-64 is the sole development target; physical Intel HCL remains empty |
| VFS and storage | Service path active; FAT32 (`/mnt/sd`), littlefs2 (`/data`), and CellosFS Native (`/srv`) remain split by backend maturity |
| Networking | Net service and net-broker pieces exist; broker routing/beacon/lease/enrollment wiring remains incomplete |
| Scripting | Lua is active; MicroPython is historical and absent from current workspace members |
| Hotswap/supervisor | Supervisor hotswap path has QEMU smoke evidence; continue keeping authority gates explicit |
| Security/trust | Signing mechanism exists; fleet enforcement and production key provisioning remain open; Tier 2 native domains have RV64 QEMU evidence behind `native-domains`, while production release remains gated by [Spec 22](../specs/22-native-domain-cell-implementation-gate.md) |
| Cell scale profiles (D5) | Large-app profile stays the default (`MAX_CELLS = 64`); the per-request server profile is an accepted goal, not capacity. The 2026-07-31 ceiling (n = 8–9) was a hardcoded 190 MiB memory map — now replaced by firmware DTB discovery (`kernel/src/boot/dtb_memory.rs`) with `MemInfo = 243` making capacity measurable. Still open: shared immutable image frames, demand-paged stacks, dynamic tables, a variable VA budget, and the N = 64/128/256/512 baselines re-measured with heavy cells resident. See [Spec 19 §3](../specs/19-hardware-isolation-layers.md) |

## Historical Milestones

Milestones 3.3 and 3.4 remain in the legacy roadmap for traceability. Treat 3.4
MicroPython text as an archived implementation snapshot, not a current supported
runtime.
