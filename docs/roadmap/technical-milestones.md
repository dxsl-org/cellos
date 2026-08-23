# Technical Milestones

**Last updated**: 2026-08-23

This file is a compact status map. See
[completed-history.md](completed-history.md) for the condensed completion
ledger and [project-roadmap-legacy.md](../project-roadmap-legacy.md) for the
archived traceability notes.

| Area | Current Status |
|---|---|
| Kernel core | Active; size and boundary residue tracked by generated metrics and roadmap notes |
| HAL/arch | RV64, AArch64, and x86_64 have implementation and smoke/build evidence; RV32/AArch32 remain separate qualification tracks |
| HAL to kernel Rust ABI | Centralized in `hal/traits/arch/src/kernel_abi.rs`; boundary script rejects local HAL declarations |
| Boards | Seven active descriptors in `boards/`; placeholder-only docs for `q35-x86_32`, `virt-riscv32`, `virt-aarch32` |
| VFS and storage | Service path active; FAT/littlefs/redoxfs-related work remains split by backend maturity |
| Networking | Net service and net-broker pieces exist; broker routing/beacon/lease/enrollment wiring remains incomplete |
| Scripting | Lua is active; MicroPython is historical and absent from current workspace members |
| Hotswap/supervisor | Supervisor hotswap path has QEMU smoke evidence; continue keeping authority gates explicit |
| Security/trust | Signing mechanism exists; fleet enforcement and production key provisioning remain open; Tier 2 native domains have RV64 QEMU evidence behind `native-domains`, while production release remains gated by [Spec 22](../specs/22-native-domain-cell-implementation-gate.md) |

## Historical Milestones

Milestones 3.3 and 3.4 remain in the legacy roadmap for traceability. Treat 3.4
MicroPython text as an archived implementation snapshot, not a current supported
runtime.
